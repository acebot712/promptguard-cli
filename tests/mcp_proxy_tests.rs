#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! `promptguard mcp-proxy -- <server command>` against a fake stdio MCP server
//! (`tests/support/fake_mcp_server.py`) and a fake `PromptGuard` API.
//!
//! The fake API mirrors `POST /api/v1/agent/validate-tool` in the backend
//! (`developer/agent/router.py`): it refuses any tool whose name starts with
//! `forbidden`, and allows the rest. "Unreachable" is a port nothing listens
//! on, so the client sees a refused connection, as in a real outage.

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use tempfile::TempDir;

const RECV_TIMEOUT: Duration = Duration::from_secs(20);

// ---------------------------------------------------------------------------
// Fake PromptGuard API
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct ApiRequest {
    path: String,
    api_key: Option<String>,
    body: Value,
}

/// How the fake API answers.
#[derive(Debug, Clone, Copy)]
enum Verdicts {
    /// Refuse tools named `forbidden*`, allow the rest.
    Normal,
    /// Answer every validate-tool request with this status and an error body.
    Status(u16),
    /// Answer 200 with a body that is not a verdict.
    Malformed,
    /// Answer normally, after this long.
    Slow(Duration),
}

#[derive(Debug, Clone, Copy)]
struct Behaviour {
    /// Status of `GET /projects`, the proxy's startup key check.
    projects: u16,
    verdicts: Verdicts,
}

struct FakeApi {
    base_url: String,
    requests: Arc<Mutex<Vec<ApiRequest>>>,
}

impl FakeApi {
    fn start() -> Self {
        Self::with(Behaviour {
            projects: 200,
            verdicts: Verdicts::Normal,
        })
    }

    fn verdicts(verdicts: Verdicts) -> Self {
        Self::with(Behaviour {
            projects: 200,
            verdicts,
        })
    }

    fn with(behaviour: Behaviour) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base_url = format!("http://{}/api/v1", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&requests);
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let recorded = Arc::clone(&recorded);
                thread::spawn(move || serve_one(stream, &recorded, behaviour));
            }
        });
        Self { base_url, requests }
    }

    /// The validate-tool requests received (the startup key check excluded).
    fn requests(&self) -> Vec<ApiRequest> {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.path.ends_with("/agent/validate-tool"))
            .cloned()
            .collect()
    }
}

fn error_body(status: u16) -> String {
    let (kind, code) = match status {
        401 | 403 => ("authentication_error", "invalid_api_key"),
        429 => ("rate_limit_error", "quota_exceeded"),
        _ => ("api_error", "server_error"),
    };
    json!({"error": {"message": format!("fake {status}"), "type": kind, "code": code}}).to_string()
}

fn respond(mut stream: TcpStream, status: u16, payload: &str) {
    let response = format!(
        "HTTP/1.1 {status} Fake\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
        payload.len()
    );
    let _ = stream.write_all(response.as_bytes());
}

fn serve_one(stream: TcpStream, recorded: &Mutex<Vec<ApiRequest>>, behaviour: Behaviour) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).is_err() {
        return;
    }
    let path = request_line
        .split_whitespace()
        .nth(1)
        .unwrap_or_default()
        .to_string();
    let mut content_length = 0usize;
    let mut api_key = None;
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header).is_err() || header == "\r\n" || header.is_empty() {
            break;
        }
        let (name, value) = header.split_once(':').unwrap_or((&header, ""));
        let value = value.trim().to_string();
        if name.eq_ignore_ascii_case("content-length") {
            content_length = value.parse().unwrap_or(0);
        } else if name.eq_ignore_ascii_case("x-api-key") {
            api_key = Some(value);
        }
    }
    let mut body = vec![0u8; content_length];
    reader.read_exact(&mut body).unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);

    let tool = body["tool_name"].as_str().unwrap_or_default().to_string();
    recorded.lock().unwrap().push(ApiRequest {
        path: path.clone(),
        api_key,
        body,
    });

    if path.ends_with("/projects") {
        let payload = if behaviour.projects == 200 {
            "[]".to_string()
        } else {
            error_body(behaviour.projects)
        };
        respond(stream, behaviour.projects, &payload);
        return;
    }

    match behaviour.verdicts {
        Verdicts::Status(status) => return respond(stream, status, &error_body(status)),
        Verdicts::Malformed => return respond(stream, 200, r#"{"ok":true}"#),
        Verdicts::Slow(delay) => thread::sleep(delay),
        Verdicts::Normal => {},
    }
    let verdict = if tool.starts_with("forbidden") {
        json!({
            "allowed": false,
            "risk_score": 1.0,
            "risk_level": "critical",
            "reason": format!("Tool '{tool}' is blocked for security reasons"),
            "warnings": ["blocked_tool"],
            "blocked_reasons": [format!("Tool '{tool}' is blocked for security reasons")],
        })
    } else {
        json!({
            "allowed": true,
            "risk_score": 0.0,
            "risk_level": "safe",
            "reason": "Tool call approved",
            "warnings": [],
            "blocked_reasons": [],
        })
    };
    respond(stream, 200, &verdict.to_string());
}

/// A base URL on which nothing listens: the connection is refused.
fn unreachable_base_url() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    format!("http://{addr}/api/v1")
}

// ---------------------------------------------------------------------------
// The proxy under test
// ---------------------------------------------------------------------------

fn fake_server_script() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/support/fake_mcp_server.py")
}

struct Proxy {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: Receiver<String>,
    stderr: Arc<Mutex<String>>,
    server_log: PathBuf,
    syncs: u32,
    _home: TempDir,
}

impl Proxy {
    fn start(base_url: &str, flags: &[&str]) -> Self {
        Self::start_with_server_args(base_url, flags, &[])
    }

    /// As [`start`](Self::start), passing `server_args` to the fake server
    /// (which ignores them).
    fn start_with_server_args(base_url: &str, flags: &[&str], server_args: &[&str]) -> Self {
        let home = TempDir::new().unwrap();
        let server_log = home.path().join("server.log");
        let mut child = Command::new(env!("CARGO_BIN_EXE_promptguard"))
            .arg("mcp-proxy")
            .args(flags)
            .arg("--")
            .arg("python3")
            .arg(fake_server_script())
            .args(server_args)
            .current_dir(home.path())
            .env("HOME", home.path())
            .env("USERPROFILE", home.path())
            .env("PROMPTGUARD_API_KEY", "pg_test_mcp_proxy_fixture")
            .env("PROMPTGUARD_BASE_URL", base_url)
            .env("FAKE_MCP_LOG", &server_log)
            .env("NO_COLOR", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn promptguard mcp-proxy");

        let (tx, rx) = mpsc::channel();
        let stdout = child.stdout.take().unwrap();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        let stderr_buf = Arc::new(Mutex::new(String::new()));
        let sink = Arc::clone(&stderr_buf);
        let stderr = child.stderr.take().unwrap();
        thread::spawn(move || {
            for line in BufReader::new(stderr).lines() {
                let Ok(line) = line else { break };
                let mut s = sink.lock().unwrap();
                s.push_str(&line);
                s.push('\n');
            }
        });

        let stdin = child.stdin.take();
        Self {
            child,
            stdin,
            stdout: rx,
            stderr: stderr_buf,
            server_log,
            syncs: 0,
            _home: home,
        }
    }

    fn send_raw(&mut self, line: &str) {
        let stdin = self.stdin.as_mut().expect("stdin open");
        stdin.write_all(line.as_bytes()).unwrap();
        stdin.write_all(b"\n").unwrap();
        stdin.flush().unwrap();
    }

    fn send(&mut self, message: &Value) {
        self.send_raw(&message.to_string());
    }

    fn call(&mut self, id: u64, tool: &str, arguments: &Value) -> Value {
        self.send(&json!({
            "jsonrpc": "2.0", "id": id, "method": "tools/call",
            "params": {"name": tool, "arguments": arguments},
        }));
        self.recv_id(id)
    }

    /// The next stdout line, raw.
    fn recv_line(&self) -> String {
        self.stdout.recv_timeout(RECV_TIMEOUT).unwrap_or_else(|_| {
            panic!(
                "no output from the proxy within {RECV_TIMEOUT:?}\nstderr:\n{}",
                self.stderr()
            )
        })
    }

    /// The first JSON message on stdout that `pred` accepts, skipping others.
    fn recv_matching(&self, pred: impl Fn(&Value) -> bool) -> Value {
        loop {
            let line = self.recv_line();
            if let Ok(value) = serde_json::from_str::<Value>(&line) {
                if pred(&value) {
                    return value;
                }
            }
        }
    }

    /// Whether any line the server received contains `needle`.
    fn server_raw_contains(&mut self, needle: &str) -> bool {
        self.sync();
        self.server_received()
            .iter()
            .any(|e| e["raw"].as_str().is_some_and(|r| r.contains(needle)))
    }

    /// The JSON-RPC message answering `id`, skipping anything else.
    fn recv_id(&self, id: u64) -> Value {
        loop {
            let line = self.recv_line();
            if let Ok(value) = serde_json::from_str::<Value>(&line) {
                if value["id"] == json!(id) {
                    return value;
                }
            }
        }
    }

    fn stderr(&self) -> String {
        self.stderr.lock().unwrap().clone()
    }

    /// Wait until stderr contains `needle` (the proxy logs asynchronously).
    fn wait_stderr(&self, needle: &str) -> String {
        for _ in 0..200 {
            let s = self.stderr();
            if s.contains(needle) {
                return s;
            }
            thread::sleep(Duration::from_millis(50));
        }
        panic!("stderr never contained {needle:?}:\n{}", self.stderr());
    }

    /// Every message the fake server received, in order.
    fn server_received(&self) -> Vec<Value> {
        std::fs::read_to_string(&self.server_log)
            .unwrap_or_default()
            .lines()
            .filter_map(|l| serde_json::from_str::<Value>(l).ok())
            .collect()
    }

    /// Round-trip a ping, so everything sent before it has reached the server
    /// (and its log) before a test asserts something did not.
    fn sync(&mut self) {
        let id = 9_000_000 + u64::from(self.syncs);
        self.syncs += 1;
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": "ping"}));
        assert_eq!(self.recv_id(id)["result"], json!({}));
    }

    /// Whether a `tools/call` with this id never reached the server.
    fn never_reached_server(&mut self, id: u64) -> bool {
        self.sync();
        !self.server_saw_call(id)
    }

    /// Whether the server ever received a `tools/call` with this id.
    fn server_saw_call(&self, id: u64) -> bool {
        self.server_received()
            .iter()
            .any(|e| e["event"] == "message" && e["method"] == "tools/call" && e["id"] == json!(id))
    }

    fn initialize(&mut self) {
        self.send(&json!({
            "jsonrpc": "2.0", "id": 0, "method": "initialize",
            "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "test", "version": "0"}},
        }));
        let reply = self.recv_id(0);
        assert_eq!(reply["result"]["serverInfo"]["name"], "fake-mcp-server");
    }

    /// Wait for the proxy to exit on its own.
    fn wait_exit(&mut self) -> i32 {
        for _ in 0..400 {
            if let Some(status) = self.child.try_wait().unwrap() {
                // Let the stderr reader drain.
                thread::sleep(Duration::from_millis(100));
                return status.code().unwrap_or(-1);
            }
            thread::sleep(Duration::from_millis(50));
        }
        panic!("proxy did not exit\nstderr:\n{}", self.stderr());
    }

    fn close_and_wait(mut self) -> i32 {
        drop(self.stdin.take());
        for _ in 0..200 {
            if let Some(status) = self.child.try_wait().unwrap() {
                return status.code().unwrap_or(-1);
            }
            thread::sleep(Duration::from_millis(50));
        }
        let _ = self.child.kill();
        panic!("proxy did not exit after stdin closed");
    }
}

impl Drop for Proxy {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn assert_refused(reply: &Value) {
    assert_eq!(
        reply["error"]["code"],
        json!(-32600),
        "expected a -32600 refusal, got {reply}"
    );
    assert!(reply.get("result").is_none(), "a refusal carries no result");
}

fn assert_relayed(reply: &Value, tool: &str) {
    assert_eq!(
        reply["result"]["content"][0]["text"],
        json!(format!("ran {tool}")),
        "expected the server's own result, got {reply}"
    );
}

const READ_ONLY: (&str, fn() -> Value) = ("read_file", || json!({"path": "README.md"}));
const DESTRUCTIVE: (&str, fn() -> Value) = ("run_command", || json!({"command": "rm -rf /"}));

// ---------------------------------------------------------------------------
// Acceptance: an allowed call is relayed, a refused one gets -32600
// ---------------------------------------------------------------------------

#[test]
fn allowed_call_is_checked_with_the_api_then_relayed() {
    let api = FakeApi::start();
    let mut proxy = Proxy::start(&api.base_url, &[]);
    proxy.initialize();

    let reply = proxy.call(1, "read_file", &json!({"path": "README.md"}));
    assert_relayed(&reply, "read_file");

    let requests = api.requests();
    assert_eq!(requests.len(), 1, "one validate-tool call per tools/call");
    let req = &requests[0];
    assert_eq!(req.path, "/api/v1/agent/validate-tool");
    assert_eq!(req.api_key.as_deref(), Some("pg_test_mcp_proxy_fixture"));
    assert_eq!(req.body["tool_name"], "read_file");
    assert_eq!(req.body["arguments"], json!({"path": "README.md"}));
    assert!(
        req.body["agent_id"]
            .as_str()
            .is_some_and(|a| a.starts_with("mcp-proxy:")),
        "agent_id defaults to the wrapped server: {}",
        req.body
    );
    assert!(req.body["session_id"].is_string());
}

#[test]
fn call_the_api_refuses_gets_invalid_request_and_never_reaches_the_server() {
    let api = FakeApi::start();
    let mut proxy = Proxy::start(&api.base_url, &[]);
    proxy.initialize();

    let reply = proxy.call(2, "forbidden_tool", &json!({}));
    assert_refused(&reply);
    let message = reply["error"]["message"].as_str().unwrap();
    assert!(
        message.contains("blocked for security reasons"),
        "the refusal names the API's reason: {message}"
    );

    // A later call still works, and the refused one never reached the server.
    assert_relayed(&proxy.call(3, "read_file", &json!({})), "read_file");
    assert!(proxy.never_reached_server(2));
    assert!(proxy.server_saw_call(3));
}

#[test]
fn destructive_call_is_refused_by_the_local_check_before_the_api() {
    let api = FakeApi::start();
    let mut proxy = Proxy::start(&api.base_url, &[]);
    proxy.initialize();

    // The fake API would allow this tool name; the local regex refuses it.
    let reply = proxy.call(4, "run_command", &json!({"command": "rm -rf /tmp/x"}));
    assert_refused(&reply);
    assert!(proxy.never_reached_server(4));
    assert!(
        api.requests().is_empty(),
        "a destructive call is refused locally, without a round trip"
    );
}

#[test]
fn malformed_tool_call_is_refused() {
    let api = FakeApi::start();
    let mut proxy = Proxy::start(&api.base_url, &[]);
    proxy.initialize();

    proxy.send(&json!({"jsonrpc": "2.0", "id": 5, "method": "tools/call", "params": {"name": "read_file", "arguments": "rm -rf /"}}));
    assert_refused(&proxy.recv_id(5));
    proxy.send(&json!({"jsonrpc": "2.0", "id": 6, "method": "tools/call", "params": {}}));
    assert_refused(&proxy.recv_id(6));
    assert!(proxy.never_reached_server(5) && proxy.never_reached_server(6));
}

#[test]
fn batch_containing_a_tool_call_is_refused_whole() {
    let api = FakeApi::start();
    let mut proxy = Proxy::start(&api.base_url, &[]);
    proxy.initialize();

    proxy.send(&json!([
        {"jsonrpc": "2.0", "id": 7, "method": "ping"},
        {"jsonrpc": "2.0", "id": 8, "method": "tools/call", "params": {"name": "run_command", "arguments": {"command": "rm -rf /"}}},
    ]));
    let line = proxy.recv_line();
    let replies: Value = serde_json::from_str(&line).unwrap();
    let replies = replies
        .as_array()
        .expect("a batch is answered with a batch");
    assert_eq!(replies.len(), 2);
    for reply in replies {
        assert_refused(reply);
    }
    assert!(proxy.never_reached_server(8));
}

// ---------------------------------------------------------------------------
// Acceptance: fail modes with the API unreachable (ADR-0034)
// ---------------------------------------------------------------------------

#[test]
fn mixed_is_the_default_and_blocks_only_destructive_calls_when_the_api_is_down() {
    for flags in [
        &[][..],
        &["--fail-mode", "mixed"][..],
        &["--fail-mode=mixed"][..],
    ] {
        let mut proxy = Proxy::start(&unreachable_base_url(), flags);
        proxy.initialize();

        let (tool, args) = DESTRUCTIVE;
        assert_refused(&proxy.call(10, tool, &args()));
        assert!(proxy.never_reached_server(10), "{flags:?}");

        let (tool, args) = READ_ONLY;
        assert_relayed(&proxy.call(11, tool, &args()), tool);
        let stderr = proxy.wait_stderr("unchecked");
        assert!(
            stderr.contains("read_file"),
            "{flags:?}: the relayed call is logged as unchecked, by name:\n{stderr}"
        );
    }
}

#[test]
fn open_relays_every_call_unchecked_when_the_api_is_down() {
    let mut proxy = Proxy::start(&unreachable_base_url(), &["--fail-mode", "open"]);
    proxy.initialize();

    let (tool, args) = READ_ONLY;
    assert_relayed(&proxy.call(20, tool, &args()), tool);
    // Fail open as the SDKs do (ADR-0034 option B): even a call the regex
    // matches runs when the check cannot complete.
    let (tool, args) = DESTRUCTIVE;
    assert_relayed(&proxy.call(21, tool, &args()), tool);
    let stderr = proxy.wait_stderr("unchecked");
    assert!(stderr.contains("run_command"), "{stderr}");
}

#[test]
fn open_still_enforces_checks_when_the_api_is_up() {
    let api = FakeApi::start();
    let mut proxy = Proxy::start(&api.base_url, &["--fail-mode", "open"]);
    proxy.initialize();

    assert_refused(&proxy.call(22, "forbidden_tool", &json!({})));
    let (tool, args) = DESTRUCTIVE;
    assert_refused(&proxy.call(23, tool, &args()));
    assert!(proxy.never_reached_server(22) && proxy.never_reached_server(23));
}

#[test]
fn closed_refuses_every_call_when_the_api_is_down() {
    let mut proxy = Proxy::start(&unreachable_base_url(), &["--fail-mode", "closed"]);
    proxy.initialize();

    let (tool, args) = READ_ONLY;
    let reply = proxy.call(30, tool, &args());
    assert_refused(&reply);
    assert!(
        reply["error"]["message"]
            .as_str()
            .unwrap()
            .contains("could not be checked"),
        "{reply}"
    );
    let (tool, args) = DESTRUCTIVE;
    assert_refused(&proxy.call(31, tool, &args()));
    assert!(proxy.never_reached_server(30) && proxy.never_reached_server(31));
}

#[test]
fn closed_relays_allowed_calls_when_the_api_is_up() {
    let api = FakeApi::start();
    let mut proxy = Proxy::start(&api.base_url, &["--fail-mode", "closed"]);
    proxy.initialize();
    let (tool, args) = READ_ONLY;
    assert_relayed(&proxy.call(32, tool, &args()), tool);
}

#[test]
fn unknown_fail_mode_is_rejected() {
    let out = Command::new(env!("CARGO_BIN_EXE_promptguard"))
        .args(["mcp-proxy", "--fail-mode", "sometimes", "--", "true"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("open") && stderr.contains("closed") && stderr.contains("mixed"));
}

// ---------------------------------------------------------------------------
// API errors: only an outage goes to the fail mode
// ---------------------------------------------------------------------------

const MODES: [&str; 3] = ["open", "mixed", "closed"];

#[test]
fn startup_exits_when_the_api_rejects_the_key() {
    for status in [401, 403] {
        let api = FakeApi::with(Behaviour {
            projects: status,
            verdicts: Verdicts::Normal,
        });
        let mut proxy = Proxy::start(&api.base_url, &[]);
        let code = proxy.wait_exit();
        assert_ne!(
            code, 0,
            "{status}: the proxy must not run with a rejected key"
        );
        let stderr = proxy.stderr();
        assert!(
            stderr.contains("rejected the API key") && stderr.contains("promptguard login"),
            "{status}: {stderr}"
        );
        assert!(
            proxy.server_received().is_empty(),
            "{status}: the server was started with a rejected key"
        );
    }
}

#[test]
fn rejected_key_mid_session_refuses_in_every_mode() {
    for status in [401, 403] {
        for mode in MODES {
            let api = FakeApi::verdicts(Verdicts::Status(status));
            let mut proxy = Proxy::start(&api.base_url, &["--fail-mode", mode]);
            proxy.initialize();
            let (tool, args) = READ_ONLY;
            assert_refused(&proxy.call(90, tool, &args()));
            assert!(proxy.never_reached_server(90), "{status} {mode}");
            proxy.wait_stderr("rejected the API key");
        }
    }
}

#[test]
fn malformed_or_refusing_api_answers_are_refused_in_every_mode() {
    for (verdicts, needle) in [
        (Verdicts::Malformed, "malformed"),
        (Verdicts::Status(404), "404"),
        (Verdicts::Status(422), "422"),
    ] {
        for mode in MODES {
            let api = FakeApi::verdicts(verdicts);
            let mut proxy = Proxy::start(&api.base_url, &["--fail-mode", mode]);
            proxy.initialize();
            let (tool, args) = READ_ONLY;
            assert_refused(&proxy.call(91, tool, &args()));
            assert!(proxy.never_reached_server(91), "{verdicts:?} {mode}");
            proxy.wait_stderr(needle);
        }
    }
}

#[test]
fn server_errors_go_to_the_fail_mode() {
    for (verdicts, needle, closed_needle) in [
        (Verdicts::Status(500), "unchecked", "could not be checked"),
        (Verdicts::Status(503), "unchecked", "could not be checked"),
        // The plan limit is not an outage, but blocking every tool when a
        // plan runs out is the outcome ADR-0034 rejects; it follows the fail
        // mode and says so.
        (Verdicts::Status(429), "plan limit", "plan limit"),
    ] {
        let api = FakeApi::verdicts(verdicts);
        let mut open = Proxy::start(&api.base_url, &["--fail-mode", "open"]);
        open.initialize();
        let (tool, args) = READ_ONLY;
        assert_relayed(&open.call(92, tool, &args()), tool);
        open.wait_stderr(needle);

        let mut closed = Proxy::start(&api.base_url, &["--fail-mode", "closed"]);
        closed.initialize();
        assert_refused(&closed.call(93, tool, &args()));
        closed.wait_stderr(closed_needle);
    }
}

// ---------------------------------------------------------------------------
// Acceptance: robustness
// ---------------------------------------------------------------------------

#[test]
fn server_printing_non_json_to_stdout_still_works() {
    let api = FakeApi::start();
    let mut proxy = Proxy::start(&api.base_url, &[]);
    // The fake server's banner is relayed untouched, before anything else.
    assert_eq!(
        proxy.recv_line(),
        "fake-mcp-server: starting up (this line is not JSON)"
    );
    proxy.initialize();
    let (tool, args) = READ_ONLY;
    assert_relayed(&proxy.call(40, tool, &args()), tool);
}

#[test]
fn non_json_client_lines_are_dropped_and_server_stderr_passes_through() {
    let api = FakeApi::start();
    let mut proxy = Proxy::start(&api.base_url, &[]);
    proxy.initialize();
    // A client never legitimately sends a line that is not JSON, and the
    // server may parse it differently from the proxy, so it is not relayed.
    proxy.send_raw("this is not JSON {");
    proxy.sync();
    assert!(
        !proxy.server_received().iter().any(|e| e["event"] == "raw"),
        "a non-JSON client line reached the server"
    );
    proxy.wait_stderr("dropped a client line that is not JSON");
    proxy.wait_stderr("fake-mcp-server: diagnostics go to stderr");
}

/// Lines that `serde_json` rejects, or reads differently, but that the fake
/// server (Python's `json.loads` on universal-newline stdin, as the official
/// Python MCP SDK reads it) accepts as a destructive `tools/call`. Relaying
/// any of them as "not JSON" would run the call unchecked.
fn parse_differential_payloads(id: u64) -> Vec<(&'static str, String)> {
    let call = |extra: &str| {
        format!(
            r#"{{"jsonrpc":"2.0","id":{id},"method":"tools/call","params":{{"name":"run_command","arguments":{{"command":"rm -rf /"{extra}}}}}}}"#
        )
    };
    let deep = format!(",\"pad\":{}{}", "[".repeat(200), "]".repeat(200));
    vec![
        ("nesting past serde_json's limit", call(&deep)),
        ("a lone surrogate", call(r#","x":"\ud800""#)),
        ("a number out of f64 range", call(r#","x":1e999"#)),
        ("NaN", call(r#","x":NaN"#)),
        (
            "a bare \\r that splits one line into three",
            format!(
                "{{\"jsonrpc\":\"2.0\",\"id\":{},\"method\":\"ping\",\"x\":\r{}\r}}",
                id + 1,
                call("")
            ),
        ),
    ]
}

#[test]
fn lines_the_server_parses_differently_never_reach_it_in_any_fail_mode() {
    let mut reached = Vec::new();
    for mode in ["closed", "mixed", "open"] {
        let mut proxy = Proxy::start(&unreachable_base_url(), &["--fail-mode", mode]);
        proxy.initialize();
        let count = parse_differential_payloads(0).len();
        for n in 0..count {
            let id = 300 + 10 * n as u64;
            let (what, payload) = parse_differential_payloads(id).swap_remove(n);
            proxy.send_raw(&payload);
            if !proxy.never_reached_server(id) {
                reached.push(format!("--fail-mode {mode}: {what}"));
            }
        }
    }
    assert!(
        reached.is_empty(),
        "a destructive tools/call reached the server:\n{}",
        reached.join("\n")
    );
}

#[test]
fn relayed_messages_are_what_the_proxy_checked() {
    let api = FakeApi::start();
    let mut proxy = Proxy::start(&api.base_url, &[]);
    proxy.initialize();

    // Duplicate keys: serde_json keeps the last, some parsers the first. The
    // server must receive the message the proxy judged, not the raw bytes.
    proxy.send_raw(
        r#"{"jsonrpc":"2.0","id":80,"method":"tools/call","params":{"name":"run_command","arguments":{"command":"rm -rf /"}},"method":"ping"}"#,
    );
    assert_eq!(proxy.recv_id(80)["result"], json!({}));
    // U+2028 is legal raw inside a JSON string, and a line separator to
    // `str.splitlines`.
    proxy.send_raw("{\"jsonrpc\":\"2.0\",\"id\":81,\"method\":\"ping\",\"x\":\"a\u{2028}b\"}");
    assert_eq!(proxy.recv_id(81)["result"], json!({}));

    let raw_for = |id: u64| {
        proxy
            .server_received()
            .into_iter()
            .find(|e| e["event"] == "message" && e["id"] == json!(id))
            .map(|e| e["raw"].as_str().unwrap().to_string())
            .unwrap()
    };
    let dup = raw_for(80);
    assert!(
        !dup.contains("tools/call"),
        "server saw a duplicate key: {dup}"
    );
    let sep = raw_for(81);
    assert!(
        sep.contains("\\u2028") && !sep.contains('\u{2028}'),
        "U+2028 must reach the server escaped: {sep}"
    );
}

#[test]
fn oversized_client_message_is_dropped_not_relayed() {
    let api = FakeApi::start();
    let mut proxy = Proxy::start(&api.base_url, &[]);
    proxy.initialize();

    // Over 4 MiB: too large to inspect, so it must not reach the server
    // unchecked. Padding a destructive call is the bypass this closes.
    let padding = "x".repeat(4 * 1024 * 1024 + 16);
    proxy.send(&json!({
        "jsonrpc": "2.0", "id": 50, "method": "tools/call",
        "params": {"name": "run_command", "arguments": {"command": "rm -rf /", "pad": padding}},
    }));
    proxy.send(&json!({"jsonrpc": "2.0", "id": 51, "method": "ping"}));
    assert_eq!(proxy.recv_id(51)["result"], json!({}));
    assert!(proxy.never_reached_server(50));
    proxy.wait_stderr("4 MiB");
}

#[test]
fn pending_requests_are_capped() {
    let api = FakeApi::start();
    let mut proxy = Proxy::start(&api.base_url, &[]);
    proxy.initialize();

    // The fake server never answers test/hang, so each stays pending.
    for id in 100..1124u64 {
        proxy.send(&json!({"jsonrpc": "2.0", "id": id, "method": "test/hang"}));
    }
    proxy.send(&json!({"jsonrpc": "2.0", "id": 2000, "method": "test/hang"}));
    let reply = proxy.recv_id(2000);
    assert_refused(&reply);
    assert!(
        reply["error"]["message"]
            .as_str()
            .unwrap()
            .contains("pending"),
        "{reply}"
    );
}

/// MCP's tools/call is a request: one without an id is refused at once,
/// takes no pending slot and no check, so it cannot starve other requests.
#[test]
fn id_less_tool_calls_are_refused_without_taking_a_slot() {
    let api = FakeApi::verdicts(Verdicts::Slow(Duration::from_secs(3)));
    let mut proxy = Proxy::start(&api.base_url, &[]);
    proxy.initialize();

    for id in 100..1123u64 {
        proxy.send(&json!({"jsonrpc": "2.0", "id": id, "method": "test/hang"}));
    }
    for _ in 0..50 {
        proxy.send(&json!({
            "jsonrpc": "2.0", "method": "tools/call",
            "params": {"name": "read_file", "arguments": {"marker": "id-less-call"}},
        }));
    }
    let reply = proxy.recv_matching(|v| v.get("error").is_some());
    assert_refused(&reply);
    assert_eq!(reply["id"], Value::Null, "{reply}");

    // 1023 slots are taken; the ping gets the last one and is answered.
    proxy.send(&json!({"jsonrpc": "2.0", "id": 2001, "method": "ping"}));
    assert_eq!(proxy.recv_id(2001)["result"], json!({}));
    assert!(api.requests().is_empty(), "an id-less call was checked");
    assert!(!proxy.server_raw_contains("id-less-call"));
}

#[test]
fn numeric_ids_are_normalised_and_an_outstanding_id_is_refused() {
    let api = FakeApi::start();
    let mut proxy = Proxy::start(&api.base_url, &[]);
    proxy.initialize();
    proxy.send(&json!({"jsonrpc": "2.0", "id": 70, "method": "test/hang"}));

    let refused_with_code = |v: &Value| v["error"]["code"] == json!(-32600);
    for (id, marker) in [("70", "same-id"), ("70.0", "float-id"), ("7e1", "exp-id")] {
        proxy.send_raw(&format!(
            r#"{{"jsonrpc":"2.0","id":{id},"method":"tools/call","params":{{"name":"read_file","arguments":{{"marker":"{marker}"}}}}}}"#
        ));
        let reply = proxy.recv_matching(refused_with_code);
        assert!(
            reply["error"]["message"]
                .as_str()
                .unwrap()
                .contains("already pending"),
            "id {id}: {reply}"
        );
        assert!(
            !proxy.server_raw_contains(marker),
            "id {id} reached the server"
        );
    }

    // A string id is a different id.
    proxy.send(&json!({"jsonrpc": "2.0", "id": "70", "method": "tools/call", "params": {"name": "read_file", "arguments": {}}}));
    let reply = proxy.recv_matching(|v| v["id"] == json!("70"));
    assert_relayed(&reply, "read_file");
}

#[test]
fn a_batch_holding_an_item_that_is_not_an_object_is_refused_whole() {
    let api = FakeApi::start();
    let mut proxy = Proxy::start(&api.base_url, &[]);
    proxy.initialize();

    proxy.send(&json!([
        {"jsonrpc": "2.0", "id": 77, "method": "ping", "params": {"marker": "non-object-batch"}},
        [{"jsonrpc": "2.0", "id": 78, "method": "tools/call", "params": {"name": "forbidden_tool", "arguments": {}}}],
        "tools/call",
    ]));
    let replies: Value = serde_json::from_str(&proxy.recv_line()).unwrap();
    let replies = replies.as_array().expect("a batch is answered with a batch");
    assert_eq!(replies.len(), 1, "{replies:?}");
    assert_refused(&replies[0]);
    assert_eq!(replies[0]["id"], json!(77));
    assert!(!proxy.server_raw_contains("non-object-batch"));
    assert!(proxy.never_reached_server(78));
}

/// The pending set is consulted in constant work per message: a batch far
/// over the cap is refused at once instead of being scanned for duplicates
/// while every other message waits on the lock.
#[test]
fn a_huge_batch_is_refused_without_stalling_the_proxy() {
    let api = FakeApi::start();
    let mut proxy = Proxy::start(&api.base_url, &[]);
    proxy.initialize();

    let batch: Vec<Value> = (0..60_000u64)
        .map(|i| json!({"jsonrpc": "2.0", "id": 100_000 + i, "method": "ping"}))
        .collect();
    let started = std::time::Instant::now();
    proxy.send(&Value::Array(batch));
    proxy.send(&json!({"jsonrpc": "2.0", "id": 3000, "method": "ping"}));
    let replies: Value = serde_json::from_str(&proxy.recv_line()).unwrap();
    assert_eq!(replies.as_array().map(Vec::len), Some(60_000));
    assert_eq!(proxy.recv_id(3000)["result"], json!({}));
    let elapsed = started.elapsed();
    assert!(
        elapsed < Duration::from_secs(3),
        "the batch held the proxy up for {elapsed:?}"
    );
}

#[test]
fn a_batch_reusing_an_id_is_refused() {
    let api = FakeApi::start();
    let mut proxy = Proxy::start(&api.base_url, &[]);
    proxy.initialize();
    proxy.send(&json!({"jsonrpc": "2.0", "id": 75, "method": "test/hang"}));

    for batch in [
        json!([
            {"jsonrpc": "2.0", "id": 76, "method": "ping", "params": {"marker": "batch-dup"}},
            {"jsonrpc": "2.0", "id": 76, "method": "ping"},
        ]),
        json!([{"jsonrpc": "2.0", "id": 75.0, "method": "ping", "params": {"marker": "batch-outstanding"}}]),
    ] {
        proxy.send(&batch);
        let line = proxy.recv_line();
        let replies: Value = serde_json::from_str(&line).unwrap();
        for reply in replies
            .as_array()
            .expect("a batch is answered with a batch")
        {
            assert_refused(reply);
        }
    }
    assert!(!proxy.server_raw_contains("batch-dup"));
    assert!(!proxy.server_raw_contains("batch-outstanding"));
}

#[test]
fn server_does_not_inherit_the_api_key_and_exit_code_propagates() {
    let api = FakeApi::start();
    let mut proxy = Proxy::start(&api.base_url, &[]);
    proxy.initialize();
    let start = proxy
        .server_received()
        .into_iter()
        .find(|e| e["event"] == "start")
        .unwrap();
    assert_eq!(
        start["inherited_api_key"], false,
        "a third-party MCP server must not receive the PromptGuard API key"
    );

    proxy.send(&json!({"jsonrpc": "2.0", "id": 60, "method": "test/exit", "params": {"code": 7}}));
    assert_eq!(proxy.close_and_wait(), 7);
}

#[test]
fn proxy_exits_when_the_client_closes_stdin() {
    let api = FakeApi::start();
    let mut proxy = Proxy::start(&api.base_url, &[]);
    proxy.initialize();
    assert_eq!(proxy.close_and_wait(), 0);
}

#[test]
fn logs_name_the_server_program_but_never_its_arguments() {
    let api = FakeApi::start();
    let mut proxy =
        Proxy::start_with_server_args(&api.base_url, &[], &["--token", "sekret-token-value"]);
    proxy.initialize();
    let (tool, args) = READ_ONLY;
    assert_relayed(&proxy.call(95, tool, &args()), tool);
    let stderr = proxy.wait_stderr("checking tool calls for");
    assert!(stderr.contains("python3"), "{stderr}");
    assert!(
        !stderr.contains("sekret-token-value"),
        "server arguments can carry tokens and must not be logged:\n{stderr}"
    );
}
