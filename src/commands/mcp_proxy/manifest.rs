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
//! - **A rug-pull (`E301`)** is reported, naming the server and the tools
//!   that changed. Under `closed` the changed and added tools are withheld
//!   from the list; under `open` and `mixed` the list is relayed, since a
//!   server upgrade changes its tools too (`npx -y` fetches the latest on
//!   every start) and withholding every changed tool would break the agent on
//!   each one, which is how a proxy gets removed. The stored pin is kept, so
//!   every run reports it until the developer accepts the new list: `--repin`
//!   on one run, or removing the server's entry from the pins file.
//! - **Other findings** (`W021`, `W022`, `E003`, `E900`) are reported only.
//! - **A tool the scan cannot take** (one that breaks the shape rules, such
//!   as a description that is not text; one too large to scan alone; one the
//!   endpoint refuses with a 413 or 422; one it does not reach twice; the
//!   tools past 16 requests) is withheld in every fail mode. Relaying it would
//!   let a server skip the scan by sending what the scan cannot read. A list
//!   over the endpoint's limits (500 tools, 131,072 characters) is split into
//!   several requests first; the endpoint is stateless, so each is scanned
//!   alone.
//! - **No verdict** (the API unreachable, a 5xx, the plan limit) is the fail
//!   mode's call: `closed` refuses the list; `mixed` and `open` relay the
//!   tools not yet scanned, logged as unchecked. `mixed` blocks destructive
//!   *calls*, and a call to any tool is still checked on its own.
//! - **An answer that is not a verdict** (a rejected key, another 4xx, a
//!   malformed body) refuses the list in every mode.

use super::check::{classify_answer, ApiFailure, FailMode, CHECK_TIMEOUT};
use super::pins::{sha256_hex, Pin, ToolHash};
use crate::api::PromptGuardClient;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
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

/// Scan requests one tool list may cost. Each counts against the plan, and a
/// list needing more (well over 8,000 tools) is not one a real server sends;
/// the tools past them are withheld.
const MAX_SCANS: usize = 16;

/// The verdict on one `tools/list` result.
#[derive(Debug, Default)]
pub struct Review {
    /// Refuse the list with this reason; the rest of the review is moot.
    pub refuse: Option<String>,
    /// Positions in the list of the tools to remove before relaying it.
    pub withhold: BTreeSet<usize>,
    /// Lines for stderr.
    pub log: Vec<String>,
    /// The pin to store, when there was none.
    pub new_pin: Option<Pin>,
    /// The scan answered, and the list matches its stored pin.
    pub matches_pin: bool,
    /// `new_pin` replaces a stored pin, as `--repin` asked.
    pub repinned: bool,
}

/// Where the scan's context comes from: which server, and its stored pin.
/// The list is the whole list, every page of it.
pub struct Context<'a> {
    pub mode: FailMode,
    pub server: &'a str,
    pub pins_path: &'a str,
    pub stored: Option<&'a Pin>,
    pub now: String,
    /// `--repin`: store this list's pin over the stored one instead of
    /// comparing with it.
    pub repin: bool,
}

/// One scan request: positions in the list, and why it is sent.
struct Job {
    tools: Vec<usize>,
    /// Every scannable tool, in the one request the plan needed: the only
    /// request sent the stored pin, whose `sha256` is the list's pin.
    whole: bool,
    /// A second scan of tools a first one did not reach.
    retry: bool,
}

/// What the scans of one list found.
#[derive(Default)]
struct Scans {
    findings: Vec<Finding>,
    /// Each scanned tool's hash, by position.
    hashes: BTreeMap<usize, ToolHash>,
    /// The endpoint's pin for the whole list, when one request scanned it.
    whole_sha256: Option<String>,
    /// Why scanning stopped early with no verdict (an outage, the plan
    /// limit), and how many tools it left unscanned.
    outage: Option<(ApiFailure, usize)>,
}

/// Review one tool list. Nothing the scan cannot take is relayed: a tool that
/// breaks the shape rules, one too large to scan alone, one the endpoint
/// refuses (413/422), one it never reaches and the tools past
/// [`MAX_SCANS`] requests are withheld in every fail mode, since relaying them
/// would let a server skip the poisoning scan by sending what it cannot read.
/// A list over the endpoint's limits is split into several requests (the
/// endpoint is stateless, so each is scanned alone). Only an answer that never
/// came (an outage, the plan limit) is left to the fail mode.
pub fn review(ctx: &Context, tools: &[Value], api: &dyn ManifestScanner) -> Review {
    let mut review = Review::default();
    let mut scannable = Vec::new();
    for (i, tool) in tools.iter().enumerate() {
        match canonical_chars(tool) {
            Err(why) => {
                review.withhold.insert(i);
                review.log.push(format!(
                    "withheld {} from the tool list of `{}` in every fail mode: it breaks the \
                     tool-list shape rules ({why}), so it cannot be scanned",
                    describe(tool, i),
                    ctx.server
                ));
            },
            Ok(chars) if chars > MAX_MANIFEST_CHARS => {
                review.withhold.insert(i);
                review.log.push(format!(
                    "withheld {} from the tool list of `{}` in every fail mode: it is about \
                     {chars} characters, and the scan takes {MAX_MANIFEST_CHARS}",
                    describe(tool, i),
                    ctx.server
                ));
            },
            Ok(chars) => scannable.push((i, chars)),
        }
    }

    let plan = plan(&scannable);
    let stored = ctx.stored.filter(|_| !ctx.repin);
    let pin = stored
        .filter(|_| plan.len() == 1)
        .map(|p| p.sha256.as_str());
    let scans = match scan_all(ctx, tools, plan, pin, api, &mut review) {
        Ok(scans) => scans,
        Err(reason) => {
            review.refuse = Some(reason);
            return review;
        },
    };

    if let Some((e, unscanned)) = &scans.outage {
        if ctx.mode == FailMode::Closed {
            review.refuse = Some(format!(
                "the tool list could not be checked and --fail-mode is closed ({})",
                e.message()
            ));
            return review;
        }
        review.log.push(format!(
            "unchecked: relayed {unscanned} tools of `{}` without a PromptGuard scan \
             (fail mode {}): {}",
            ctx.server,
            ctx.mode.as_str(),
            e.message()
        ));
    }

    let positions = |name: &str| -> Vec<usize> {
        tools
            .iter()
            .enumerate()
            .filter(|(_, t)| t.get("name").and_then(Value::as_str) == Some(name))
            .map(|(i, _)| i)
            .collect()
    };
    let current: Vec<ToolHash> = scans.hashes.values().cloned().collect();
    let mut rug_pull = false;
    for finding in &scans.findings {
        if finding.code == "E301" {
            rug_pull = true;
            continue;
        }
        let named = finding.tool != "*";
        let withheld = named && finding.severity == "high";
        if withheld {
            review.withhold.extend(positions(&finding.tool));
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
        if withheld {
            line.push_str("; the tool was withheld from the agent");
        }
        review.log.push(line);
    }

    // The list's pin: the endpoint's, when one request scanned the whole
    // list (it compared the stored pin too); otherwise one made here from
    // the per-tool hashes, compared here. The same list always takes the
    // same path, so a pin is only ever compared with one made the same way.
    if scans.outage.is_some() {
        return review;
    }
    let sha256 = if let Some(sha) = scans.whole_sha256 {
        sha
    } else {
        let sha = list_sha256(&current);
        rug_pull |= stored.is_some_and(|p| p.sha256 != sha);
        sha
    };
    if let (true, Some(old)) = (rug_pull, stored) {
        rug_pull_verdict(ctx, old, &current, tools, &mut review);
        if review.refuse.is_some() {
            return review;
        }
    }
    if stored.is_some() {
        review.matches_pin = !rug_pull;
    } else {
        review.repinned = ctx.repin && ctx.stored.is_some();
        review.new_pin = Some(Pin {
            server: ctx.server.to_string(),
            sha256,
            tools: current,
            pinned_at: ctx.now.clone(),
        });
    }
    review
}

/// Report a rug-pull, and under `closed` withhold every tool it changed or
/// added (by the per-tool hashes), so a tool the developer never approved
/// does not reach the agent. Under `open` and `mixed` it is reported only: a
/// server upgrade changes its tools too, and withholding them would break the
/// agent on each one. The stored pin is kept either way, so every run reports
/// it until the developer accepts the change.
fn rug_pull_verdict(
    ctx: &Context,
    stored: &Pin,
    current: &[ToolHash],
    tools: &[Value],
    review: &mut Review,
) {
    let moved = moved(&stored.tools, current);
    let mut line = format!(
        "tool list finding E301 (high) on `*`: the tool list of `{}` changed since it was \
         pinned (possible rug-pull){}",
        ctx.server,
        changes(&stored.tools, current)
    );
    if ctx.mode == FailMode::Closed {
        if moved.is_empty() {
            review.refuse = Some(format!(
                "the tool list of `{}` changed since it was pinned and the changed tools \
                 cannot be named, so under --fail-mode closed it is refused",
                ctx.server
            ));
        } else {
            for (i, tool) in tools.iter().enumerate() {
                if tool
                    .get("name")
                    .and_then(Value::as_str)
                    .is_some_and(|n| moved.contains(n))
                {
                    review.withhold.insert(i);
                }
            }
            line.push_str(
                ". As --fail-mode is closed, the changed and added tools were withheld from \
                 the agent",
            );
        }
    } else {
        line.push_str(". The list is relayed");
    }
    let _ = write!(
        line,
        ", and the old pin is kept, so every run reports this. Once you have checked the \
         change, accept it by running this server once with `promptguard mcp-proxy --repin \
         -- <its command>`, or by deleting the entry whose \"server\" is \"{}\" from {} \
         (deleting the file re-pins every server)",
        ctx.server, ctx.pins_path
    );
    review.log.push(line);
}

/// The names of the tools in `current` that are new or changed since `pinned`.
fn moved(pinned: &[ToolHash], current: &[ToolHash]) -> BTreeSet<String> {
    current
        .iter()
        .filter(|t| {
            !pinned
                .iter()
                .any(|p| p.name == t.name && p.sha256 == t.sha256)
        })
        .map(|t| t.name.clone())
        .collect()
}

/// Send every scan the list needs. `Err` is a reason to refuse the list in
/// every mode: an answer that is not a verdict.
fn scan_all(
    ctx: &Context,
    tools: &[Value],
    plan: Vec<Vec<usize>>,
    pin: Option<&str>,
    api: &dyn ManifestScanner,
    review: &mut Review,
) -> Result<Scans, String> {
    let whole = plan.len() == 1;
    let mut queue: VecDeque<Job> = plan
        .into_iter()
        .map(|tools| Job {
            tools,
            whole,
            retry: false,
        })
        .collect();
    let mut scans = Scans::default();
    let mut requests = 0;
    while let Some(job) = queue.pop_front() {
        if requests == MAX_SCANS {
            let left: Vec<usize> = job
                .tools
                .iter()
                .chain(queue.iter().flat_map(|j| j.tools.iter()))
                .copied()
                .collect();
            review.log.push(format!(
                "withheld {} tools from the tool list of `{}` in every fail mode: scanning \
                 the list takes more than {MAX_SCANS} requests",
                left.len(),
                ctx.server
            ));
            review.withhold.extend(left);
            break;
        }
        requests += 1;
        let batch: Vec<Value> = job.tools.iter().map(|&i| tools[i].clone()).collect();
        match api.scan(&batch, pin.filter(|_| job.whole)) {
            Ok(scan) => {
                if scan.tools.len() != batch.len() {
                    return Err(
                        "the API's answer was malformed (its tools do not match the \
                                list sent)"
                            .to_string(),
                    );
                }
                if job.whole {
                    scans.whole_sha256 = Some(scan.sha256.clone());
                }
                if !job.retry {
                    scans
                        .hashes
                        .extend(job.tools.iter().copied().zip(scan.tools));
                }
                let unscanned: Vec<usize> = job
                    .tools
                    .iter()
                    .copied()
                    .filter(|&i| {
                        tools[i]
                            .get("name")
                            .and_then(Value::as_str)
                            .is_some_and(|n| scan.unscanned_tools.iter().any(|u| u == n))
                    })
                    .collect();
                scans.findings.extend(scan.findings);
                if unscanned.is_empty() {
                    continue;
                }
                if job.retry {
                    let names: Vec<String> =
                        unscanned.iter().map(|&i| describe(&tools[i], i)).collect();
                    review.log.push(format!(
                        "withheld tools the scan did not reach, twice, in every fail mode: {}",
                        names.join(", ")
                    ));
                    review.withhold.extend(unscanned);
                } else {
                    queue.push_back(Job {
                        tools: unscanned,
                        whole: false,
                        retry: true,
                    });
                }
            },
            Err(ApiFailure::Unusable(reason)) => return Err(reason),
            Err(ApiFailure::ContentRefused(_)) if job.tools.len() > 1 => {
                let (a, b) = job.tools.split_at(job.tools.len() / 2);
                for half in [b, a] {
                    queue.push_front(Job {
                        tools: half.to_vec(),
                        whole: false,
                        retry: job.retry,
                    });
                }
            },
            Err(ApiFailure::ContentRefused(why)) => {
                let i = job.tools[0];
                review.log.push(format!(
                    "withheld {} from the tool list of `{}` in every fail mode: the scan \
                     refused it ({why})",
                    describe(&tools[i], i),
                    ctx.server
                ));
                review.withhold.insert(i);
            },
            Err(e) => {
                let left = job.tools.len() + queue.iter().map(|j| j.tools.len()).sum::<usize>();
                scans.outage = Some((e, left));
                break;
            },
        }
    }
    Ok(scans)
}

/// Group consecutive scannable tools into requests within the endpoint's
/// limits.
fn plan(scannable: &[(usize, usize)]) -> Vec<Vec<usize>> {
    let mut plan: Vec<Vec<usize>> = Vec::new();
    let mut chars = 0;
    for &(i, size) in scannable {
        match plan.last_mut() {
            Some(last) if last.len() < MAX_TOOLS && chars + size <= MAX_MANIFEST_CHARS => {
                last.push(i);
                chars += size;
            },
            _ => {
                plan.push(vec![i]);
                chars = size;
            },
        }
    }
    plan
}

/// A tool as a log names it: its name, or its position when it has none.
fn describe(tool: &Value, index: usize) -> String {
    match tool.get("name").and_then(Value::as_str) {
        Some(name) if !name.is_empty() => {
            format!("`{}`", name.chars().take(64).collect::<String>())
        },
        _ => format!("the tool at position {index}"),
    }
}

/// A pin for a list scanned in several requests: the SHA-256 of its tools'
/// own hashes, sorted. 64 lowercase hex digits, as the endpoint's are.
fn list_sha256(tools: &[ToolHash]) -> String {
    let mut pairs: Vec<(&str, &str)> = tools
        .iter()
        .map(|t| (t.name.as_str(), t.sha256.as_str()))
        .collect();
    pairs.sort_unstable();
    sha256_hex(&serde_json::to_vec(&pairs).unwrap_or_default())
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

/// The characters of a tool's canonical JSON as the endpoint counts them,
/// or why the endpoint would refuse the tool's shape with a 422. The count is
/// the endpoint's canonical form re-created closely (every non-ASCII
/// character escaped), not exactly; a request near the limit that the
/// endpoint still refuses comes back as a 422, and is split.
fn canonical_chars(tool: &Value) -> Result<usize, String> {
    let name = match tool.get("name") {
        Some(Value::String(n)) if !n.is_empty() && n.chars().count() <= MAX_NAME_CHARS => n,
        Some(Value::String(_)) => {
            return Err(format!(
                "its name is empty or over {MAX_NAME_CHARS} characters"
            ))
        },
        _ => return Err("its name is not text".to_string()),
    };
    let description = match tool.get("description") {
        None | Some(Value::Null) => "",
        Some(Value::String(d)) => d,
        Some(_) => return Err("its description is not text".to_string()),
    };
    let schema = match tool.get("inputSchema").or_else(|| tool.get("input_schema")) {
        None | Some(Value::Null) => json!({}),
        Some(s @ Value::Object(_)) => s.clone(),
        Some(_) => return Err("its inputSchema is not an object".to_string()),
    };
    let canonical =
        json!({"description": description, "input_schema": schema, "name": name}).to_string();
    Ok(canonical
        .chars()
        .map(|c| match c.len_utf16() {
            _ if c.is_ascii() => 1,
            1 => 6,
            _ => 12,
        })
        .sum())
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
    fn tools_the_endpoint_would_refuse_break_the_shape_rules() {
        let ok = json!({"name": "t", "description": "d", "inputSchema": {"type": "object"}});
        assert!(canonical_chars(&ok).is_ok());
        for bad in [
            json!({"description": "no name"}),
            json!({"name": "x".repeat(257)}),
            json!({"name": "t", "description": 3}),
            json!({"name": "t", "inputSchema": []}),
        ] {
            assert!(canonical_chars(&bad).is_err(), "{bad}");
        }
        let wide = json!({"name": "t", "description": "é".repeat(MAX_MANIFEST_CHARS / 6 + 1)});
        assert!(canonical_chars(&wide).unwrap() > MAX_MANIFEST_CHARS);
    }

    #[test]
    fn a_plan_keeps_each_request_within_the_limits() {
        let sizes: Vec<(usize, usize)> = (0..1200).map(|i| (i, 100)).collect();
        let lens: Vec<usize> = plan(&sizes).iter().map(Vec::len).collect();
        assert_eq!(lens, [500, 500, 200]);
        let sizes = [(0, 100_000), (1, 40_000), (2, 30_000)];
        assert_eq!(plan(&sizes), [vec![0], vec![1, 2]]);
    }
}
