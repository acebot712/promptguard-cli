//! `promptguard mcp-proxy -- <server command>`: wrap a stdio MCP server,
//! check every tool call before it runs, redact every tool result, and scan
//! and pin every tool list.
//!
//! The proxy spawns the server and relays newline-delimited JSON-RPC both
//! ways. Each `tools/call` from the client is decided first by the local
//! destructive-action check and then by the API's validate-tool endpoint
//! (see [`check::decide`]); a refusal is answered with JSON-RPC `-32600` and
//! never reaches the server. Client messages are relayed as the proxy parsed
//! them (re-serialized), never as the client's bytes, and a client line that
//! is not JSON is dropped: see [`Proxy::handle_client_line`].
//!
//! The server's output is relayed byte for byte, except two kinds of
//! response. A `tools/call` result has its text redacted through the guard
//! endpoint ([`redact::review`]); a `tools/list` result is scanned by the
//! manifest-scan endpoint against the pin stored for this server
//! ([`manifest::review`], [`pins`]), and a tool flagged as poisoned is removed
//! from it. Either is re-serialized only when it changed. The server's stderr
//! is inherited, so it reaches the client untouched; the proxy's own log
//! lines go there too, prefixed `promptguard mcp-proxy:`. Nothing but relayed
//! messages is ever written to stdout.
//!
//! Threads: the main thread reads the server; one thread reads the client;
//! each `tools/call` is checked on its own short-lived thread, and each tool
//! result or tool list is inspected on its own, so a slow API does not hold
//! up pings, cancellations or other messages. A request keeps its pending
//! slot until its answer has been written, so the pending cap bounds the
//! check threads. Inspections are bounded on their own, since a server can
//! send results nobody asked for: at most [`MAX_INSPECTIONS`] run at once,
//! and while that many do the main thread stops reading the server, which
//! slows the server down rather than dropping anything. A `tools/call`
//! without an id is refused at once, so it takes neither a slot nor a check.
//!
//! So messages can overtake a `tools/call` that is still being checked. A
//! `notifications/cancelled` for that call may reach the server first; the
//! server ignores a cancellation for an id it does not know, and then runs
//! the call when it arrives. Keeping order would mean holding every message
//! behind every check, which is the stall the threads exist to avoid. For the
//! same reason a server message can overtake a tool result being redacted.

mod check;
mod destructive;
mod line;
mod manifest;
mod pins;
mod redact;

pub use check::FailMode;

use crate::api::{PromptGuardClient, RawResponse};
use crate::auth::{credentials_dir, resolve_session};
use crate::error::{PromptGuardError, Result};
use check::{ApiValidator, Decision, Validator};
use line::{Line, MAX_LINE_BYTES};
use manifest::{ApiManifestScanner, ManifestScanner};
use pins::PinStore;
use redact::{ApiRedactor, Redactor};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::io::{self, BufReader, Write};
use std::path::Path;
use std::process::{ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Requests the client may have in flight at once. A request past this is
/// refused rather than queued: the proxy keeps state for each one.
const MAX_PENDING: usize = 1024;

/// A pending request older than this is forgotten. Covers servers that never
/// answer, and answers too large for the proxy to read the id from.
const PENDING_TTL: Duration = Duration::from_mins(10);

/// Tool results and tool lists inspected at once. Each inspection is a thread
/// waiting on the API; past this the proxy stops reading the server until one
/// finishes, so a server flooding results slows itself down instead of
/// starting a thread apiece, and nothing is dropped.
const MAX_INSPECTIONS: usize = 64;

/// `tools/list` requests the client may have in flight at once. Each can hold
/// an inspection while it waits on the server for later pages, which only
/// the main thread reads, so this keeps most of [`MAX_INSPECTIONS`] free for
/// inspections that finish on their own and the main thread never waits on
/// itself.
const MAX_LIST_REQUESTS: usize = 8;

/// Pages and tools one tool list may have before it is refused: more than
/// any real server lists, and a bound on what the proxy holds.
const MAX_PAGES: usize = 100;
const MAX_LISTED_TOOLS: usize = 10_000;

/// How long the startup key check may take. It delays the server's start.
const STARTUP_CHECK_TIMEOUT: Duration = Duration::from_secs(5);

/// The refusal for a request past [`MAX_PENDING`].
const TOO_MANY_PENDING: &str = "too many pending requests (limit 1024)";

/// The refusal for a `tools/list` past [`MAX_LIST_REQUESTS`].
const TOO_MANY_LISTS: &str = "too many pending tools/list requests (limit 8)";

/// The refusal for a `tools/list` that sends a cursor.
const CURSOR_REFUSED: &str = "mcp-proxy answers tools/list with the whole list in one page and \
                              hands out no cursor, so it does not accept one";

/// JSON-RPC "Invalid Request", the code every refusal carries.
const INVALID_REQUEST: i64 = -32600;

/// The proxy's credential must not reach a third-party server.
const SCRUBBED_ENV: &[&str] = &["PROMPTGUARD_API_KEY"];

/// Package runners whose first non-flag argument names the server, so the
/// default agent id says `npx @modelcontextprotocol/server-filesystem`
/// rather than just `npx`.
const PACKAGE_RUNNERS: &[&str] = &["npx", "pnpx", "bunx", "uvx"];

/// The package runners' flags that take the next argument as their value,
/// so it is not mistaken for the package: `uvx --from <url> <server>`. A
/// value can carry a token (`--from git+https://TOKEN@github.com/...`). An
/// unknown flag is taken to be a switch, like `-y`.
const RUNNER_VALUE_FLAGS: &[&str] = &[
    // npx, pnpx, bunx
    "-p",
    "--package",
    "-c",
    "--call",
    "--registry",
    "--cache",
    "--userconfig",
    "-w",
    "--workspace",
    // uvx
    "--from",
    "--with",
    "--with-editable",
    "--with-requirements",
    "--python",
    "--index",
    "--default-index",
    "--index-url",
    "--extra-index-url",
    "-f",
    "--find-links",
    "--constraints",
    "--overrides",
    "--build-constraints",
    "--index-strategy",
    "--keyring-provider",
    "--resolution",
    "--prerelease",
    "--exclude-newer",
    "--link-mode",
    "--cache-dir",
    "--directory",
    "--project",
    "--config-file",
    "--env-file",
    "--color",
    "--allow-insecure-host",
    "-P",
    "--upgrade-package",
    "--reinstall-package",
    "--refresh-package",
];

pub struct McpProxyCommand {
    pub fail_mode: FailMode,
    pub agent_id: Option<String>,
    /// Accept the server's current tool list: replace its pin on the first
    /// complete scan instead of comparing with it.
    pub repin: bool,
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
        self.verify_key(&client)?;
        let agent_id = self
            .agent_id
            .clone()
            .unwrap_or_else(|| default_agent_id(&self.command));
        let session_id = session_id();
        let validator = ApiValidator {
            client: Arc::clone(&client),
            agent_id: agent_id.clone(),
            session_id: session_id.clone(),
        };
        let redactor = ApiRedactor {
            client: Arc::clone(&client),
            agent_id,
            session_id: session_id.clone(),
        };
        let scanner = ApiManifestScanner { client };
        let pins = match credentials_dir() {
            Ok(dir) => {
                let cwd = std::env::current_dir().and_then(|d| d.canonicalize()).ok();
                Some(PinStore::new(dir, &self.command, cwd.as_deref()))
            },
            Err(e) => {
                log(&format!("tool lists are scanned but not pinned: {e}"));
                None
            },
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

        // The program's name only: arguments can carry tokens.
        let program_name = Path::new(program)
            .file_name()
            .map_or_else(|| program.clone(), |n| n.to_string_lossy().into_owned());
        log(&format!(
            "checking tool calls for `{program_name}` (fail mode: {})",
            self.fail_mode.as_str()
        ));

        let proxy = Arc::new(Proxy {
            fail_mode: self.fail_mode,
            validator: Box::new(validator),
            redactor: Box::new(redactor),
            scanner: Box::new(scanner),
            server: default_agent_id(&self.command),
            pins,
            withheld: Mutex::new(HashSet::new()),
            pending: Mutex::new(HashMap::new()),
            to_server: Mutex::new(Some(to_server)),
            repin: AtomicBool::new(self.repin),
            inspections: Permits::new(MAX_INSPECTIONS),
            internal: Mutex::new(HashMap::new()),
            next_internal: AtomicU64::new(0),
            session_id: session_id.clone(),
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

impl McpProxyCommand {
    /// Refuse to start with a key the API rejects: every call would then be
    /// refused, which is a configuration error to report now, not per call.
    /// An API that cannot be reached is an outage, not a bad key, so the
    /// proxy starts and its fail mode applies.
    fn verify_key(&self, client: &PromptGuardClient) -> Result<()> {
        match client.send_raw(
            &reqwest::Method::GET,
            "/projects",
            None,
            STARTUP_CHECK_TIMEOUT,
        ) {
            RawResponse::Answered {
                status: 401 | 403, ..
            } => Err(PromptGuardError::Auth(
                "PromptGuard rejected the API key, so mcp-proxy did not start the MCP server. \
                 Run `promptguard login` with a valid key, or set PROMPTGUARD_API_KEY."
                    .to_string(),
            )),
            RawResponse::Answered { status, .. } if (200..300).contains(&status) => Ok(()),
            RawResponse::Answered { status, .. } => {
                log(&format!(
                    "could not verify the API key at startup (the API returned {status}); \
                     each tool call is still checked"
                ));
                Ok(())
            },
            RawResponse::NoAnswer(e) => {
                log(&format!(
                    "could not verify the API key at startup ({e}); tool calls follow \
                     --fail-mode {} until the API answers",
                    self.fail_mode.as_str()
                ));
                Ok(())
            },
        }
    }
}

/// `mcp-proxy:<server>`, from the command's file name and, for a package
/// runner, the package it runs. Never the full command line: arguments can
/// carry tokens, and the agent id is sent to the API and stored in the pins
/// file. So a flag's value is never taken for the package, and a package
/// given as a URL loses its credentials, query and fragment.
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
        .then(|| runner_package(&command[1..]))
        .flatten();
    match package {
        Some(p) => format!("mcp-proxy:{program} {}", without_credentials(p)),
        None => format!("mcp-proxy:{program}"),
    }
}

/// The first argument that is neither a flag nor a flag's value.
fn runner_package(args: &[String]) -> Option<&str> {
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        if arg == "--" {
            return args.next().map(String::as_str);
        }
        if !arg.starts_with('-') {
            return Some(arg);
        }
        if !arg.contains('=') && RUNNER_VALUE_FLAGS.contains(&arg.as_str()) {
            args.next();
        }
    }
    None
}

/// `arg` with a URL's userinfo (`user:pass@`, `TOKEN@`), query and fragment
/// removed; anything that is not a URL is returned as it is.
fn without_credentials(arg: &str) -> String {
    let Some((scheme, rest)) = arg.split_once("://") else {
        return arg.to_string();
    };
    let rest = rest.split(['?', '#']).next().unwrap_or_default();
    let authority_end = rest.find('/').unwrap_or(rest.len());
    let (authority, path) = rest.split_at(authority_end);
    let host = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    format!("{scheme}://{host}{path}")
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

/// The pending-set key for an id. Numbers are normalised, so `1`, `1.0` and
/// `1e0` are one id, as a server parsing them to the same number sees them;
/// strings keep their quotes, so `"1"` stays a different id from `1`.
fn id_key(id: &Value) -> String {
    /// The largest integer an f64 holds exactly.
    const EXACT: f64 = 9_007_199_254_740_992.0;
    match id {
        Value::Number(n) if n.is_f64() => match n.as_f64() {
            #[allow(clippy::cast_possible_truncation)] // whole and within EXACT
            Some(f) if f.fract() == 0.0 && f.abs() <= EXACT => format!("{}", f as i64),
            _ => n.to_string(),
        },
        other => other.to_string(),
    }
}

/// Whether a request carries a non-null `params.cursor`.
fn has_cursor(message: &Value) -> bool {
    message
        .pointer("/params/cursor")
        .is_some_and(|c| !c.is_null())
}

fn method_of(message: &Value) -> Option<&str> {
    message.get("method").and_then(Value::as_str)
}

/// What a client request is waiting for, so its response is inspected for
/// what it is: a tool result is redacted, a tool list is scanned.
#[derive(Debug, Clone)]
enum Awaiting {
    ToolResult { tool: String },
    ToolList,
    Other,
}

impl Awaiting {
    fn of(message: &Value) -> Self {
        match method_of(message) {
            Some("tools/list") => Self::ToolList,
            _ => Self::Other,
        }
    }
}

struct Pending {
    since: Instant,
    awaiting: Awaiting,
}

/// A server message that needs the API before it is relayed: its index in a
/// batch (0 for a single message), its pending key, and what it answers.
struct Inspection {
    index: usize,
    key: Option<String>,
    awaiting: Awaiting,
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

/// A counting semaphore: [`Permits::acquire`] blocks while `cap` permits are
/// out, and a [`Permit`] gives its slot back when dropped.
struct Permits {
    out: Mutex<usize>,
    freed: Condvar,
    cap: usize,
}

struct Permit(Arc<Permits>);

impl Permits {
    fn new(cap: usize) -> Arc<Self> {
        Arc::new(Self {
            out: Mutex::new(0),
            freed: Condvar::new(),
            cap,
        })
    }

    fn acquire(self: &Arc<Self>) -> Permit {
        let mut out = lock(&self.out);
        while *out >= self.cap {
            out = self.freed.wait(out).unwrap_or_else(PoisonError::into_inner);
        }
        *out += 1;
        Permit(Arc::clone(self))
    }
}

impl Drop for Permit {
    fn drop(&mut self) {
        *lock(&self.0.out) -= 1;
        self.0.freed.notify_one();
    }
}

struct Proxy {
    fail_mode: FailMode,
    validator: Box<dyn Validator>,
    redactor: Box<dyn Redactor>,
    scanner: Box<dyn ManifestScanner>,
    /// The server's name in logs and the pins file (`mcp-proxy:<program>`).
    server: String,
    /// `None` when there is no home directory to keep pins in.
    pins: Option<PinStore>,
    /// Tools removed from a tool list for a finding; a call to one is refused.
    withheld: Mutex<HashSet<String>>,
    /// Client requests awaiting a server response, by serialized id.
    pending: Mutex<HashMap<String, Pending>>,
    /// `None` once the client has gone and the server's stdin is closed.
    to_server: Mutex<Option<ChildStdin>>,
    /// `--repin`, until the first complete scan has replaced the pin.
    repin: AtomicBool,
    /// Bounds the inspection threads ([`MAX_INSPECTIONS`]).
    inspections: Arc<Permits>,
    /// Requests the proxy sent the server itself, by id, and where their
    /// answers go.
    internal: Mutex<HashMap<String, mpsc::Sender<Value>>>,
    next_internal: AtomicU64,
    /// This run's id, which makes the proxy's own request ids unique.
    session_id: String,
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

        if method == "tools/list" && has_cursor(message) {
            if let Some(id) = &id {
                self.write_client_value(&refusal(id, CURSOR_REFUSED));
            }
            return;
        }
        if method != "tools/call" {
            if let Some(id) = &id {
                if let Err(why) = self.track(&[(id_key(id), Awaiting::of(message))]) {
                    self.write_client_value(&refusal(id, why));
                    return;
                }
            }
            self.write_server(&raw);
            return;
        }

        // MCP's tools/call is a request, so one without an id is malformed.
        // It is refused before any work, so a flood of them takes no pending
        // slot and no check thread. JSON-RPC answers a request whose id it
        // cannot use with a null id.
        let Some(id) = id else {
            log("refused a tools/call without an id");
            self.write_client_value(&refusal(
                &Value::Null,
                "Invalid tools/call: it must have an id",
            ));
            return;
        };
        let (tool, arguments) = match parse_tool_call(message) {
            Ok(call) => call,
            Err(why) => {
                log(&format!("refused a malformed tools/call: {why}"));
                self.write_client_value(&refusal(&id, &format!("Invalid tools/call: {why}")));
                return;
            },
        };
        if lock(&self.withheld).contains(&tool) {
            log(&format!(
                "refused tools/call `{tool}`: the tool was withheld from the tool list"
            ));
            self.write_client_value(&refusal(
                &id,
                &format!(
                    "PromptGuard refused tool call `{tool}`: the tool was withheld from this \
                     server's tool list for a security finding"
                ),
            ));
            return;
        }
        let key = id_key(&id);
        let awaiting = Awaiting::ToolResult { tool: tool.clone() };
        if let Err(why) = self.track(&[(key.clone(), awaiting)]) {
            self.write_client_value(&refusal(&id, why));
            return;
        }

        let proxy = Arc::clone(self);
        let (thread_key, thread_id) = (key.clone(), id.clone());
        let check = thread::Builder::new()
            .name("mcp-proxy-check".to_string())
            .spawn(move || {
                proxy.check_and_forward(&raw, &thread_key, &thread_id, &tool, &arguments);
            });
        if let Err(e) = check {
            // Nothing will check or answer the call: free its slot and say so.
            log(&format!("could not start a check: {e}"));
            self.untrack(&key);
            self.write_client_value(&refusal(
                &id,
                "the proxy could not start a check for this call",
            ));
        }
    }

    fn check_and_forward(&self, raw: &[u8], key: &str, id: &Value, tool: &str, arguments: &Value) {
        let decision = check::decide(self.fail_mode, tool, arguments, self.validator.as_ref());
        // A relayed call keeps its slot until the server answers; a refused
        // one is finished with now.
        if matches!(decision, Decision::Refuse { .. }) {
            self.untrack(key);
        }
        match decision {
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
                self.write_client_value(&refusal(
                    id,
                    &format!("PromptGuard refused tool call `{tool}`: {reason}"),
                ));
            },
        }
    }

    /// A batch is relayed whole or refused whole. One holding a `tools/call`
    /// is refused: checking its calls one by one would mean splitting the
    /// batch, and forwarding it unchecked would be a way around the proxy.
    /// So is one holding anything but objects (a nested batch, a string):
    /// JSON-RPC allows only objects there, and a server that unwrapped a
    /// nested batch would run a `tools/call` the proxy never looked at.
    /// (MCP dropped batching in its 2025-06-18 revision.)
    fn handle_client_batch(&self, items: &[Value]) {
        // The batch's requests: messages with a method and an id.
        let requests: Vec<&Value> = items
            .iter()
            .filter(|m| method_of(m).is_some())
            .filter_map(id_of)
            .collect();
        let refuse_all = |why: &str| {
            log(&format!("refused a batch: {why}"));
            let replies: Vec<Value> = requests.iter().map(|id| refusal(id, why)).collect();
            if !replies.is_empty() {
                self.write_client_value(&Value::Array(replies));
            }
        };

        if items.iter().any(|m| !m.is_object()) {
            refuse_all("a batch may contain only JSON-RPC objects");
            return;
        }
        if items
            .iter()
            .any(|m| method_of(m) == Some("tools/list") && has_cursor(m))
        {
            refuse_all(CURSOR_REFUSED);
            return;
        }
        if items.iter().any(|m| method_of(m) == Some("tools/call")) {
            refuse_all(
                "a batch containing tools/call cannot be checked; send tool calls one at a time",
            );
            return;
        }
        if requests.len() > MAX_PENDING {
            refuse_all(TOO_MANY_PENDING);
            return;
        }
        let keys: Vec<(String, Awaiting)> = items
            .iter()
            .filter(|m| method_of(m).is_some())
            .filter_map(|m| id_of(m).map(|id| (id_key(id), Awaiting::of(m))))
            .collect();
        if let Err(why) = self.track(&keys) {
            refuse_all(why);
            return;
        }
        self.write_server(&canonical(&Value::Array(items.to_vec())));
    }

    /// Record requests as pending, all or none: each key must be new (to the
    /// pending set and within `keys`) and the set must stay within the cap.
    ///
    /// Every message takes this lock, so the work under it is bounded by
    /// `keys.len()` (at most [`MAX_PENDING`]), never by anything the client
    /// controls: the cap is checked first, duplicates are found by hashing,
    /// and expired entries are swept only when the set is otherwise full (an
    /// expired id is not a duplicate, swept or not).
    fn track(&self, keys: &[(String, Awaiting)]) -> std::result::Result<(), &'static str> {
        if keys.len() > MAX_PENDING {
            return Err(TOO_MANY_PENDING);
        }
        let mut pending = lock(&self.pending);
        if pending.len() + keys.len() > MAX_PENDING {
            pending.retain(|_, p| p.since.elapsed() < PENDING_TTL);
            if pending.len() + keys.len() > MAX_PENDING {
                return Err(TOO_MANY_PENDING);
            }
        }
        let live = |key: &String| {
            pending
                .get(key)
                .is_some_and(|p| p.since.elapsed() < PENDING_TTL)
        };
        let lists = keys
            .iter()
            .filter(|(_, a)| matches!(a, Awaiting::ToolList))
            .count();
        if lists > 0
            && lists
                + pending
                    .values()
                    .filter(|p| matches!(p.awaiting, Awaiting::ToolList))
                    .count()
                > MAX_LIST_REQUESTS
        {
            return Err(TOO_MANY_LISTS);
        }
        let mut seen = HashSet::with_capacity(keys.len());
        if keys.iter().any(|(key, _)| live(key) || !seen.insert(key)) {
            return Err("a request with this id is already pending");
        }
        let now = Instant::now();
        for (key, awaiting) in keys {
            pending.insert(
                key.clone(),
                Pending {
                    since: now,
                    awaiting: awaiting.clone(),
                },
            );
        }
        Ok(())
    }

    /// What a pending request is waiting for, leaving it pending.
    fn awaiting(&self, key: &str) -> Option<Awaiting> {
        lock(&self.pending).get(key).map(|p| p.awaiting.clone())
    }

    fn untrack(&self, key: &str) {
        lock(&self.pending).remove(key);
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

    fn relay_server_to_client(self: &Arc<Self>, from_server: ChildStdout) {
        let mut reader = BufReader::new(from_server);
        loop {
            match line::read_line(&mut reader, MAX_LINE_BYTES) {
                Ok(None) => break,
                Ok(Some(Line::Complete(raw))) => self.handle_server_line(raw),
                Ok(Some(Line::Oversized(prefix))) => {
                    // Too large to redact or scan, so it is dropped in every
                    // mode: relaying it would let a server skip both by
                    // padding. Its ids are read as it is skipped, so each
                    // request it answered gets an error instead of waiting.
                    let mut ids = line::IdScanner::default();
                    ids.feed(&prefix);
                    let skipped = line::finish_line(&mut reader, Some(&mut ids));
                    self.drop_oversized(&ids);
                    if skipped.is_err() {
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

    /// Answer each pending request a dropped oversized server line answered.
    fn drop_oversized(&self, ids: &line::IdScanner) {
        log(
            "dropped a server message over 4 MiB in every fail mode: too large to redact \
             or scan, so it was not relayed",
        );
        for id in ids.response_ids() {
            let key = id_key(id);
            if self.awaiting(&key).is_none() {
                continue;
            }
            self.untrack(&key);
            self.write_client_value(&refusal(
                id,
                "PromptGuard withheld the server's answer: it is over 4 MiB, too large to \
                 redact or scan",
            ));
        }
    }

    /// Relay a server line. A line holding a tool result or a tool list is
    /// inspected on its own thread and relayed when that is done; any other
    /// line is relayed byte for byte at once. Either way a response frees its
    /// request's pending slot, once it has been written.
    fn handle_server_line(self: &Arc<Self>, raw: Vec<u8>) {
        let parsed = serde_json::from_slice::<Value>(&raw).ok();
        // The answer to a request the proxy made itself (a later page of a
        // tool list) goes to the thread waiting for it, never to the client.
        if let Some(message @ Value::Object(_)) = &parsed {
            let waiter = message
                .get("id")
                .and_then(Value::as_str)
                .filter(|_| method_of(message).is_none())
                .and_then(|id| lock(&self.internal).remove(id));
            if let Some(waiter) = waiter {
                let _ = waiter.send(message.clone());
                return;
            }
        }
        let items: Vec<&Value> = match &parsed {
            Some(message @ Value::Object(_)) => vec![message],
            Some(Value::Array(items)) => items.iter().collect(),
            _ => Vec::new(),
        };
        let mut inspections = Vec::new();
        for (index, message) in items.into_iter().enumerate() {
            // A response is an id and no method. Server requests and
            // notifications carry a method and hold no slot.
            if method_of(message).is_some() {
                continue;
            }
            let key = id_of(message).map(id_key);
            let awaiting = key.as_deref().and_then(|k| self.awaiting(k));
            let Some(result) = message.get("result") else {
                key.inspect(|k| self.untrack(k));
                continue;
            };
            let awaiting = match awaiting {
                Some(a @ (Awaiting::ToolResult { .. } | Awaiting::ToolList { .. })) => a,
                // A result whose request is no longer pending (it expired) is
                // still redacted if it has a tool result's shape.
                None if result.get("content").is_some_and(Value::is_array) => {
                    Awaiting::ToolResult {
                        tool: "unknown".to_string(),
                    }
                },
                _ => {
                    key.inspect(|k| self.untrack(k));
                    continue;
                },
            };
            inspections.push(Inspection {
                index,
                key,
                awaiting,
            });
        }

        let Some(parsed) = parsed.filter(|_| !inspections.is_empty()) else {
            self.write_client(&raw);
            return;
        };
        // Blocks the server reader while MAX_INSPECTIONS are running: the
        // backpressure that bounds the threads below.
        let permit = self.inspections.acquire();
        let line = Arc::new((raw, parsed, inspections));
        let (proxy, work) = (Arc::clone(self), Arc::clone(&line));
        let spawned = thread::Builder::new()
            .name("mcp-proxy-inspect".to_string())
            .spawn(move || {
                proxy.inspect_and_relay(&work.0, work.1.clone(), &work.2);
                drop(permit);
            });
        if let Err(e) = spawned {
            // Inspect it here instead, holding up the server's later lines,
            // rather than relay it uninspected or not at all.
            log(&format!("could not start an inspection thread: {e}"));
            self.inspect_and_relay(&line.0, line.1.clone(), &line.2);
        }
    }

    /// Redact the tool results and review the tool lists in one server line,
    /// then relay it: as the server sent it when nothing changed.
    fn inspect_and_relay(&self, raw: &[u8], mut parsed: Value, inspections: &[Inspection]) {
        let mut changed = false;
        for inspection in inspections {
            let message = match &mut parsed {
                Value::Array(items) => &mut items[inspection.index],
                single => single,
            };
            if let Some(replacement) = self.inspect(message, &inspection.awaiting) {
                *message = replacement;
                changed = true;
            }
        }
        if changed {
            self.write_client(&canonical(&parsed));
        } else {
            self.write_client(raw);
        }
        for key in inspections.iter().filter_map(|i| i.key.as_deref()) {
            self.untrack(key);
        }
    }

    /// The message to relay in place of `message`, a response with a result,
    /// or `None` to relay it as it is.
    fn inspect(&self, message: &Value, awaiting: &Awaiting) -> Option<Value> {
        let id = message.get("id").cloned().unwrap_or(Value::Null);
        let result = message.get("result")?;
        match awaiting {
            Awaiting::ToolResult { tool } => {
                let outcome = redact::review(
                    self.fail_mode,
                    tool,
                    result,
                    self.redactor.as_ref(),
                    redact::RESULT_DEADLINE,
                );
                let with_result = |result: Value| {
                    let mut out = message.clone();
                    out["result"] = result;
                    out
                };
                match outcome {
                    redact::Outcome::Unchanged => None,
                    redact::Outcome::Redacted(result) => {
                        log(&format!("redacted the result of tools/call `{tool}`"));
                        Some(with_result(result))
                    },
                    redact::Outcome::Unredacted { result, why } => {
                        log(&format!(
                            "unredacted: relayed the result of tools/call `{tool}` without \
                             PromptGuard redaction (fail mode {}): {why}",
                            self.fail_mode.as_str()
                        ));
                        Some(with_result(result))
                    },
                    redact::Outcome::Withhold { reason } => {
                        log(&format!(
                            "withheld the result of tools/call `{tool}`: {reason}"
                        ));
                        Some(refusal(
                            &id,
                            &format!(
                                "PromptGuard withheld the result of tool call `{tool}`: {reason}"
                            ),
                        ))
                    },
                }
            },
            Awaiting::ToolList => match self.collect_tool_list(message) {
                Ok(Some(tools)) => self.review_tool_list(&id, message, &tools),
                Ok(None) => None,
                Err(reason) => {
                    log(&format!("refused tools/list: {reason}"));
                    Some(refusal(
                        &id,
                        &format!("PromptGuard refused this server's tool list: {reason}"),
                    ))
                },
            },
            Awaiting::Other => None,
        }
    }

    /// Every tool the server lists: the first page's, then each later page's,
    /// fetched from the server by the proxy itself, so the whole list is
    /// scanned and pinned. A server answering `tools: []` and a cursor must
    /// not get past the pin by putting its tools on page two. `Ok(None)` is a
    /// result with no tool list to read; `Err` refuses the list in every
    /// mode, since a list the proxy cannot finish reading cannot be pinned.
    fn collect_tool_list(
        &self,
        message: &Value,
    ) -> std::result::Result<Option<Vec<Value>>, String> {
        let Some(first) = message.pointer("/result/tools").and_then(Value::as_array) else {
            return Ok(None);
        };
        let mut tools = first.clone();
        let mut page = message.get("result").cloned().unwrap_or(Value::Null);
        let mut cursors = HashSet::new();
        loop {
            let cursor = match page.get("nextCursor") {
                None | Some(Value::Null) => return Ok(Some(tools)),
                Some(Value::String(c)) => c.clone(),
                Some(_) => return Err("its nextCursor is not text".to_string()),
            };
            if !cursors.insert(cursor.clone()) {
                return Err("its pages repeat a cursor, so the list never ends".to_string());
            }
            if cursors.len() > MAX_PAGES {
                return Err(format!("it has more than {MAX_PAGES} pages"));
            }
            page = self.fetch_page(&cursor)?;
            let Some(more) = page.get("tools").and_then(Value::as_array) else {
                return Err("a later page has no tools array".to_string());
            };
            tools.extend(more.iter().cloned());
            if tools.len() > MAX_LISTED_TOOLS {
                return Err(format!("it lists more than {MAX_LISTED_TOOLS} tools"));
            }
        }
    }

    /// Ask the server for the tool list page at `cursor`, under an id of the
    /// proxy's own, and wait for its answer, which is not relayed.
    fn fetch_page(&self, cursor: &str) -> std::result::Result<Value, String> {
        let n = self.next_internal.fetch_add(1, Ordering::Relaxed);
        let id = format!("promptguard-mcp-proxy:{}:{n}", self.session_id);
        let (tx, rx) = mpsc::channel();
        lock(&self.internal).insert(id.clone(), tx);
        self.write_server(&canonical(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/list",
            "params": {"cursor": cursor},
        })));
        let answer = rx.recv_timeout(check::CHECK_TIMEOUT);
        lock(&self.internal).remove(&id);
        match answer {
            Ok(message) => message.get("result").cloned().ok_or_else(|| {
                "the server answered a request for a later page with an error".to_string()
            }),
            Err(_) => Err(format!(
                "the server did not send a later page within {}s",
                check::CHECK_TIMEOUT.as_secs()
            )),
        }
    }

    fn review_tool_list(&self, id: &Value, message: &Value, tools: &[Value]) -> Option<Value> {
        let loaded = self.pins.as_ref().map(PinStore::load);
        let (stored, writable) = loaded
            .as_ref()
            .map_or((None, false), |l| (l.pin.clone(), l.writable));
        let pins_path = self.pins.as_ref().map_or_else(
            || "the pins file".to_string(),
            |p| p.path().display().to_string(),
        );
        if let Some(problem) = loaded.and_then(|l| l.problem) {
            log(&format!("the pins file {pins_path}: {problem}"));
        }
        let review = manifest::review(
            &manifest::Context {
                mode: self.fail_mode,
                server: &self.server,
                pins_path: &pins_path,
                stored: stored.as_ref(),
                now: chrono::Utc::now().to_rfc3339(),
                repin: self.repin.load(Ordering::Relaxed),
            },
            tools,
            self.scanner.as_ref(),
        );
        review.log.iter().for_each(|line| log(line));

        if let Some(reason) = review.refuse {
            log(&format!("refused tools/list: {reason}"));
            return Some(refusal(
                id,
                &format!("PromptGuard refused this server's tool list: {reason}"),
            ));
        }
        if let (Some(pin), Some(store), true) = (&review.new_pin, &self.pins, writable) {
            match store.save(pin) {
                Ok(()) if review.repinned => {
                    // Once per run: a change later in the session is compared
                    // with the list just accepted.
                    self.repin.store(false, Ordering::Relaxed);
                    log(&format!(
                        "re-pinned the tool list of `{}` ({} tools) in {pins_path}, as --repin \
                         asked: its old pin was replaced; no other server's pin changed",
                        self.server,
                        tools.len()
                    ));
                },
                Ok(()) => {
                    self.repin.store(false, Ordering::Relaxed);
                    log(&format!(
                        "pinned the tool list of `{}` ({} tools) in {pins_path}",
                        self.server,
                        tools.len()
                    ));
                },
                Err(e) => log(&format!(
                    "could not save the tool list's pin to {pins_path}: {e}"
                )),
            }
        }
        if review.matches_pin {
            log(&format!(
                "the tool list of `{}` ({} tools) matches its pin",
                self.server,
                tools.len()
            ));
        }

        // A name is withheld when any tool of that name is, so a duplicate
        // cannot carry a withheld tool's calls through.
        let name_of = |t: &Value| t.get("name").and_then(Value::as_str).map(str::to_string);
        let withheld_names: HashSet<String> = review
            .withhold
            .iter()
            .filter_map(|&i| tools.get(i).and_then(name_of))
            .collect();
        {
            let mut withheld = lock(&self.withheld);
            for name in tools.iter().filter_map(name_of) {
                if withheld_names.contains(&name) {
                    withheld.insert(name);
                } else {
                    withheld.remove(&name);
                }
            }
        }
        // A list read from one page and with nothing withheld is relayed as
        // the server sent it; otherwise the whole list goes in one page.
        let paged = message.pointer("/result/nextCursor").is_some();
        if review.withhold.is_empty() && !paged {
            return None;
        }
        let kept: Vec<Value> = tools
            .iter()
            .enumerate()
            .filter(|(i, _)| !review.withhold.contains(i))
            .map(|(_, t)| t.clone())
            .collect();
        let mut out = message.clone();
        out["result"]["tools"] = Value::Array(kept);
        if let Some(result) = out["result"].as_object_mut() {
            result.remove("nextCursor");
        }
        Some(out)
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
    fn default_agent_id_skips_flag_values_and_strips_url_credentials() {
        for (command, expected) in [
            (
                &["uvx", "--from", "git+https://TOKEN@github.com/x/y", "srv"][..],
                "mcp-proxy:uvx srv",
            ),
            (
                &["uvx", "--from=git+https://TOKEN@github.com/x/y", "srv"][..],
                "mcp-proxy:uvx srv",
            ),
            (
                &[
                    "uvx",
                    "--with",
                    "httpx",
                    "--python",
                    "3.12",
                    "mcp-server-fetch",
                ][..],
                "mcp-proxy:uvx mcp-server-fetch",
            ),
            (
                &["uvx", "git+https://user:pass@github.com/x/y?token=abc#frag"][..],
                "mcp-proxy:uvx git+https://github.com/x/y",
            ),
            (
                &[
                    "npx",
                    "-y",
                    "--registry",
                    "https://u:p@npm.example",
                    "@scope/server",
                ][..],
                "mcp-proxy:npx @scope/server",
            ),
            (
                &["npx", "--package", "https://TOKEN@host/pkg.tgz", "server"][..],
                "mcp-proxy:npx server",
            ),
            (
                &["npx", "https://TOKEN@host/pkg.tgz"][..],
                "mcp-proxy:npx https://host/pkg.tgz",
            ),
        ] {
            let id = default_agent_id(&cmd(command));
            assert_eq!(id, expected, "{command:?}");
            assert!(!id.contains("TOKEN") && !id.contains("pass"), "{id}");
        }
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
