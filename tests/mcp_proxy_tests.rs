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

struct FakeApi {
    base_url: String,
    requests: Arc<Mutex<Vec<ApiRequest>>>,
}

impl FakeApi {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base_url = format!("http://{}/api/v1", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&requests);
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let recorded = Arc::clone(&recorded);
                thread::spawn(move || serve_one(stream, &recorded));
            }
        });
        Self { base_url, requests }
    }

    fn requests(&self) -> Vec<ApiRequest> {
        self.requests.lock().unwrap().clone()
    }
}

fn serve_one(stream: TcpStream, recorded: &Mutex<Vec<ApiRequest>>) {
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
        path,
        api_key,
        body,
    });

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
    let payload = verdict.to_string();
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
        payload.len()
    );
    let mut stream = stream;
    let _ = stream.write_all(response.as_bytes());
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
        let home = TempDir::new().unwrap();
        let server_log = home.path().join("server.log");
        let mut child = Command::new(env!("CARGO_BIN_EXE_promptguard"))
            .arg("mcp-proxy")
            .args(flags)
            .arg("--")
            .arg("python3")
            .arg(fake_server_script())
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
        self.server_received().iter().any(|e| {
            e["event"] == "message"
                && e["message"]["method"] == "tools/call"
                && e["message"]["id"] == json!(id)
        })
    }

    fn initialize(&mut self) {
        self.send(&json!({
            "jsonrpc": "2.0", "id": 0, "method": "initialize",
            "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "test", "version": "0"}},
        }));
        let reply = self.recv_id(0);
        assert_eq!(reply["result"]["serverInfo"]["name"], "fake-mcp-server");
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
fn non_json_client_lines_and_server_stderr_pass_through_untouched() {
    let api = FakeApi::start();
    let mut proxy = Proxy::start(&api.base_url, &[]);
    proxy.initialize();
    proxy.send_raw("this is not JSON {");
    proxy.send(&json!({"jsonrpc": "2.0", "id": 41, "method": "ping"}));
    assert_eq!(proxy.recv_id(41)["result"], json!({}));

    let raw: Vec<String> = proxy
        .server_received()
        .iter()
        .filter(|e| e["event"] == "raw")
        .map(|e| e["line"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(raw, vec!["this is not JSON {".to_string()]);
    proxy.wait_stderr("fake-mcp-server: diagnostics go to stderr");
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
