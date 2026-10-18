//! Redacting a tool result: the text in a `tools/call` result is sent to the
//! `PromptGuard` API's guard endpoint as output, and what it masks (PII,
//! secrets, as the project's guardrails say) is put back in its place before
//! the result reaches the agent.
//!
//! Only strings are sent and only strings change: each text block's `text`,
//! each embedded resource's `text`, a resource link's `name`, `title` and
//! `description` (not its `uri`), every string value in `structuredContent`,
//! and, for a call answered with a JSON-RPC error, the error's `message` and
//! every string in its `data`. The JSON-RPC envelope, the block order and
//! every other block (an image, audio) are relayed as the server sent them.
//!
//! What happens when the API gives no answer is the fail mode's call, as for a
//! tool call (ADR-0034), with one difference. `mixed` relays an unredacted
//! result as `open` does: its distinction is between destructive and other
//! *actions*, and a result is information about an action that has already
//! run. Withholding it undoes nothing and breaks the agent, while the cost of
//! relaying it, text reaching the agent's model unmasked, is the cost the SDKs
//! accept for prompts when they fail open. `closed` withholds it. A block, or
//! an answer that is not a verdict, withholds it in every mode.

use super::check::{classify_answer, ApiFailure, FailMode, CHECK_TIMEOUT};
use crate::api::PromptGuardClient;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Characters per guard request, separators included. The endpoint takes
/// 100,000 per message and 100,000 in all; the margin covers counting
/// differences between the two sides.
const MAX_REQUEST_CHARS: usize = 90_000;

/// How long one result may take to redact, all its requests together, on
/// top of each request's own [`CHECK_TIMEOUT`]. An agent waits on every tool
/// result; past this, the text not yet redacted is the fail mode's call.
pub const RESULT_DEADLINE: Duration = Duration::from_secs(20);

/// Messages per guard request; the endpoint takes 512.
const MAX_REQUEST_MESSAGES: usize = 512;

/// Guard requests one result may cost. Each is a scan against the plan, so
/// in a result needing more (over about 1.4 million characters of text) only
/// the text these cover is redacted, and the fail mode decides the rest.
const MAX_REQUESTS: usize = 16;

/// The guard endpoint joins messages with U+001E to scan them as one text,
/// and splits the redacted text on it again. One inside a message would shift
/// every later message's redaction onto its neighbour, so it is sent as
/// U+FFFD instead.
const RECORD_SEPARATOR: char = '\u{1e}';

/// The guard endpoint's answer (`GuardResponse` in the backend's
/// `developer/guard/router.py`), the fields the proxy reads.
#[derive(Debug, Deserialize)]
pub struct GuardAnswer {
    pub decision: String,
    #[serde(default)]
    pub threat_type: Option<String>,
    #[serde(default)]
    pub threats: Vec<GuardThreat>,
    #[serde(default)]
    pub redacted_messages: Option<Vec<GuardMessage>>,
}

#[derive(Debug, Deserialize)]
pub struct GuardThreat {
    #[serde(rename = "type", default)]
    pub threat_type: String,
    #[serde(default)]
    pub details: String,
}

#[derive(Debug, Deserialize)]
pub struct GuardMessage {
    pub content: Value,
}

/// Where a tool result's text is sent.
pub trait Redactor: Send + Sync {
    /// The guard endpoint's answer for these texts, one message each, or a
    /// failure if none came within `timeout`.
    fn redact(
        &self,
        tool: &str,
        texts: &[String],
        timeout: Duration,
    ) -> Result<GuardAnswer, ApiFailure>;
}

/// `POST /guard`, through the CLI's own HTTP client.
pub struct ApiRedactor {
    pub client: Arc<PromptGuardClient>,
    pub agent_id: String,
    pub session_id: String,
}

impl Redactor for ApiRedactor {
    fn redact(
        &self,
        tool: &str,
        texts: &[String],
        timeout: Duration,
    ) -> Result<GuardAnswer, ApiFailure> {
        let messages: Vec<Value> = texts
            .iter()
            .map(|t| json!({"role": "tool", "content": t}))
            .collect();
        let body = json!({
            "messages": messages,
            "direction": "output",
            "context": {
                "framework": "mcp-proxy",
                "agent_id": self.agent_id,
                "session_id": self.session_id,
                "metadata": {"tool_name": tool},
            },
        });
        let response = self
            .client
            .send_raw(&reqwest::Method::POST, "/guard", Some(&body), timeout);
        classify_answer(response, "redact the result", true)
    }
}

/// What to relay in place of a tool result.
#[derive(Debug, PartialEq)]
pub enum Outcome {
    /// Nothing to mask: relay the server's message as it is.
    Unchanged,
    /// Relay this result instead.
    Redacted(Value),
    /// Not redacted, and the fail mode lets it through: relay this result
    /// (the server's, with any part that was redacted before the failure
    /// masked), and log why.
    Unredacted { result: Value, why: String },
    /// Answer the agent with an error instead.
    Withhold { reason: String },
}

/// Decide what the agent gets for one `tools/call` result, within
/// `deadline` in all.
pub fn review(
    mode: FailMode,
    tool: &str,
    result: &Value,
    api: &dyn Redactor,
    deadline: Duration,
) -> Outcome {
    review_slots(mode, tool, result, &text_slots(result), api, deadline)
}

/// As [`review`], for the `error` of a `tools/call` answered with one: its
/// `message` and every string in its `data`, where a server often puts the
/// output of the failed call.
pub fn review_error(
    mode: FailMode,
    tool: &str,
    error: &Value,
    api: &dyn Redactor,
    deadline: Duration,
) -> Outcome {
    let mut slots = Vec::new();
    if let Some(message) = error.get("message") {
        strings_in(message, "/message".to_string(), &mut slots);
    }
    if let Some(data) = error.get("data") {
        strings_in(data, "/data".to_string(), &mut slots);
    }
    review_slots(mode, tool, error, &slots, api, deadline)
}

fn review_slots(
    mode: FailMode,
    tool: &str,
    result: &Value,
    slots: &[(String, String)],
    api: &dyn Redactor,
    deadline: Duration,
) -> Outcome {
    let started = Instant::now();
    let pieces: Vec<Piece> = slots
        .iter()
        .enumerate()
        .flat_map(|(slot, (_, text))| {
            split(text, MAX_REQUEST_CHARS)
                .into_iter()
                .map(move |original| Piece {
                    slot,
                    original,
                    redacted: None,
                })
        })
        .collect();
    let mut pieces = pieces;
    let batches = batches(&pieces);

    // A result needing more than MAX_REQUESTS still has its first ones
    // sent, so a block in them is enforced; the text past them is left to
    // the fail mode. Sending nothing would skip the guard's block.
    let over = batches.len() > MAX_REQUESTS;
    let mut failure = None;
    let late = || {
        ApiFailure::Unavailable(format!(
            "the result was not redacted within {}s",
            deadline.as_secs()
        ))
    };
    for batch in batches.into_iter().take(MAX_REQUESTS) {
        let left = deadline.saturating_sub(started.elapsed());
        if left.is_zero() {
            failure = Some(late());
            break;
        }
        let texts: Vec<String> = pieces[batch.clone()]
            .iter()
            .map(|p| p.original.replace(RECORD_SEPARATOR, "\u{fffd}"))
            .collect();
        let answered = api.redact(tool, &texts, left.min(CHECK_TIMEOUT));
        let answered = match answered {
            Err(ApiFailure::Unavailable(_)) if started.elapsed() >= deadline => Err(late()),
            other => other,
        };
        match answer(answered, texts.len()) {
            Ok(Verdict::Allow) => {},
            Ok(Verdict::Redact(redacted)) => {
                for (piece, text) in pieces[batch].iter_mut().zip(redacted) {
                    piece.redacted = Some(text);
                }
            },
            Ok(Verdict::Block(reason)) => return Outcome::Withhold { reason },
            Err(e) => {
                failure = Some(e);
                break;
            },
        }
    }
    if over && failure.is_none() {
        failure = Some(ApiFailure::ContentRefused(format!(
            "the result has more text than {MAX_REQUESTS} redaction requests take; the text \
             past them was not redacted"
        )));
    }

    let rebuilt = rebuild(result, slots, &pieces);
    match failure {
        None => rebuilt.map_or(Outcome::Unchanged, Outcome::Redacted),
        Some(ApiFailure::Unusable(reason)) => Outcome::Withhold { reason },
        Some(e) if mode == FailMode::Closed => Outcome::Withhold {
            reason: format!(
                "the result could not be redacted and --fail-mode is closed ({})",
                e.message()
            ),
        },
        Some(e) => Outcome::Unredacted {
            result: rebuilt.unwrap_or_else(|| result.clone()),
            why: e.message().to_string(),
        },
    }
}

enum Verdict {
    Allow,
    Redact(Vec<String>),
    Block(String),
}

/// Read a guard answer for `sent` messages.
fn answer(answer: Result<GuardAnswer, ApiFailure>, sent: usize) -> Result<Verdict, ApiFailure> {
    let answer = answer?;
    let malformed = |why: &str| {
        Err(ApiFailure::Unusable(format!(
            "the API's answer was malformed ({why})"
        )))
    };
    match answer.decision.as_str() {
        "allow" => Ok(Verdict::Allow),
        "block" => Ok(Verdict::Block(block_reason(&answer))),
        "redact" => {
            let Some(messages) = answer.redacted_messages else {
                return malformed("a redact decision without redacted_messages");
            };
            if messages.len() != sent {
                return malformed("redacted_messages does not match the messages sent");
            }
            let texts: Option<Vec<String>> = messages
                .into_iter()
                .map(|m| m.content.as_str().map(str::to_string))
                .collect();
            match texts {
                Some(texts) => Ok(Verdict::Redact(texts)),
                None => malformed("a redacted message that is not text"),
            }
        },
        other => malformed(&format!("unknown decision `{other}`")),
    }
}

fn block_reason(answer: &GuardAnswer) -> String {
    let what = answer
        .threat_type
        .clone()
        .or_else(|| answer.threats.first().map(|t| t.threat_type.clone()))
        .filter(|t| !t.is_empty());
    let details = answer
        .threats
        .first()
        .map(|t| t.details.clone())
        .filter(|d| !d.is_empty());
    match (what, details) {
        (Some(t), Some(d)) => format!("blocked by PromptGuard ({t}: {d})"),
        (Some(t), None) => format!("blocked by PromptGuard ({t})"),
        (None, Some(d)) => format!("blocked by PromptGuard ({d})"),
        (None, None) => "blocked by PromptGuard".to_string(),
    }
}

/// A part of one slot's text, sent as one guard message.
struct Piece {
    slot: usize,
    original: String,
    redacted: Option<String>,
}

/// Every non-empty string a result's text lives in, as a JSON pointer and
/// the text: text blocks, embedded resources' text, a resource link's name,
/// title and description, and the string values of `structuredContent`
/// (keys are not sent).
fn text_slots(result: &Value) -> Vec<(String, String)> {
    let mut slots = Vec::new();
    if let Some(blocks) = result.get("content").and_then(Value::as_array) {
        for (i, block) in blocks.iter().enumerate() {
            let fields: &[&str] = match block.get("type").and_then(Value::as_str) {
                Some("text") => &["text"],
                Some("resource") => &["resource/text"],
                // A link's own text; its uri is left, or the link would break.
                Some("resource_link") => &["name", "title", "description"],
                _ => &[],
            };
            for field in fields {
                let pointer = format!("/content/{i}/{field}");
                if let Some(Value::String(text)) = result.pointer(&pointer) {
                    if !text.is_empty() {
                        slots.push((pointer, text.clone()));
                    }
                }
            }
        }
    }
    if let Some(structured) = result.get("structuredContent") {
        strings_in(structured, "/structuredContent".to_string(), &mut slots);
    }
    slots
}

fn strings_in(value: &Value, pointer: String, out: &mut Vec<(String, String)>) {
    match value {
        Value::String(s) if !s.is_empty() => out.push((pointer, s.clone())),
        Value::Array(items) => {
            for (i, item) in items.iter().enumerate() {
                strings_in(item, format!("{pointer}/{i}"), out);
            }
        },
        Value::Object(map) => {
            for (key, item) in map {
                let key = key.replace('~', "~0").replace('/', "~1");
                strings_in(item, format!("{pointer}/{key}"), out);
            }
        },
        _ => {},
    }
}

/// Split `text` into pieces of at most `limit` characters, at a line break
/// where there is one, else at whitespace, so a value worth masking is not
/// cut in two. Joined, the pieces are `text`.
fn split(text: &str, limit: usize) -> Vec<String> {
    let mut pieces = Vec::new();
    let mut rest = text;
    while rest.chars().count() > limit {
        let end = rest
            .char_indices()
            .nth(limit)
            .map_or(rest.len(), |(i, _)| i);
        let window = &rest[..end];
        let cut = window
            .rfind('\n')
            .or_else(|| window.rfind(char::is_whitespace))
            .map_or(end, |i| {
                i + window[i..].chars().next().map_or(1, char::len_utf8)
            });
        let cut = if cut == 0 { end } else { cut };
        pieces.push(rest[..cut].to_string());
        rest = &rest[cut..];
    }
    pieces.push(rest.to_string());
    pieces
}

/// Group consecutive pieces into requests within the endpoint's limits.
fn batches(pieces: &[Piece]) -> Vec<std::ops::Range<usize>> {
    let mut batches = Vec::new();
    let (mut start, mut chars) = (0, 0);
    for (i, piece) in pieces.iter().enumerate() {
        let len = piece.original.chars().count();
        let joined = if i == start { len } else { chars + 1 + len };
        if i > start && (joined > MAX_REQUEST_CHARS || i - start == MAX_REQUEST_MESSAGES) {
            batches.push(start..i);
            start = i;
            chars = len;
        } else {
            chars = joined;
        }
    }
    if start < pieces.len() {
        batches.push(start..pieces.len());
    }
    batches
}

/// The result with each redacted piece in place, or `None` when nothing was
/// redacted.
fn rebuild(result: &Value, slots: &[(String, String)], pieces: &[Piece]) -> Option<Value> {
    if pieces.iter().all(|p| p.redacted.is_none()) {
        return None;
    }
    let mut out = result.clone();
    for (slot, (pointer, _)) in slots.iter().enumerate() {
        let parts: Vec<&Piece> = pieces.iter().filter(|p| p.slot == slot).collect();
        if parts.iter().all(|p| p.redacted.is_none()) {
            continue;
        }
        let text: String = parts
            .iter()
            .map(|p| p.redacted.as_deref().unwrap_or(&p.original))
            .collect();
        if let Some(target) = out.pointer_mut(pointer) {
            *target = Value::String(text);
        }
    }
    Some(out)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn split_keeps_every_character_and_prefers_line_breaks() {
        let text = "aaaa\nbbbb cccc";
        let pieces = split(text, 6);
        assert_eq!(pieces.concat(), text);
        assert_eq!(pieces[0], "aaaa\n");
        assert!(pieces.iter().all(|p| p.chars().count() <= 6), "{pieces:?}");

        let unbroken = "é".repeat(13);
        let pieces = split(&unbroken, 5);
        assert_eq!(pieces.concat(), unbroken);
        assert_eq!(pieces.len(), 3);
        assert_eq!(split("", 5), [""]);
    }

    #[test]
    fn slots_are_text_resource_text_and_structured_strings() {
        let result = json!({
            "content": [
                {"type": "text", "text": "one"},
                {"type": "image", "data": "x", "mimeType": "image/png"},
                {"type": "resource", "resource": {"uri": "file:///a", "text": "two"}},
                {"type": "text", "text": ""},
                {"type": "resource_link", "uri": "file:///b", "name": "five", "description": "six"},
            ],
            "structuredContent": {"a/b": ["three", 4, {"c": "four"}]},
            "isError": false,
        });
        let pointers: Vec<String> = text_slots(&result).into_iter().map(|(p, _)| p).collect();
        assert_eq!(
            pointers,
            [
                "/content/0/text",
                "/content/2/resource/text",
                "/content/4/name",
                "/content/4/description",
                "/structuredContent/a~1b/0",
                "/structuredContent/a~1b/2/c",
            ]
        );
        for pointer in pointers {
            assert!(result.pointer(&pointer).is_some(), "{pointer}");
        }
    }

    /// Masks every piece by upper-casing it; records what it was sent.
    struct Upper(Mutex<Vec<Vec<String>>>);

    impl Redactor for Upper {
        fn redact(
            &self,
            _: &str,
            texts: &[String],
            _: Duration,
        ) -> Result<GuardAnswer, ApiFailure> {
            self.0.lock().unwrap().push(texts.to_vec());
            Ok(GuardAnswer {
                decision: "redact".into(),
                threat_type: None,
                threats: vec![],
                redacted_messages: Some(
                    texts
                        .iter()
                        .map(|t| GuardMessage {
                            content: Value::String(t.to_uppercase()),
                        })
                        .collect(),
                ),
            })
        }
    }

    #[test]
    fn a_long_text_is_redacted_in_pieces_and_put_back_whole() {
        let line = "x".repeat(1000) + "\n";
        let text = line.repeat(200); // 200,200 characters
        let result = json!({"content": [{"type": "text", "text": text}]});
        let api = Upper(Mutex::new(vec![]));
        let Outcome::Redacted(out) = review(FailMode::Mixed, "t", &result, &api, RESULT_DEADLINE)
        else {
            panic!("expected a redaction");
        };
        assert_eq!(out["content"][0]["text"], text.to_uppercase());
        let sent = api.0.lock().unwrap();
        assert_eq!(sent.len(), 3, "within the per-request character limit");
        for batch in sent.iter() {
            let chars: usize = batch.iter().map(|t| t.chars().count() + 1).sum();
            assert!(chars <= MAX_REQUEST_CHARS + 1);
        }
    }

    #[test]
    fn a_record_separator_cannot_shift_redactions_between_slots() {
        let result = json!({"content": [
            {"type": "text", "text": "a\u{1e}b"},
            {"type": "text", "text": "c"},
        ]});
        let api = Upper(Mutex::new(vec![]));
        review(FailMode::Mixed, "t", &result, &api, RESULT_DEADLINE);
        let sent = api.0.lock().unwrap();
        assert_eq!(sent[0], ["a\u{fffd}b", "c"]);
    }

    #[test]
    fn a_result_with_no_text_costs_no_request() {
        let api = Upper(Mutex::new(vec![]));
        let result = json!({"content": [{"type": "image", "data": "x", "mimeType": "image/png"}]});
        assert_eq!(
            review(FailMode::Closed, "t", &result, &api, RESULT_DEADLINE),
            Outcome::Unchanged
        );
        assert!(api.0.lock().unwrap().is_empty());
    }

    #[test]
    fn a_wrong_number_of_redacted_messages_is_unusable() {
        struct Short;
        impl Redactor for Short {
            fn redact(
                &self,
                _: &str,
                _: &[String],
                _: Duration,
            ) -> Result<GuardAnswer, ApiFailure> {
                Ok(GuardAnswer {
                    decision: "redact".into(),
                    threat_type: None,
                    threats: vec![],
                    redacted_messages: Some(vec![]),
                })
            }
        }
        let result = json!({"content": [{"type": "text", "text": "jane@example.com"}]});
        for mode in [FailMode::Open, FailMode::Mixed, FailMode::Closed] {
            assert!(matches!(
                review(mode, "t", &result, &Short, RESULT_DEADLINE),
                Outcome::Withhold { .. }
            ));
        }
    }

    /// Answers the first request after `delay`, like `Upper`; every later
    /// one hangs for the whole timeout it is given. Records each timeout.
    ///
    /// Only the first call sleeps a fixed time, and a sleep never ends early,
    /// so what the test asserts does not depend on how late a loaded runner
    /// wakes the thread: the second call always gets at least `delay` less
    /// than the first, and always exhausts the deadline.
    struct Slow {
        delay: Duration,
        timeouts: Mutex<Vec<Duration>>,
    }

    impl Redactor for Slow {
        fn redact(
            &self,
            tool: &str,
            texts: &[String],
            timeout: Duration,
        ) -> Result<GuardAnswer, ApiFailure> {
            let first = {
                let mut timeouts = self.timeouts.lock().unwrap();
                timeouts.push(timeout);
                timeouts.len() == 1
            };
            if !first {
                std::thread::sleep(timeout);
                return Err(ApiFailure::Unavailable("timed out".into()));
            }
            std::thread::sleep(self.delay);
            Upper(Mutex::new(vec![])).redact(tool, texts, timeout)
        }
    }

    #[test]
    fn requests_share_one_deadline_and_the_rest_follows_the_fail_mode() {
        let text = ("x".repeat(1000) + "\n").repeat(300); // 4 requests
        let result = json!({"content": [{"type": "text", "text": text}]});
        // The deadline also covers splitting the text, which on a starved
        // macOS runner took 350ms in a debug build; leave room for that and
        // the first answer.
        let deadline = Duration::from_secs(1);
        let delay = Duration::from_millis(50);
        let api = Slow {
            delay,
            timeouts: Mutex::new(vec![]),
        };
        let outcome = review(FailMode::Mixed, "t", &result, &api, deadline);
        let Outcome::Unredacted { result: out, why } = outcome else {
            panic!("expected the fail mode, got {outcome:?}");
        };
        assert!(why.contains("within"), "{why}");
        let out = out["content"][0]["text"].as_str().unwrap();
        assert!(
            out.starts_with('X') && out.ends_with("x\n"),
            "the first requests are kept"
        );
        let timeouts = api.timeouts.lock().unwrap();
        assert_eq!(
            timeouts.len(),
            2,
            "no request after the deadline: {timeouts:?}"
        );
        assert!(timeouts.iter().all(|t| *t <= deadline));
        assert!(
            timeouts[1] + delay <= timeouts[0],
            "the second gets only what is left: {timeouts:?}"
        );
        drop(timeouts);
        // Every later call hangs, so a short deadline is enough here.
        assert!(matches!(
            review(FailMode::Closed, "t", &result, &api, delay),
            Outcome::Withhold { .. }
        ));
    }
}
