//! `promptguard mcp-proxy -- <server command>`: wrap a stdio MCP server and
//! check every tool call before it runs.
//!
//! The proxy spawns the server and relays newline-delimited JSON-RPC both
//! ways. Each `tools/call` from the client is decided first by the local
//! destructive-action check and then by the API's validate-tool endpoint
//! (see [`check::decide`]); a refusal is answered with JSON-RPC `-32600` and
//! never reaches the server. Client messages are relayed as the proxy parsed
//! them (re-serialized), never as the client's bytes, and a client line that
//! is not JSON is dropped: see [`Proxy::handle_client_line`]. The server's
//! output is relayed byte for byte. The server's stderr is inherited, so it
//! reaches the client untouched; the proxy's own log lines go there too,
//! prefixed `promptguard mcp-proxy:`. Nothing but relayed messages is ever
//! written to stdout.
//!
//! Threads: the main thread reads the server; one thread reads the client;
//! each `tools/call` is checked on its own short-lived thread so a slow API
//! does not hold up pings, cancellations or other requests. The pending cap
//! bounds how many of those exist at once.

mod check;
mod destructive;
mod inspect;
mod line;

pub use check::FailMode;

use crate::api::PromptGuardClient;
use crate::auth::resolve_session;
use crate::error::{PromptGuardError, Result};
use check::{ApiValidator, Decision, Validator};
use inspect::{AnsweredRequest, Relay, ResponseInspector, ServerIdentity};
use line::{Line, MAX_LINE_BYTES};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{self, BufReader, Write};
use std::path::Path;
use std::process::{ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Requests the client may have in flight at once. A request past this is
/// refused rather than queued: the proxy keeps state for each one.
const MAX_PENDING: usize = 1024;

/// A pending request older than this is forgotten. Covers servers that never
/// answer, and answers too large for the proxy to read the id from.
const PENDING_TTL: Duration = Duration::from_mins(10);

/// JSON-RPC "Invalid Request", the code every refusal carries.
const INVALID_REQUEST: i64 = -32600;

/// The proxy's credential must not reach a third-party server.
const SCRUBBED_ENV: &[&str] = &["PROMPTGUARD_API_KEY"];

/// Package runners whose first non-flag argument names the server, so the
/// default agent id says `npx @modelcontextprotocol/server-filesystem`
/// rather than just `npx`.
const PACKAGE_RUNNERS: &[&str] = &["npx", "pnpx", "bunx", "uvx"];

pub struct McpProxyCommand {
    pub fail_mode: FailMode,
    pub agent_id: Option<String>,
    /// The server command and its arguments.
    pub command: Vec<String>,
}

impl McpProxyCommand {
    /// Run until the server exits, and return its exit code.
    pub fn execute(&self) -> Result<i32> {
        let Some((program, args)) = self.command.split_first() else {
            return Err(PromptGuardError::Custom(
                "mcp-proxy needs the MCP server's command after `--`, e.g. \
                 promptguard mcp-proxy -- npx -y @modelcontextprotocol/server-filesystem ."
                    .to_string(),
            ));
        };

        let (api_key, base_url) = resolve_session()?;
        let client = Arc::new(PromptGuardClient::new(api_key, Some(base_url))?);
        let server = ServerIdentity {
            command: self.command.clone(),
        };
        let inspector = inspect::response_inspector(&server, &client);
        let validator = ApiValidator {
            client,
            agent_id: self
                .agent_id
                .clone()
                .unwrap_or_else(|| default_agent_id(&self.command)),
            session_id: session_id(),
        };

        let mut command = Command::new(program);
        command
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        for var in SCRUBBED_ENV {
            command.env_remove(var);
        }
        let mut child = command.spawn().map_err(|e| {
            PromptGuardError::Custom(format!("Could not start the MCP server `{program}`: {e}"))
        })?;
        let (Some(to_server), Some(from_server)) = (child.stdin.take(), child.stdout.take()) else {
            let _ = child.kill();
            return Err(PromptGuardError::Custom(
                "Could not connect to the MCP server's stdin and stdout".to_string(),
            ));
        };

        log(&format!(
            "checking tool calls for `{}` (fail mode: {})",
            self.command.join(" "),
            self.fail_mode.as_str()
        ));

        let proxy = Arc::new(Proxy {
            fail_mode: self.fail_mode,
            validator: Box::new(validator),
            inspector,
            pending: Mutex::new(HashMap::new()),
            to_server: Mutex::new(Some(to_server)),
        });

        let client_side = Arc::clone(&proxy);
        thread::Builder::new()
            .name("mcp-proxy-client".to_string())
            .spawn(move || client_side.relay_client_to_server())
            .map_err(PromptGuardError::Io)?;

        proxy.relay_server_to_client(from_server);
        let status = child.wait()?;
        Ok(status.code().unwrap_or(1))
    }
}

/// `mcp-proxy:<server>`, from the command's file name and, for a package
/// runner, the package it runs. Never the full command line: arguments can
/// carry tokens, and the agent id is sent to the API.
fn default_agent_id(command: &[String]) -> String {
    let name = |s: &str| {
        Path::new(s)
            .file_stem()
            .map_or_else(|| s.to_string(), |n| n.to_string_lossy().into_owned())
    };
    let Some(program) = command.first() else {
        return "mcp-proxy".to_string();
    };
    let program = name(program);
    let package = PACKAGE_RUNNERS
        .contains(&program.as_str())
        .then(|| command[1..].iter().find(|a| !a.starts_with('-')))
        .flatten();
    match package {
        Some(p) => format!("mcp-proxy:{program} {p}"),
        None => format!("mcp-proxy:{program}"),
    }
}

/// One id per proxy run, so the API can group a session's calls.
fn session_id() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    format!("mcp-proxy-{}-{millis}", std::process::id())
}

fn log(message: &str) {
    eprintln!("promptguard mcp-proxy: {message}");
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A non-null JSON-RPC id, if the message has one.
fn id_of(message: &Value) -> Option<&Value> {
    message.get("id").filter(|id| !id.is_null())
}

fn method_of(message: &Value) -> Option<&str> {
    message.get("method").and_then(Value::as_str)
}

fn refusal(id: &Value, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {"code": INVALID_REQUEST, "message": message},
    })
}

/// `message` serialized on one line, exactly as the proxy read it. `serde_json`
/// leaves U+2028, U+2029 and U+0085 raw inside strings, and some readers
/// split lines on them, so they are escaped; they cannot occur outside a
/// string in `serde_json`'s output, so the escape is always valid JSON.
fn canonical(message: &Value) -> Vec<u8> {
    let mut out = message.to_string();
    for (raw, escaped) in [
        ('\u{2028}', "\\u2028"),
        ('\u{2029}', "\\u2029"),
        ('\u{85}', "\\u0085"),
    ] {
        if out.contains(raw) {
            out = out.replace(raw, escaped);
        }
    }
    out.push('\n');
    out.into_bytes()
}

/// The tool name and arguments of a `tools/call`, or why it is malformed.
/// Arguments must be an object (absent means `{}`), as MCP requires.
fn parse_tool_call(message: &Value) -> std::result::Result<(String, Value), String> {
    let params = message.get("params");
    let name = params
        .and_then(|p| p.get("name"))
        .and_then(Value::as_str)
        .ok_or("params.name must be a string")?;
    let arguments = match params.and_then(|p| p.get("arguments")) {
        None | Some(Value::Null) => json!({}),
        Some(v @ Value::Object(_)) => v.clone(),
        Some(_) => return Err("params.arguments must be an object".to_string()),
    };
    Ok((name.to_string(), arguments))
}

struct Proxy {
    fail_mode: FailMode,
    validator: Box<dyn Validator>,
    inspector: Box<dyn ResponseInspector>,
    /// Client requests awaiting a server response, by serialized id.
    pending: Mutex<HashMap<String, (AnsweredRequest, Instant)>>,
    /// `None` once the client has gone and the server's stdin is closed.
    to_server: Mutex<Option<ChildStdin>>,
}

impl Proxy {
    // -- client -> server ---------------------------------------------------

    fn relay_client_to_server(self: &Arc<Self>) {
        let stdin = io::stdin();
        let mut reader = stdin.lock();
        loop {
            match line::read_line(&mut reader, MAX_LINE_BYTES) {
                Ok(None) => break,
                Ok(Some(Line::Complete(raw))) => self.handle_client_line(&raw),
                Ok(Some(Line::Oversized(_))) => {
                    // Too large to inspect, so relaying it would let a padded
                    // tool call through unchecked. Its id is unreadable, so
                    // there is nothing to answer; the client times it out.
                    log("dropped a client message over 4 MiB: too large to check, so it was not relayed");
                    if line::finish_line(&mut reader, None).is_err() {
                        break;
                    }
                },
                Err(e) => {
                    log(&format!("stopped reading the client: {e}"));
                    break;
                },
            }
        }
        // The client has gone: close the server's stdin so it can exit.
        lock(&self.to_server).take();
    }

    /// A client line is relayed only as the proxy read it: parsed, then
    /// re-serialized. A client never legitimately sends anything else, and
    /// the server's parser may read the original bytes differently (deeper
    /// nesting, NaN, 1e999, lone surrogates, duplicate keys, or a bare `\r`
    /// that a universal-newline reader treats as a line break), which would
    /// let a `tools/call` past the check. So a line `serde_json` cannot parse,
    /// or one with a bare `\r`, is dropped and logged.
    fn handle_client_line(self: &Arc<Self>, raw: &[u8]) {
        let body = raw.strip_suffix(b"\n").unwrap_or(raw);
        let body = body.strip_suffix(b"\r").unwrap_or(body);
        if body.contains(&b'\r') {
            log("dropped a client line with a bare carriage return: a server may split it into several messages, so it was not relayed");
            return;
        }
        match serde_json::from_slice::<Value>(body) {
            Ok(message @ Value::Object(_)) => self.handle_client_message(&message),
            Ok(Value::Array(items)) => self.handle_client_batch(&items),
            Ok(_) | Err(_) => log(
                "dropped a client line that is not JSON-RPC: the server could parse it differently, so it was not relayed",
            ),
        }
    }

    fn handle_client_message(self: &Arc<Self>, message: &Value) {
        let raw = canonical(message);
        let Some(method) = method_of(message).map(str::to_string) else {
            // A response to a request the server made.
            self.write_server(&raw);
            return;
        };
        let id = id_of(message).cloned();

        if method != "tools/call" {
            if let Some(id) = &id {
                if let Err(why) = self.track(id, &method, None) {
                    self.write_client_value(&refusal(id, why));
                    return;
                }
            }
            self.write_server(&raw);
            return;
        }

        let (tool, arguments) = match parse_tool_call(message) {
            Ok(call) => call,
            Err(why) => {
                log(&format!("refused a malformed tools/call: {why}"));
                if let Some(id) = &id {
                    self.write_client_value(&refusal(id, &format!("Invalid tools/call: {why}")));
                }
                return;
            },
        };
        if let Some(id) = &id {
            if let Err(why) = self.track(id, &method, Some(tool.clone())) {
                self.write_client_value(&refusal(id, why));
                return;
            }
        }

        let proxy = Arc::clone(self);
        let check = thread::Builder::new()
            .name("mcp-proxy-check".to_string())
            .spawn(move || proxy.check_and_forward(&raw, id.as_ref(), &tool, &arguments));
        if let Err(e) = check {
            log(&format!("could not start a check: {e}"));
        }
    }

    fn check_and_forward(&self, raw: &[u8], id: Option<&Value>, tool: &str, arguments: &Value) {
        match check::decide(self.fail_mode, tool, arguments, self.validator.as_ref()) {
            Decision::Relay => self.write_server(raw),
            Decision::RelayUnchecked { why } => {
                log(&format!(
                    "unchecked: relayed tools/call `{tool}` without a PromptGuard verdict \
                     (fail mode {}): {why}",
                    self.fail_mode.as_str()
                ));
                self.write_server(raw);
            },
            Decision::Refuse { reason } => {
                log(&format!("refused tools/call `{tool}`: {reason}"));
                if let Some(id) = id {
                    self.untrack(id);
                    self.write_client_value(&refusal(
                        id,
                        &format!("PromptGuard refused tool call `{tool}`: {reason}"),
                    ));
                }
            },
        }
    }

    /// A batch is relayed whole or refused whole. One holding a `tools/call`
    /// is refused: checking its calls one by one would mean splitting the
    /// batch, and forwarding it unchecked would be a way around the proxy.
    /// (MCP dropped batching in its 2025-06-18 revision.)
    fn handle_client_batch(&self, items: &[Value]) {
        let requests: Vec<(&Value, &str)> = items
            .iter()
            .filter_map(|m| Some((id_of(m)?, method_of(m)?)))
            .collect();
        let refuse_all = |why: &str| {
            log(&format!("refused a batch: {why}"));
            let replies: Vec<Value> = requests.iter().map(|(id, _)| refusal(id, why)).collect();
            if !replies.is_empty() {
                self.write_client_value(&Value::Array(replies));
            }
        };

        if items.iter().any(|m| method_of(m) == Some("tools/call")) {
            refuse_all(
                "a batch containing tools/call cannot be checked; send tool calls one at a time",
            );
            return;
        }
        {
            let mut pending = lock(&self.pending);
            Self::expire(&mut pending);
            if pending.len() + requests.len() > MAX_PENDING {
                drop(pending);
                refuse_all("too many pending requests (limit 1024)");
                return;
            }
            for (id, method) in &requests {
                pending.insert(
                    id.to_string(),
                    (
                        AnsweredRequest {
                            method: (*method).to_string(),
                            tool: None,
                        },
                        Instant::now(),
                    ),
                );
            }
        }
        self.write_server(&canonical(&Value::Array(items.to_vec())));
    }

    fn expire(pending: &mut HashMap<String, (AnsweredRequest, Instant)>) {
        pending.retain(|_, (_, since)| since.elapsed() < PENDING_TTL);
    }

    fn track(
        &self,
        id: &Value,
        method: &str,
        tool: Option<String>,
    ) -> std::result::Result<(), &'static str> {
        let mut pending = lock(&self.pending);
        Self::expire(&mut pending);
        let key = id.to_string();
        if pending.contains_key(&key) {
            return Err("a request with this id is already pending");
        }
        if pending.len() >= MAX_PENDING {
            return Err("too many pending requests (limit 1024)");
        }
        pending.insert(
            key,
            (
                AnsweredRequest {
                    method: method.to_string(),
                    tool,
                },
                Instant::now(),
            ),
        );
        Ok(())
    }

    fn untrack(&self, id: &Value) -> Option<AnsweredRequest> {
        lock(&self.pending).remove(&id.to_string()).map(|(r, _)| r)
    }

    fn write_server(&self, raw: &[u8]) {
        let mut to_server = lock(&self.to_server);
        if let Some(stdin) = to_server.as_mut() {
            let ok = stdin.write_all(raw).is_ok()
                && (raw.ends_with(b"\n") || stdin.write_all(b"\n").is_ok())
                && stdin.flush().is_ok();
            if !ok {
                // The server has gone; its stdout reaching EOF ends the proxy.
                to_server.take();
            }
        }
    }

    // -- server -> client ---------------------------------------------------

    fn relay_server_to_client(&self, from_server: ChildStdout) {
        let mut reader = BufReader::new(from_server);
        loop {
            match line::read_line(&mut reader, MAX_LINE_BYTES) {
                Ok(None) => break,
                Ok(Some(Line::Complete(raw))) => self.handle_server_line(&raw),
                Ok(Some(Line::Oversized(prefix))) => {
                    // Streamed through unread, so memory stays bounded and a
                    // large tool result still arrives. Nothing in #943 inspects
                    // this direction; see inspect.rs for #954.
                    log("relayed a server message over 4 MiB without inspecting it");
                    let stdout = io::stdout();
                    let mut out = stdout.lock();
                    let streamed = out.write_all(&prefix).is_ok()
                        && line::finish_line(&mut reader, Some(&mut out)).is_ok();
                    let _ = out.flush();
                    if !streamed {
                        break;
                    }
                },
                Err(e) => {
                    log(&format!("stopped reading the server: {e}"));
                    break;
                },
            }
        }
    }

    fn handle_server_line(&self, raw: &[u8]) {
        let replacement = match serde_json::from_slice::<Value>(raw) {
            Ok(message @ Value::Object(_)) => match self.inspect(&message) {
                Relay::Unchanged => None,
                Relay::Replace(v) => Some(v),
            },
            Ok(Value::Array(items)) => {
                let mut replaced = false;
                let out: Vec<Value> = items
                    .into_iter()
                    .map(|m| match self.inspect(&m) {
                        Relay::Unchanged => m,
                        Relay::Replace(v) => {
                            replaced = true;
                            v
                        },
                    })
                    .collect();
                replaced.then_some(Value::Array(out))
            },
            _ => None,
        };
        match replacement {
            Some(v) => self.write_client_value(&v),
            None => self.write_client(raw),
        }
    }

    /// Pass a response to the inspector with the request it answers. Server
    /// requests and notifications, and responses to ids the proxy is not
    /// tracking, are relayed unchanged.
    fn inspect(&self, message: &Value) -> Relay {
        if method_of(message).is_some() {
            return Relay::Unchanged;
        }
        match id_of(message).and_then(|id| self.untrack(id)) {
            Some(request) => self.inspector.inspect(&request, message),
            None => Relay::Unchanged,
        }
    }

    fn write_client(&self, raw: &[u8]) {
        let stdout = io::stdout();
        let mut out = stdout.lock();
        // A failed write means the client has gone; its stdin closing ends
        // the proxy, so there is nothing more to do here.
        let _ = out.write_all(raw);
        if !raw.ends_with(b"\n") {
            let _ = out.write_all(b"\n");
        }
        let _ = out.flush();
    }

    fn write_client_value(&self, message: &Value) {
        self.write_client(message.to_string().as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn default_agent_id_names_the_server_without_its_arguments() {
        assert_eq!(
            default_agent_id(&cmd(&["/usr/local/bin/my-server", "--token", "secret"])),
            "mcp-proxy:my-server"
        );
        assert_eq!(
            default_agent_id(&cmd(&[
                "npx",
                "-y",
                "@modelcontextprotocol/server-filesystem",
                "."
            ])),
            "mcp-proxy:npx @modelcontextprotocol/server-filesystem"
        );
        assert_eq!(
            default_agent_id(&cmd(&["python3", "server.py"])),
            "mcp-proxy:python3"
        );
    }

    #[test]
    fn tool_call_arguments_must_be_an_object() {
        let call =
            |params: Value| parse_tool_call(&json!({"method": "tools/call", "params": params}));
        assert_eq!(
            call(json!({"name": "t"})).ok(),
            Some(("t".to_string(), json!({})))
        );
        assert!(call(json!({"name": "t", "arguments": {"a": 1}})).is_ok());
        assert!(call(json!({"name": "t", "arguments": "rm -rf /"})).is_err());
        assert!(call(json!({"name": 3})).is_err());
        assert!(call(json!({})).is_err());
    }
}
