//! Deciding a `tools/call`: the local destructive-action check, then the
//! `PromptGuard` API, then the fail mode when the API gives no verdict.

use super::destructive::is_destructive;
use crate::api::{PromptGuardClient, RawResponse};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;

/// How long one API request from the proxy may take before its answer is
/// treated as missing. Shorter than the client's 30s default: an agent waits
/// on every tool call, and the fail mode already says what to do when no
/// verdict comes.
pub const CHECK_TIMEOUT: Duration = Duration::from_secs(10);

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

/// Why the API gave no usable verdict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApiFailure {
    /// No answer, or a 5xx: an outage. The fail mode decides.
    Unavailable(String),
    /// 429, the plan's limit. Not an outage, but blocking every tool when a
    /// plan runs out is what ADR-0034 rejects, so the fail mode decides; it is
    /// logged as a plan limit, not as an outage.
    PlanLimit(String),
    /// The endpoint does not take this content (too large, or a shape it
    /// refuses): the proxy's own check of the documented limits, or a 413 or
    /// 422 from an endpoint whose input the wrapped server wrote (a tool
    /// result, a tool list). No verdict, so the fail mode decides, as for an
    /// outage: refusing it in every mode would let a server break the agent
    /// by padding what it sends.
    ContentRefused(String),
    /// The API answered and the answer cannot be used: a rejected key,
    /// another 4xx, or a 200 that is not a verdict. Refused in every mode: a
    /// revoked key must not turn the proxy into a pass-through.
    Unusable(String),
}

impl ApiFailure {
    pub fn message(&self) -> &str {
        match self {
            Self::Unavailable(m)
            | Self::PlanLimit(m)
            | Self::ContentRefused(m)
            | Self::Unusable(m) => m,
        }
    }
}

/// Where a tool call's verdict comes from when the API is consulted.
pub trait Validator: Send + Sync {
    /// The API's verdict, or why there is none.
    fn validate(&self, tool: &str, arguments: &Value) -> Result<ToolValidation, ApiFailure>;
}

/// The validate-tool endpoint, through the CLI's own HTTP client.
pub struct ApiValidator {
    pub client: Arc<PromptGuardClient>,
    pub agent_id: String,
    pub session_id: String,
}

impl Validator for ApiValidator {
    fn validate(&self, tool: &str, arguments: &Value) -> Result<ToolValidation, ApiFailure> {
        let body = serde_json::json!({
            "agent_id": self.agent_id,
            "tool_name": tool,
            "arguments": arguments,
            "session_id": self.session_id,
        });
        let response = self.client.send_raw(
            &reqwest::Method::POST,
            "/agent/validate-tool",
            Some(&body),
            CHECK_TIMEOUT,
        );
        classify(response)
    }
}

/// Turn the API's answer into a verdict, or say which kind of failure it is.
pub fn classify(response: RawResponse) -> Result<ToolValidation, ApiFailure> {
    classify_answer(response, "check the call", false)
}

/// Read the API's answer as a `T`, or say which kind of failure it is. With
/// `content_refusals`, a 413 or 422 is [`ApiFailure::ContentRefused`]: for an
/// endpoint whose input the wrapped server wrote, which the proxy checks
/// against the documented limits before sending but cannot measure exactly.
pub fn classify_answer<T: DeserializeOwned>(
    response: RawResponse,
    what: &str,
    content_refusals: bool,
) -> Result<T, ApiFailure> {
    let (status, body) = match response {
        RawResponse::NoAnswer(e) => {
            return Err(ApiFailure::Unavailable(format!("API unreachable: {e}")))
        },
        RawResponse::Answered { status, body } => (status, body),
    };
    match status {
        200..=299 => serde_json::from_str(&body).map_err(|e| {
            ApiFailure::Unusable(format!("the API's answer was malformed ({status}: {e})"))
        }),
        413 | 422 if content_refusals => Err(ApiFailure::ContentRefused(format!(
            "the API could not {what} ({status}: {})",
            error_message(&body)
        ))),
        401 | 403 => Err(ApiFailure::Unusable(format!(
            "PromptGuard rejected the API key ({status}: {}); run `promptguard login` with a valid key",
            error_message(&body)
        ))),
        429 => Err(ApiFailure::PlanLimit(format!(
            "the plan limit was reached (429: {})",
            error_message(&body)
        ))),
        500..=599 => Err(ApiFailure::Unavailable(format!("the API returned {status}"))),
        _ => Err(ApiFailure::Unusable(format!(
            "the API could not {what} ({status}: {})",
            error_message(&body)
        ))),
    }
}

/// The message of the API's error envelope, or the start of the body.
fn error_message(body: &str) -> String {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| v["error"]["message"].as_str().map(str::to_string))
        .unwrap_or_else(|| body.chars().take(200).collect())
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
/// | either      | unusable     | refuse    | refuse | refuse    |
///
/// "Unreachable" is an outage or the plan limit; "unusable" is a rejected
/// key, another 4xx, or a malformed answer (see [`ApiFailure`]).
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
        Err(ApiFailure::Unusable(reason)) => Decision::Refuse { reason },
        Err(
            ApiFailure::Unavailable(e) | ApiFailure::PlanLimit(e) | ApiFailure::ContentRefused(e),
        ) => match mode {
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
        fn validate(&self, _: &str, _: &Value) -> Result<ToolValidation, ApiFailure> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            match self.verdict {
                Some(allowed) => Ok(ToolValidation {
                    allowed,
                    reason: if allowed { String::new() } else { "no".into() },
                    blocked_reasons: vec![],
                }),
                None => Err(ApiFailure::Unavailable("connection refused".into())),
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

    struct Failing(ApiFailure);

    impl Validator for Failing {
        fn validate(&self, _: &str, _: &Value) -> Result<ToolValidation, ApiFailure> {
            Err(self.0.clone())
        }
    }

    /// The table's last row, and the plan limit following the fail mode.
    #[test]
    fn unusable_answers_are_refused_in_every_mode() {
        let read = json!({"path": "README.md"});
        for mode in [FailMode::Open, FailMode::Closed, FailMode::Mixed] {
            let d = decide(
                mode,
                "t",
                &read,
                &Failing(ApiFailure::Unusable("401".into())),
            );
            assert!(matches!(d, Decision::Refuse { .. }), "{mode:?}: {d:?}");
            let d = decide(
                mode,
                "t",
                &read,
                &Failing(ApiFailure::PlanLimit("429".into())),
            );
            let refused = matches!(d, Decision::Refuse { .. });
            assert_eq!(refused, mode == FailMode::Closed, "{mode:?}: {d:?}");
        }
    }

    #[test]
    fn only_an_outage_or_the_plan_limit_is_left_to_the_fail_mode() {
        let answered = |status: u16, body: &str| {
            classify(RawResponse::Answered {
                status,
                body: body.to_string(),
            })
        };
        let unavailable =
            |r: &Result<ToolValidation, ApiFailure>| matches!(r, Err(ApiFailure::Unavailable(_)));
        let unusable =
            |r: &Result<ToolValidation, ApiFailure>| matches!(r, Err(ApiFailure::Unusable(_)));

        assert!(unavailable(&classify(RawResponse::NoAnswer(
            "timed out".into()
        ))));
        assert!(unavailable(&answered(500, "")));
        assert!(unavailable(&answered(504, "")));
        assert!(matches!(answered(429, ""), Err(ApiFailure::PlanLimit(_))));
        for status in [400, 401, 403, 404, 422] {
            assert!(unusable(&answered(status, "")), "{status}");
        }
        assert!(unusable(&answered(200, r#"{"ok":true}"#)));
        assert!(unusable(&answered(200, "not json")));
        assert!(answered(200, r#"{"allowed":true}"#).is_ok_and(|v| v.allowed));
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
