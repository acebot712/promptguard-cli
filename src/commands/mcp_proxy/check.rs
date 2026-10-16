//! Deciding a `tools/call`: the local destructive-action check, then the
//! `PromptGuard` API, then the fail mode when the API gives no verdict.

use super::destructive::is_destructive;
use crate::api::PromptGuardClient;
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;

/// How long one validate-tool request may take before the call is treated as
/// unchecked. Shorter than the client's 30s default: an agent waits on every
/// tool call, and the fail mode already says what to do when no verdict comes.
const CHECK_TIMEOUT: Duration = Duration::from_secs(10);

/// What the proxy does with a tool call when the API cannot give a verdict
/// (ADR-0034).
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum FailMode {
    /// Relay every call, logged as unchecked
    Open,
    /// Refuse every call
    Closed,
    /// Refuse calls the local destructive-action check matches; relay the
    /// rest, logged as unchecked (default)
    Mixed,
}

impl FailMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Closed => "closed",
            Self::Mixed => "mixed",
        }
    }
}

/// The verdict for one call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Checked and allowed: forward it.
    Relay,
    /// Not checked (the API gave no verdict) and the fail mode lets it
    /// through: forward it, and log why.
    RelayUnchecked { why: String },
    /// Answer the client with JSON-RPC -32600 and never forward it.
    Refuse { reason: String },
}

/// `POST /agent/validate-tool`'s response (`ToolCallResponse` in the
/// backend's `developer/agent/router.py`). Hand-written, as AGENTS.md
/// explains for every response type in this crate.
#[derive(Debug, Deserialize)]
pub struct ToolValidation {
    pub allowed: bool,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub blocked_reasons: Vec<String>,
}

/// Where a tool call's verdict comes from when the API is consulted.
pub trait Validator: Send + Sync {
    /// The API's verdict, or why there is none.
    fn validate(&self, tool: &str, arguments: &Value) -> Result<ToolValidation, String>;
}

/// The validate-tool endpoint, through the CLI's own HTTP client.
pub struct ApiValidator {
    pub client: Arc<PromptGuardClient>,
    pub agent_id: String,
    pub session_id: String,
}

impl Validator for ApiValidator {
    fn validate(&self, tool: &str, arguments: &Value) -> Result<ToolValidation, String> {
        self.client
            .post_with_timeout(
                "/agent/validate-tool",
                &serde_json::json!({
                    "agent_id": self.agent_id,
                    "tool_name": tool,
                    "arguments": arguments,
                    "session_id": self.session_id,
                }),
                CHECK_TIMEOUT,
            )
            .map_err(|e| e.to_string())
    }
}

const DESTRUCTIVE: &str = "the arguments match PromptGuard's destructive-action check";

/// Decide one call. The local check runs first; a match is refused without a
/// round trip unless the mode is `open`, which must learn whether the API is
/// reachable before it may let anything through.
///
/// | local check | API          | open      | closed | mixed     |
/// |-------------|--------------|-----------|--------|-----------|
/// | matches     | any verdict  | refuse    | refuse | refuse    |
/// | matches     | unreachable  | unchecked | refuse | refuse    |
/// | no match    | allows       | relay     | relay  | relay     |
/// | no match    | refuses      | refuse    | refuse | refuse    |
/// | no match    | unreachable  | unchecked | refuse | unchecked |
pub fn decide(mode: FailMode, tool: &str, arguments: &Value, api: &dyn Validator) -> Decision {
    let destructive = is_destructive(arguments);

    if destructive && mode != FailMode::Open {
        return Decision::Refuse {
            reason: DESTRUCTIVE.to_string(),
        };
    }

    match api.validate(tool, arguments) {
        Ok(v) if !v.allowed => Decision::Refuse {
            reason: refusal_reason(&v),
        },
        Ok(_) if destructive => Decision::Refuse {
            reason: DESTRUCTIVE.to_string(),
        },
        Ok(_) => Decision::Relay,
        Err(e) => match mode {
            FailMode::Closed => Decision::Refuse {
                reason: format!("the call could not be checked and --fail-mode is closed ({e})"),
            },
            FailMode::Open | FailMode::Mixed => Decision::RelayUnchecked { why: e },
        },
    }
}

fn refusal_reason(v: &ToolValidation) -> String {
    if !v.reason.is_empty() {
        return v.reason.clone();
    }
    if !v.blocked_reasons.is_empty() {
        return v.blocked_reasons.join("; ");
    }
    "refused by PromptGuard".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Fake {
        verdict: Option<bool>,
        calls: AtomicUsize,
    }

    impl Validator for Fake {
        fn validate(&self, _: &str, _: &Value) -> Result<ToolValidation, String> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            match self.verdict {
                Some(allowed) => Ok(ToolValidation {
                    allowed,
                    reason: if allowed { String::new() } else { "no".into() },
                    blocked_reasons: vec![],
                }),
                None => Err("connection refused".into()),
            }
        }
    }

    fn fake(verdict: Option<bool>) -> Fake {
        Fake {
            verdict,
            calls: AtomicUsize::new(0),
        }
    }

    /// Every cell of the table in `decide`'s documentation.
    #[test]
    fn decision_table() {
        type Check = fn(&Decision) -> bool;
        type Case<'a> = (&'a Value, Option<bool>, [Check; 3]);

        let rm = json!({"command": "rm -rf /"});
        let read = json!({"path": "README.md"});
        let relay = |d: &Decision| *d == Decision::Relay;
        let unchecked = |d: &Decision| matches!(d, Decision::RelayUnchecked { .. });
        let refuse = |d: &Decision| matches!(d, Decision::Refuse { .. });

        let cases: [Case; 6] = [
            (&rm, Some(true), [refuse, refuse, refuse]),
            (&rm, Some(false), [refuse, refuse, refuse]),
            (&rm, None, [unchecked, refuse, refuse]),
            (&read, Some(true), [relay, relay, relay]),
            (&read, Some(false), [refuse, refuse, refuse]),
            (&read, None, [unchecked, refuse, unchecked]),
        ];
        for (args, verdict, expected) in cases {
            for (mode, ok) in [FailMode::Open, FailMode::Closed, FailMode::Mixed]
                .into_iter()
                .zip(expected)
            {
                let d = decide(mode, "tool", args, &fake(verdict));
                assert!(ok(&d), "{mode:?} {args} api={verdict:?} gave {d:?}");
            }
        }
    }

    #[test]
    fn destructive_calls_skip_the_round_trip_unless_open() {
        let rm = json!({"command": "rm -rf /"});
        for (mode, round_trips) in [
            (FailMode::Open, 1),
            (FailMode::Closed, 0),
            (FailMode::Mixed, 0),
        ] {
            let api = fake(Some(true));
            decide(mode, "tool", &rm, &api);
            assert_eq!(api.calls.load(Ordering::Relaxed), round_trips, "{mode:?}");
        }
    }
}
