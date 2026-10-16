//! The seam for inspecting what the server sends back.
//!
//! acebot712/promptguard#954 plugs in here: it redacts PII and secrets in
//! `tools/call` results through the guard API, and sends each `tools/list`
//! result to the manifest-scan endpoint with the previous pin hash. Both need
//! to know which request a response answers, which the proxy already tracks
//! for its pending cap, so the inspector receives that alongside the message.
//! Pins are keyed by server identity; [`ServerIdentity`] is that key's input.
//!
//! Until #954 lands, [`PassThrough`] relays every response unchanged, byte for
//! byte.

use crate::api::PromptGuardClient;
use serde_json::Value;
use std::sync::Arc;

/// The client request a server response answers.
#[derive(Debug, Clone)]
#[allow(dead_code)] // Read by #954's inspector; PassThrough ignores it.
pub struct AnsweredRequest {
    /// JSON-RPC method, e.g. `tools/list` or `tools/call`.
    pub method: String,
    /// For `tools/call`, the tool's name.
    pub tool: Option<String>,
}

/// The wrapped server, as the user launched it.
#[derive(Debug, Clone)]
#[allow(dead_code)] // Read by #954's inspector; PassThrough ignores it.
pub struct ServerIdentity {
    /// The command and its arguments, as given after `--`.
    pub command: Vec<String>,
}

/// What to relay to the client in place of a server response.
#[derive(Debug)]
#[allow(dead_code)] // `Replace` is for #954's inspector; PassThrough never builds it.
pub enum Relay {
    /// The server's bytes, unchanged.
    Unchanged,
    /// A rewritten message, serialized on one line.
    Replace(Value),
}

/// Inspects each server response that answers a tracked client request.
///
/// Called on the thread that reads the server, once per response, in the
/// order the server sent them. A response the proxy did not buffer (one over
/// the 4 MiB line limit) is streamed through without reaching the inspector;
/// #954 must decide whether that is acceptable for results it is meant to
/// redact.
pub trait ResponseInspector: Send + Sync {
    fn inspect(&self, request: &AnsweredRequest, response: &Value) -> Relay;
}

/// Relays every response unchanged.
pub struct PassThrough;

impl ResponseInspector for PassThrough {
    fn inspect(&self, _: &AnsweredRequest, _: &Value) -> Relay {
        Relay::Unchanged
    }
}

/// The inspector the proxy runs with. #954 replaces the body, calling the
/// guard and manifest-scan endpoints through `client`; nothing else in the
/// proxy needs to change.
pub fn response_inspector(
    _server: &ServerIdentity,
    _client: &Arc<PromptGuardClient>,
) -> Box<dyn ResponseInspector> {
    Box::new(PassThrough)
}
