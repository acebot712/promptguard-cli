#!/usr/bin/env python3
"""A minimal stdio MCP server that tests/mcp_proxy_tests.rs runs behind
`promptguard mcp-proxy`.

It misbehaves on purpose in the ways real servers do: it prints a non-JSON
banner to stdout before speaking JSON-RPC, and writes diagnostics to stderr.
Every line it receives is appended to the file named by FAKE_MCP_LOG, so a test
can prove that a refused tool call never reached the server.

It reads stdin the way the official Python MCP SDK does (a TextIOWrapper with
universal newlines, so a bare \r ends a line) and parses with `json.loads`,
which accepts NaN, 1e999, lone surrogates and nesting far deeper than
serde_json does. Those differences are what the proxy must not let a tool
call hide behind.

Methods:
  initialize, ping               answered normally
  tools/list                     the tools in the JSON file FAKE_MCP_TOOLS names,
                                 or two default tools. With FAKE_MCP_PAGE_SIZE=n
                                 the list is paginated: the first page is empty
                                 (`tools: []` plus a cursor), and each cursor is
                                 the offset of the next n tools. With
                                 FAKE_MCP_CURSOR_LOOP=1 every page is empty and
                                 hands back the same cursor.
  tools/call                     answered with "ran <tool name>"; the tool
                                 `echo` answers with its `text` argument
                                 (`repeat` times), as a text block, an image
                                 block and structuredContent
  tools/call `fail`              answered with a JSON-RPC error whose message
                                 and data carry params.arguments.text
  tools/call `link`              answered with a resource_link block whose name
                                 and description carry params.arguments.text
  test/flood                     writes params["count"] results for ids nobody
                                 asked for (each a text block holding an email),
                                 then answers {}
  test/huge                      answered with a result over 4 MiB; with
                                 params["id_last"] the id is written after it
  test/hang                      never answered (fills the proxy's pending set)
  test/exit                      exits with params["code"]
"""

import io
import json
import os
import sys

LOG_PATH = os.environ.get("FAKE_MCP_LOG")
TOOLS_PATH = os.environ.get("FAKE_MCP_TOOLS")
PAGE_SIZE = int(os.environ.get("FAKE_MCP_PAGE_SIZE", "0"))
CURSOR_LOOP = os.environ.get("FAKE_MCP_CURSOR_LOOP") == "1"

DEFAULT_TOOLS = [
    {"name": "read_file", "description": "Read a file", "inputSchema": {"type": "object"}},
    {"name": "run_command", "description": "Run a shell command", "inputSchema": {"type": "object"}},
]


def tools():
    if TOOLS_PATH:
        with open(TOOLS_PATH, encoding="utf-8") as f:
            return json.load(f)
    return DEFAULT_TOOLS


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

stdin = io.TextIOWrapper(sys.stdin.buffer, encoding="utf-8", errors="replace")
while True:
    raw = stdin.readline()
    if raw == "":
        break
    line = raw.rstrip("\n")
    try:
        message = json.loads(line)
    except ValueError:
        log({"event": "raw", "line": line})
        continue
    if not isinstance(message, dict):
        log({"event": "message", "raw": line})
        continue
    # Only plain fields: the parsed message may hold NaN, infinities or lone
    # surrogates, which the Rust side of the test could not read back.
    log({"event": "message", "id": message.get("id"), "method": message.get("method"), "raw": line})

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
    elif method == "tools/list" and CURSOR_LOOP:
        reply(request_id, {"tools": [], "nextCursor": "loop"})
    elif method == "tools/list" and PAGE_SIZE:
        cursor = params.get("cursor")
        if cursor is None:
            reply(request_id, {"tools": [], "nextCursor": "0"})
        else:
            start = int(cursor)
            page = {"tools": tools()[start : start + PAGE_SIZE]}
            if start + PAGE_SIZE < len(tools()):
                page["nextCursor"] = str(start + PAGE_SIZE)
            reply(request_id, page)
    elif method == "tools/list":
        reply(request_id, {"tools": tools()})
    elif method == "tools/call" and params.get("name") in ("fail", "link"):
        text = str((params.get("arguments") or {}).get("text", ""))
        if params.get("name") == "fail":
            send(
                {
                    "jsonrpc": "2.0",
                    "id": request_id,
                    "error": {"code": -32000, "message": "failed: " + text, "data": {"detail": text, "n": 1}},
                }
            )
        else:
            link = {"type": "resource_link", "uri": "file:///notes.txt", "name": text, "description": text}
            reply(request_id, {"content": [link]})
    elif method == "test/flood":
        for i in range(int(params.get("count", 0))):
            reply("flood-%d" % i, {"content": [{"type": "text", "text": "flood %d jane@example.com" % i}]})
        reply(request_id, {})
    elif method == "test/huge":
        result = json.dumps({"content": [{"type": "text", "text": "jane@example.com " * 300_000}]})
        if params.get("id_last"):
            sys.stdout.write('{"jsonrpc":"2.0","result":%s,"id":%s}\n' % (result, json.dumps(request_id)))
            sys.stdout.flush()
        else:
            reply(request_id, json.loads(result))
    elif method == "tools/call" and params.get("name") == "echo":
        arguments = params.get("arguments") or {}
        text = str(arguments.get("text", "")) * int(arguments.get("repeat", 1))
        reply(
            request_id,
            {
                "content": [
                    {"type": "text", "text": text},
                    {"type": "image", "data": "aGVsbG8=", "mimeType": "image/png"},
                ],
                "structuredContent": {"echo": text, "length": len(text)},
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
