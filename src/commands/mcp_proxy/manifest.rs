//! Scanning a server's tool list: every `tools/list` result is sent to the
//! `PromptGuard` API's manifest-scan endpoint with the pin stored for that
//! server, and its findings are reported before the agent sees the list.
//!
//! What the proxy does with an answer, and why:
//!
//! - **A `high` finding on a named tool** (`E001` poisoning, `E002`
//!   shadowing) removes that tool from the list, in every fail mode. It is a
//!   verdict, and ADR-0034 enforces verdicts in every mode, as it does a
//!   refused tool call. Removing the one tool, rather than refusing the list,
//!   keeps the rest of the server working; the harm of a poisoned description
//!   is done when the model reads it, so it must not be relayed at all.
//! - **A rug-pull (`E301`)** is reported, naming the tools that changed, and
//!   the list is relayed. The stored pin is kept, so every run reports it
//!   until the developer trusts the new list by removing the server's entry
//!   from the pins file. A server upgrade changes its tools too (`npx -y`
//!   fetches the latest on every start), and withholding every changed tool
//!   would break the agent on each one, which is how a proxy gets removed.
//! - **Other findings** (`W021`, `W022`, `E003`, `E900`) are reported only.
//! - **No verdict** (the API unreachable, a 5xx, the plan limit, a list over
//!   the endpoint's limits, or tools an incomplete scan did not reach) is the
//!   fail mode's call: `closed` refuses the list, or withholds the unscanned
//!   tools; `mixed` and `open` relay it, logged as unchecked. `mixed` blocks
//!   destructive *calls*, and a call to any tool is still checked on its own.
//! - **An answer that is not a verdict** (a rejected key, another 4xx, a
//!   malformed body) refuses the list in every mode.

use super::check::{classify_answer, ApiFailure, FailMode, CHECK_TIMEOUT};
use super::pins::{Pin, ToolHash};
use crate::api::PromptGuardClient;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::sync::Arc;

/// The endpoint's limits (`developer/mcp/router.py`): tools per list, the
/// characters of their canonical JSON, and a tool name's length.
const MAX_TOOLS: usize = 500;
const MAX_MANIFEST_CHARS: usize = 131_072;
const MAX_NAME_CHARS: usize = 256;

/// `POST /mcp/manifest-scan`'s answer (`ManifestScanResponse`).
#[derive(Debug, Deserialize)]
pub struct ManifestScan {
    pub sha256: String,
    pub findings: Vec<Finding>,
    pub tools: Vec<ToolHash>,
    #[serde(default)]
    pub unscanned_tools: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct Finding {
    pub code: String,
    pub tool: String,
    pub severity: String,
    pub reason: String,
    #[serde(default)]
    pub details: Value,
}

/// Where a tool list is scanned.
pub trait ManifestScanner: Send + Sync {
    fn scan(&self, tools: &[Value], pin: Option<&str>) -> Result<ManifestScan, ApiFailure>;
}

pub struct ApiManifestScanner {
    pub client: Arc<PromptGuardClient>,
}

impl ManifestScanner for ApiManifestScanner {
    fn scan(&self, tools: &[Value], pin: Option<&str>) -> Result<ManifestScan, ApiFailure> {
        let mut body = json!({"tools": tools});
        if let Some(pin) = pin {
            body["pin"] = json!(pin);
        }
        let response = self.client.send_raw(
            &reqwest::Method::POST,
            "/mcp/manifest-scan",
            Some(&body),
            CHECK_TIMEOUT,
        );
        classify_answer(response, "scan the tool list", true)
    }
}

/// The verdict on one `tools/list` result.
#[derive(Debug, Default)]
pub struct Review {
    /// Refuse the list with this reason; the rest of the review is moot.
    pub refuse: Option<String>,
    /// Names to remove from the list before relaying it.
    pub withhold: BTreeSet<String>,
    /// Lines for stderr.
    pub log: Vec<String>,
    /// The pin to store, when there was none.
    pub new_pin: Option<Pin>,
    /// The scan answered, and the list matches its stored pin.
    pub matches_pin: bool,
}

/// Where the scan's context comes from: which server, its stored pin, and
/// whether this page of the list is the one pinned.
pub struct Context<'a> {
    pub mode: FailMode,
    pub server: &'a str,
    pub pins_path: &'a str,
    /// The stored pin, for the first page only: a later page is scanned for
    /// findings but neither compared nor pinned, since its cursor is opaque.
    pub stored: Option<&'a Pin>,
    pub first_page: bool,
    pub now: String,
}

pub fn review(ctx: &Context, tools: &[Value], api: &dyn ManifestScanner) -> Review {
    let mut review = Review::default();
    let scan = within_limits(tools).and_then(|()| {
        api.scan(
            tools,
            ctx.stored
                .filter(|_| ctx.first_page)
                .map(|p| p.sha256.as_str()),
        )
    });
    let scan = match scan {
        Ok(scan) => scan,
        Err(ApiFailure::Unusable(reason)) => {
            review.refuse = Some(reason);
            return review;
        },
        Err(e) => {
            if ctx.mode == FailMode::Closed {
                review.refuse = Some(format!(
                    "the tool list could not be checked and --fail-mode is closed ({})",
                    e.message()
                ));
            } else {
                review.log.push(format!(
                    "unchecked: relayed the tool list of `{}` without a PromptGuard scan \
                     (fail mode {}): {}",
                    ctx.server,
                    ctx.mode.as_str(),
                    e.message()
                ));
            }
            return review;
        },
    };

    for finding in &scan.findings {
        let named = finding.tool != "*";
        let withheld = named && finding.severity == "high";
        if withheld {
            review.withhold.insert(finding.tool.clone());
        }
        let mut line = format!(
            "tool list finding {} ({}) on `{}`: {}",
            finding.code, finding.severity, finding.tool, finding.reason
        );
        if let Some(rules) = finding
            .details
            .get("matched_rules")
            .and_then(Value::as_array)
        {
            let rules: Vec<&str> = rules.iter().filter_map(Value::as_str).collect();
            if !rules.is_empty() {
                let _ = write!(line, " (rules {})", rules.join(", "));
            }
        }
        if finding.code == "E301" {
            if let Some(stored) = ctx.stored {
                line.push_str(&changes(&stored.tools, &scan.tools));
            }
            let _ = write!(
                line,
                ". The list is relayed and the old pin kept; if you trust the new list, \
                 remove the `{}` entry from {} (or delete the file to re-pin every server)",
                ctx.server, ctx.pins_path
            );
        }
        if withheld {
            line.push_str("; the tool was withheld from the agent");
        }
        review.log.push(line);
    }

    if !scan.unscanned_tools.is_empty() {
        let names = scan.unscanned_tools.join(", ");
        if ctx.mode == FailMode::Closed {
            review.withhold.extend(scan.unscanned_tools.iter().cloned());
            review.log.push(format!(
                "withheld tools the scan did not reach, as --fail-mode is closed: {names}"
            ));
        } else {
            review.log.push(format!(
                "unchecked: relayed tools the scan did not reach (fail mode {}): {names}",
                ctx.mode.as_str()
            ));
        }
    }

    if ctx.first_page {
        match ctx.stored {
            None => {
                review.new_pin = Some(Pin {
                    server: ctx.server.to_string(),
                    sha256: scan.sha256.clone(),
                    tools: scan.tools,
                    pinned_at: ctx.now.clone(),
                });
            },
            Some(stored) => review.matches_pin = stored.sha256 == scan.sha256,
        }
    }
    review
}

/// "; changed: a; added: b; removed: c", from the per-tool hashes.
fn changes(pinned: &[ToolHash], current: &[ToolHash]) -> String {
    let hash_of = |list: &[ToolHash], name: &str| {
        list.iter()
            .find(|t| t.name == name)
            .map(|t| t.sha256.clone())
    };
    let names = |list: &[ToolHash]| list.iter().map(|t| t.name.clone()).collect::<BTreeSet<_>>();
    let (before, after) = (names(pinned), names(current));
    let changed: Vec<String> = after
        .intersection(&before)
        .filter(|n| hash_of(pinned, n) != hash_of(current, n))
        .cloned()
        .collect();
    let added: Vec<String> = after.difference(&before).cloned().collect();
    let removed: Vec<String> = before.difference(&after).cloned().collect();
    let mut out = String::new();
    for (label, list) in [("changed", changed), ("added", added), ("removed", removed)] {
        if !list.is_empty() {
            let _ = write!(out, "; {label}: {}", list.join(", "));
        }
    }
    out
}

/// Refuse to send what the endpoint would refuse with a 422, as a list it
/// cannot scan rather than an unusable answer: the tools' shape, their count,
/// and the characters of their canonical JSON. The count is the endpoint's
/// canonical form re-created closely (every non-ASCII character escaped), not
/// exactly; a list near the limit that the endpoint still refuses comes back
/// as a 422, which is treated the same way.
fn within_limits(tools: &[Value]) -> Result<(), ApiFailure> {
    let too_large = |why: String| Err(ApiFailure::ContentRefused(why));
    if tools.len() > MAX_TOOLS {
        return too_large(format!(
            "the list has {} tools; the scan takes {MAX_TOOLS}",
            tools.len()
        ));
    }
    let mut chars = 0;
    for tool in tools {
        let name = tool.get("name").and_then(Value::as_str).unwrap_or_default();
        if name.is_empty() || name.chars().count() > MAX_NAME_CHARS {
            return too_large(format!(
                "a tool's name is empty or over {MAX_NAME_CHARS} characters"
            ));
        }
        let description = match tool.get("description") {
            None | Some(Value::Null) => "",
            Some(Value::String(d)) => d,
            Some(_) => return too_large("a tool's description is not text".to_string()),
        };
        let schema = match tool.get("inputSchema").or_else(|| tool.get("input_schema")) {
            None | Some(Value::Null) => json!({}),
            Some(s @ Value::Object(_)) => s.clone(),
            Some(_) => return too_large("a tool's inputSchema is not an object".to_string()),
        };
        let canonical =
            json!({"description": description, "input_schema": schema, "name": name}).to_string();
        chars += canonical
            .chars()
            .map(|c| match c.len_utf16() {
                _ if c.is_ascii() => 1,
                1 => 6,
                _ => 12,
            })
            .sum::<usize>();
    }
    if chars > MAX_MANIFEST_CHARS {
        return too_large(format!(
            "the list is about {chars} characters; the scan takes {MAX_MANIFEST_CHARS}"
        ));
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn hash(name: &str, sha: &str) -> ToolHash {
        ToolHash {
            name: name.into(),
            sha256: sha.into(),
        }
    }

    #[test]
    fn changes_name_each_tool_that_moved() {
        let pinned = [hash("a", "1"), hash("b", "2"), hash("c", "3")];
        let current = [hash("a", "1"), hash("b", "9"), hash("d", "4")];
        assert_eq!(
            changes(&pinned, &current),
            "; changed: b; added: d; removed: c"
        );
        assert_eq!(changes(&pinned, &pinned), "");
    }

    #[test]
    fn lists_the_endpoint_would_refuse_are_too_large_not_unusable() {
        let ok = json!({"name": "t", "description": "d", "inputSchema": {"type": "object"}});
        assert!(within_limits(std::slice::from_ref(&ok)).is_ok());
        for bad in [
            json!({"description": "no name"}),
            json!({"name": "x".repeat(257)}),
            json!({"name": "t", "description": 3}),
            json!({"name": "t", "inputSchema": []}),
            json!({"name": "t", "description": "é".repeat(MAX_MANIFEST_CHARS / 6 + 1)}),
        ] {
            assert!(
                matches!(
                    within_limits(std::slice::from_ref(&bad)),
                    Err(ApiFailure::ContentRefused(_))
                ),
                "{bad}"
            );
        }
        let many: Vec<Value> = (0..=MAX_TOOLS)
            .map(|i| json!({"name": format!("t{i}")}))
            .collect();
        assert!(within_limits(&many).is_err());
    }
}
