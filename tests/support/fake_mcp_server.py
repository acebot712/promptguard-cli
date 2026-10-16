#!/usr/bin/env python3
"""A minimal stdio MCP server that tests/mcp_proxy_tests.rs runs behind
`promptguard mcp-proxy`.

It misbehaves on purpose in the ways real servers do: it prints a non-JSON
banner to stdout before speaking JSON-RPC, and writes diagnostics to stderr.
Every line it receives is appended to the file named by FAKE_MCP_LOG, so a test
can prove that a refused tool call never reached the server.

Methods:
  initialize, tools/list, ping   answered normally
  tools/call                     answered with "ran <tool name>"
  test/hang                      never answered (fills the proxy's pending set)
  test/exit                      exits with params["code"]
"""

import json
import os
import sys

LOG_PATH = os.environ.get("FAKE_MCP_LOG")


def log(entry):
    if LOG_PATH:
        with open(LOG_PATH, "a", encoding="utf-8") as f:
            f.write(json.dumps(entry) + "\n")


def send(message):
    sys.stdout.write(json.dumps(message) + "\n")
    sys.stdout.flush()


def reply(request_id, result):
    send({"jsonrpc": "2.0", "id": request_id, "result": result})


log({"event": "start", "inherited_api_key": "PROMPTGUARD_API_KEY" in os.environ})
sys.stdout.write("fake-mcp-server: starting up (this line is not JSON)\n")
sys.stdout.flush()
sys.stderr.write("fake-mcp-server: diagnostics go to stderr\n")
sys.stderr.flush()

while True:
    raw = sys.stdin.readline()
    if raw == "":
        break
    line = raw.rstrip("\n")
    try:
        message = json.loads(line)
    except ValueError:
        log({"event": "raw", "line": line})
        continue
    log({"event": "message", "message": message})
    if not isinstance(message, dict):
        continue

    method = message.get("method")
    request_id = message.get("id")
    params = message.get("params") or {}
    if request_id is None:
        continue

    if method == "initialize":
        reply(
            request_id,
            {
                "protocolVersion": "2025-06-18",
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "fake-mcp-server", "version": "0.0.0"},
            },
        )
    elif method == "tools/list":
        reply(
            request_id,
            {
                "tools": [
                    {"name": "read_file", "description": "Read a file", "inputSchema": {"type": "object"}},
                    {"name": "run_command", "description": "Run a shell command", "inputSchema": {"type": "object"}},
                ]
            },
        )
    elif method == "tools/call":
        reply(request_id, {"content": [{"type": "text", "text": "ran " + str(params.get("name"))}]})
    elif method == "ping":
        reply(request_id, {})
    elif method == "test/hang":
        pass
    elif method == "test/exit":
        sys.exit(int(params.get("code", 0)))
    else:
        send({"jsonrpc": "2.0", "id": request_id, "error": {"code": -32601, "message": "Method not found"}})
