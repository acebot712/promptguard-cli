[![CI](https://github.com/acebot712/promptguard-cli/actions/workflows/ci.yml/badge.svg)](https://github.com/acebot712/promptguard-cli/actions/workflows/ci.yml)
[![License](https://img.shields.io/github/license/acebot712/promptguard-cli)](https://github.com/acebot712/promptguard-cli/blob/main/LICENSE)
[![Rust](https://img.shields.io/badge/Built_with-Rust-orange)](https://www.rust-lang.org/)

# PromptGuard CLI

> Drop-in LLM security for your applications — built with Rust + Tree-sitter

![Rust](https://img.shields.io/badge/rust-stable-orange?logo=rust)
![License](https://img.shields.io/badge/license-Apache%202.0-green)

Single 5.3MB binary. No runtime interpreter required. Real AST transformations. Under 10ms startup.

## Installation

| Method | Command |
|--------|---------|
| **Shell script** (macOS, Linux x86_64) | `curl -fsSL https://raw.githubusercontent.com/acebot712/promptguard-cli/main/install.sh \| sh` |
| **Binary** | Download from [GitHub Releases](https://github.com/acebot712/promptguard-cli/releases/latest) |
| **Cargo** (build from source) | `cargo install --locked --git https://github.com/acebot712/promptguard-cli --tag cli-v2.1.2` |

Every release publishes these prebuilt binaries, each with a `.sha256` checksum
beside it (the shell script verifies it before installing):

| Platform | Release asset |
|----------|---------------|
| macOS ARM64 (Apple Silicon) | `promptguard-macos-arm64` |
| macOS x86_64 (Intel) | `promptguard-macos-x86_64` |
| Linux x86_64 | `promptguard-linux-x86_64` |
| Windows x86_64 | `promptguard-windows-x86_64.exe` |

Any other platform, Linux ARM64 included, builds from source with the Cargo
command, which needs a current stable Rust toolchain. It builds the release tag
from this repository because the crate is not published to crates.io.

> For an MCP-only install (no Rust required), use `pip install promptguard-mcp-server` instead.

## Quick Start

```bash
promptguard login                                  # Authenticate (works anywhere)
promptguard scan --text "ignore previous instructions"  # Instant threat scan
promptguard scan --offline --file prompt.txt       # Local subset, no key or network (CI)
promptguard init --api-key pg_live_xxx             # Integrate a project
promptguard verify                                 # Test connectivity + scan + redact
promptguard status                                 # Check configuration
promptguard mcp -t stdio                           # Start MCP server
```

## Commands

| Command | Description |
|---------|-------------|
| `init` | Initialize PromptGuard and rewrite SDK constructors to route through proxy |
| `scan` | Scan project for LLM SDK usage, or scan text/files for security threats (`--offline` for a local subset with no key or network) |
| `status` | Show current configuration and managed files |
| `doctor` | Diagnose common issues |
| `apply` | Apply pending code transformations |
| `disable` / `enable` | Toggle PromptGuard on/off |
| `revert` | Revert all changes (restores backups) |
| `mcp` | Start MCP server for AI editor integration |
| `mcp-proxy` | Wrap any stdio MCP server and check every tool call before it runs |
| `redteam` | Run the adversarial corpus against your policy (`--test`, `--prompt`, or all) |
| `verify` | End-to-end integration check: connectivity, auth, scan, and redact |
| `policy` | Policy-as-code: `apply`, `diff`, `export` YAML guardrail configs |
| `login` / `logout` | Authenticate with PromptGuard cloud |
| `whoami` | Show current authenticated user |
| `projects` | `list` / `select` active project |
| `config` | View or update local configuration |
| `key` | Manage API keys |
| `dashboard` | Open the PromptGuard dashboard in browser |
| `redact` | Redact PII from text |
| `test` | Run a quick connectivity and detection test |
| `update` | Update the CLI to latest version |

## Offline Scan for CI

`promptguard scan --offline` checks `--text` or `--file` content on the machine
it runs on, against a regex rule pack compiled into the binary. It needs no API
key and opens no network connection, so it can gate a CI job that has neither.

```bash
promptguard scan --offline --file prompts/system.txt              # your own prompt
promptguard scan --offline --untrusted --file fixtures/page.html  # content you did not write
promptguard scan --offline --file prompts/system.txt --json       # machine-readable
```

**It is a local subset, not the full detection the PromptGuard API runs**, and
every result says so. The pack covers prompt injection, jailbreak phrasings and
provider credentials. It has none of the API's ML classifiers and does not apply
your project's policy, so a clean offline result does not mean the API would
allow the content. Run `promptguard scan --file …` with a key for the real verdict.

Each match is scored as the upstream rule set scores it, severity times
confidence, against per-detector thresholds. A **block** finding fails the scan;
**warn** findings are reported and do not; a credential is a **mask** finding
(the gateway would redact it in flight), reported without failing, and quoted by
its first four characters only. Matches below every threshold appear only in
`--json`. Ordinary system-prompt lines ("Never reveal your system prompt",
"Summarize the context below") pass; a file that quotes an attack string such as
"ignore previous instructions" blocks, because it contains one.

`--untrusted` adds the indirect-injection rules, for content from outside your
control: tool output, retrieved documents, web pages. They look for text that
addresses or instructs the model ("when you read this", "silently", "send the
API key to"), which is suspicious in a fetched document and normal in your own
prompt, so they are off unless you ask for them.

Exit codes are the same as the API scan's: `0` when nothing blocks, `2` when a
finding does, `1` on an error, including a file over 10 MiB, which is refused
rather than scanned. In `--json` output, `mode` is `"offline"`, `notice`
carries the subset statement, and each finding names its `rule`, `severity`,
`score`, `effect` and `line`.

The pack is generated, never edited by hand: `scripts/local-rules/regenerate.sh`
dumps it from an upstream set of Go detectors, together with the scoring
pipeline they run under there. The `provenance` block in
`src/local_rules/pack.json` names that source, its files' hashes, which
detectors and rules were left out or demoted to warn, and why.
`tests/local_rules_tests.rs` checks that the Go and Rust regex engines agree on
each rule, and that the port reaches the upstream engine's own verdict on its
sample corpus.

## MCP Server

The CLI includes a native [Model Context Protocol](https://modelcontextprotocol.io) server:

```bash
promptguard mcp -t stdio
```

### Supported Transports

| Transport | Command | Notes |
|-----------|---------|-------|
| **stdio** | `promptguard mcp -t stdio` | Default, used by all editors |

For Streamable HTTP transport, use the [standalone Python server](https://github.com/acebot712/promptguard/tree/main/mcp-server) (`pip install promptguard-mcp-server`).

### Available Tools

| Tool | Parameters | Description |
|------|------------|-------------|
| `promptguard_auth` | `api_key` (optional) | Save/validate API key |
| `promptguard_logout` | — | Clear local credentials |
| `promptguard_scan_text` | `text` (required) | Scan text for security threats |
| `promptguard_scan_project` | `directory`, `provider` (optional) | Scan codebase for unprotected LLM SDK usage |
| `promptguard_redact` | `text` (required) | Redact PII from text |
| `promptguard_status` | — | Check config and API connectivity |

### Client Configuration

**Cursor** (`.cursor/mcp.json`):

```json
{
  "mcpServers": {
    "promptguard": {
      "command": "promptguard",
      "args": ["mcp", "-t", "stdio"]
    }
  }
}
```

**Claude Code:**

```bash
claude mcp add promptguard -- promptguard mcp -t stdio
```

**Gemini CLI:**

```bash
gemini mcp add -t stdio promptguard -- promptguard mcp -t stdio
```

Full setup instructions for 13+ clients: [docs.promptguard.co/tools/mcp](https://docs.promptguard.co/tools/mcp)

## MCP Proxy

`promptguard mcp-proxy` sits between your agent and an MCP server you already
run. It checks every tool call before the server sees it, redacts every tool
result before your agent sees it, and scans every tool list for poisoned
descriptions and for changes since it was pinned:

```bash
promptguard mcp-proxy -- npx -y @modelcontextprotocol/server-filesystem .
```

Put it in front of the server's command wherever your client launches it:

```json
{
  "mcpServers": {
    "filesystem": {
      "command": "promptguard",
      "args": ["mcp-proxy", "--", "npx", "-y", "@modelcontextprotocol/server-filesystem", "."]
    }
  }
}
```

```bash
claude mcp add filesystem -- promptguard mcp-proxy -- npx -y @modelcontextprotocol/server-filesystem .
```

It needs an API key (`promptguard login` or `PROMPTGUARD_API_KEY`), and does
not pass `PROMPTGUARD_API_KEY` on to the server it wraps. The key is verified
once at startup: if the API rejects it, the proxy exits with an error before
starting the server. If the API cannot be reached then, the proxy starts anyway
and its fail mode applies.

### What it checks

Each `tools/call` is checked twice, in order:

1. **Locally**, by a destructive-action check on the call's arguments:
   recursive forced removal (`rm -rf`, `rm -rfv`, `rm -r -f`,
   `rm --recursive --force` or `--rec --forc`, in any order), `find ... -delete`
   and `find ... -exec rm`, PowerShell's `Remove-Item -Recurse -Force`,
   `del /s`, `rmdir /s`; `git clean -f`, `git reset --hard` and a forced
   `git push`; `DROP TABLE|DATABASE|SCHEMA|INDEX`, `ALTER TABLE ... DROP` and
   `TRUNCATE` (comments between the keywords included), `DELETE FROM <table>`
   with no `WHERE`; `mkfs`, `wipefs`, and `dd`, `shred` or a redirect onto a
   disk; a recursive `chmod`/`chown` of `/`; the fork bomb; a download piped
   into a shell, `sudo`, `python`, `perl`, `ruby` or `node`, base64-decoded text
   piped into a shell or `sudo`, and a download run through
   `sh -c "$(curl ...)"` or `bash <(curl ...)`; `/etc/shadow`; and `../../`
   path climbing. Each string is
   checked after NFKC normalisation with zero-width and other format characters
   removed, so fullwidth `ｒｍ` or a zero-width space inside `rm` is still `rm`.
2. **By the PromptGuard API**, through `POST /api/v1/agent/validate-tool`, which
   applies your project's agent policies and behaviour analysis. Calls are
   reported under the agent ID `mcp-proxy:<server name>`; set your own with
   `--agent-id`.

A refused call never reaches the server. The agent gets a JSON-RPC error with
code `-32600` and a message naming the reason, and the proxy logs it to stderr.

### When the API is unreachable

`--fail-mode` decides what happens to a call the API cannot give a verdict on
because it is unreachable, timed out after 10 seconds, or returned a 5xx. A
`429` (your plan's limit) is treated the same way and logged as a plan limit:

| Fail mode | Call the local check matches | Any other call |
|-----------|------------------------------|----------------|
| `mixed` (default) | Refused | Relayed, logged as unchecked |
| `open` | Relayed, logged as unchecked | Relayed, logged as unchecked |
| `closed` | Refused | Refused |

While the API is reachable, a call the local check matches is refused in every
mode. `mixed` is the default because an outage should neither break your agent
nor let through the actions the proxy exists to stop. Each unchecked call is
logged to stderr with the word `unchecked`, the tool's name and why the API
gave no verdict; arguments are never logged.

Any other answer that is not a verdict refuses the call in every mode: a
rejected key (`401`/`403`), another `4xx`, or a response that is not a
verdict. A revoked key must not turn the proxy into a pass-through.

### Tool results are redacted

The text of each `tools/call` result is sent to the PromptGuard API's
[guard endpoint](https://docs.promptguard.co/api-reference/guard) as output
(`direction: "output"`), and whatever your project's guardrails mask, such as
email addresses, card numbers or API keys, is masked in the result your agent
receives. Only text is sent and only text changes: each text block's `text`,
an embedded resource's `text`, a resource link's `name`, `title` and
`description` (not its `uri`), and the string values of `structuredContent`.
A call the server answers with a JSON-RPC error has the error's `message` and
every string in its `data` redacted the same way. The JSON-RPC envelope, the
order of the blocks and every other block (an image, audio) arrive as the
server sent them, and a result with nothing to mask arrives byte for byte.
Each result costs one scan against your plan; a result with more than 90,000
characters of text is sent in several, at most 16 (about 1.4 million
characters). A result with more text than that has its first 16 requests'
worth redacted, so a block in them still applies, and the rest is treated as
text the API could not redact. Each request may take 10 seconds, and a result
20 seconds in all; text not redacted by then is treated the same way.

If the guard endpoint **blocks** a result, your agent gets a JSON-RPC error
(`-32600`) in its place, in every fail mode. If the result cannot be redacted:

| Fail mode | Result the API could not redact |
|-----------|---------------------------------|
| `mixed` (default) | Relayed as the server sent it (with any text already redacted masked), logged as `unredacted` |
| `open` | Relayed as the server sent it (with any text already redacted masked), logged as `unredacted` |
| `closed` | Withheld: your agent gets a `-32600` error |

"Could not redact" is the same as "could not check" for a tool call (no
answer, a timeout, a 5xx, a `429`), plus a result larger than the API takes
(over 16 requests, or a `413`/`422`) or not redacted within 20 seconds.
`mixed` relays such a result because its line is drawn between destructive
and other *actions*: a result reports an action that has already run, and
withholding it would undo nothing. A rejected key, another `4xx`, or an
answer that is not a verdict withholds the result in every mode.

**Not redacted:** only tool results are. `resources/read` and `prompts/get`
results pass through unredacted, as do the server's log and progress
notifications (`notifications/message`, `notifications/progress`), which are
the server's own messages rather than a tool's output.

### Tool lists are scanned and pinned

Each `tools/list` result is sent to
[`POST /api/v1/mcp/manifest-scan`](https://docs.promptguard.co/api-reference/mcp-manifest-scan)
before your agent sees it. A list the server splits into pages is collected
first: the proxy asks the server for every later page itself, scans and pins
the whole list, and gives your agent the whole list in one page, with no
cursor. (So a `tools/list` from your agent that carries a cursor is refused,
and at most 8 may be pending at once.) A list whose pages repeat a cursor,
stop arriving for 10 seconds, or run past 100 pages or 10,000 tools is
refused in every mode. Every finding is logged to stderr with its code,
severity, tool and reason:

- **A `high` finding on a tool** (`E001` instructions hidden in a description
  or a parameter's description, `E002` shadowing another tool): the tool is
  removed from the list your agent receives, in every fail mode, and a
  `tools/call` to it is refused.
- **`E301`, a rug-pull:** the list no longer matches the pin stored for this
  server. The finding names the server and the tools that changed, were added
  or were removed. Under `closed`, the changed and added tools are withheld
  from your agent; under `mixed` and `open` the list is relayed, because a
  server you upgraded changes its tools too. The old pin is kept, so every run
  reports the change until you accept it. Once you have checked the change,
  run the server once with `--repin`:

  ```bash
  promptguard mcp-proxy --repin -- npx -y @modelcontextprotocol/server-filesystem .
  ```

  `--repin` replaces this server's pin with its current list on the first
  complete scan of that run, leaves every other server's pin alone, and says
  so on stderr. Removing that server's entry from
  `~/.promptguard/mcp-pins.json`, or deleting the file to re-pin every server,
  does the same.
- **Anything else** (`W021` invisible characters, `W022` duplicate names,
  `E003` community rules, `E900`) is logged only.

A server cannot get a tool past the scan by sending what the scan cannot
read. A tool that breaks the shape the endpoint takes (a name that is not
text, empty or over 256 characters, a description that is not text, an
`inputSchema` that is not an object), a tool too large to scan on its own,
one the endpoint refuses with a `413` or `422`, one a scan does not reach
twice, and the tools past 16 scan requests are all withheld, in every fail
mode. A list over the endpoint's limits (500 tools or 131,072 characters) is
split into several scans, each counted against your plan.

If the list cannot be scanned because the API gave no answer (the same
failures as a result: no answer, a timeout, a 5xx, a `429`), `closed` refuses
it with a `-32600` error, and `mixed` and `open` relay the tools not yet
scanned, logged as `unchecked`. A rejected key, another `4xx` or an answer
that is not a scan refuses the list in every mode.

**Pins** live in `~/.promptguard/mcp-pins.json`, readable by you only. The
first complete scan of a server stores its pin; later scans send it back. A
server is identified by a SHA-256 hash of its command line and the directory
it runs in, so the same command in two projects is pinned twice, and the file
holds neither the path nor the arguments. Each entry does keep a readable
label for the server: `mcp-proxy:<program>` and, for a package runner (`npx`,
`pnpx`, `bunx`, `uvx`), the package name, skipping flags' values (`--from`,
`--with`, `--registry` ...) and with any user, password, token, query or
fragment removed from a URL. No other argument is stored, and nothing from the
environment. The file is written atomically and under a lock, so several
proxies can run at once. A missing file is created. A corrupt one is moved
aside as `mcp-pins.json.corrupt-<timestamp>` and every server is pinned
again, and the log says that all pins were reset. A file written by a newer
promptguard (`version` above 1), or one that cannot be read, is left
untouched, and the server runs unpinned.

### What is relayed, and how

- **From the server to the agent:** everything, byte for byte, including lines
  that are not JSON (so a server that prints a banner to stdout still works),
  except a tool result or tool list the proxy changed, which is re-serialized,
  a message over 4 MiB, and the answers to the proxy's own requests for later
  pages of a tool list.
  The server's stderr is passed through too.
- **From the agent to the server:** each message as the proxy parsed it, then
  re-serialized on one line, so the server receives exactly what was checked.
  The agent's original bytes are never relayed: a server's parser can read
  them differently (nesting deeper than the proxy reads, `NaN`, `1e999`, lone
  surrogates, duplicate keys) and see a tool call the proxy did not. For the
  same reason a line that is not valid JSON, or that contains a bare carriage
  return (which a universal-newline reader, like the Python MCP SDK's, treats
  as a line break), is dropped and logged to stderr. A well-behaved agent sends
  neither.

Messages are bounded at 4 MiB. A larger one from the agent is dropped (it is
too large to check, so relaying it would be a way around the check). A larger
one from the server is dropped in every fail mode, since it is too large to
redact or scan; the proxy reads the ids in it as it skips it, and answers each
request it was answering with a `-32600` error. At most 1024 requests may be
pending at once; one past that is refused with `-32600`. So is a request whose
id is already pending (`1`, `1.0` and `1e0` are the same id), and a
`tools/call` without an id, which MCP does not allow (it is answered with a
null id). A JSON-RPC batch that contains a `tools/call`, or anything but
JSON-RPC objects, is refused whole.

At most 64 tool results and tool lists are redacted or scanned at once. While
that many are, the proxy stops reading the server, which slows the server
down; nothing is dropped.

Tool calls are checked concurrently, so other messages can overtake one that
is still being checked. A `notifications/cancelled` for such a call may reach
the server before the call does; the server ignores it, and runs the call.
In the same way, other server messages can overtake a tool result being
redacted or a tool list being scanned.

The proxy exits when the server does, with the server's exit code.

## How the CLI Works

The CLI rewrites SDK constructors using Tree-sitter AST parsing (not regex):

**Before:**
```typescript
const openai = new OpenAI({
  apiKey: process.env.OPENAI_API_KEY
});
```

**After `promptguard init`:**
```typescript
const openai = new OpenAI({
  apiKey: process.env.PROMPTGUARD_API_KEY,
  baseURL: "https://api.promptguard.co/api/v1"
});
```

All LLM requests now flow through PromptGuard's six-layer security pipeline.

### Supported Providers

"Static transform mode" rewrites the SDK base URL in your source (works for every
provider/language below). "Runtime shim mode" (`promptguard enable --runtime`)
intercepts SDK calls in-process instead, and is available for a subset of
providers; the rest use static transform mode.

| Provider | TypeScript | JavaScript | Python | Runtime shim |
|----------|:---:|:---:|:---:|:---:|
| OpenAI | Yes | Yes | Yes | Yes |
| Anthropic | Yes | Yes | Yes | Yes |
| Cohere | Yes | Yes | Yes | Yes |
| HuggingFace | Yes | Yes | Yes | Yes |
| Gemini | Yes | Yes | Yes | Static only |
| Groq | Yes | Yes | Yes | Static only |
| AWS Bedrock | Yes | Yes | Yes | SDK auto-instrument |

## Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `PROMPTGUARD_API_KEY` | — | API key (read by `init` and MCP tools) |
| `PROMPTGUARD_BASE_URL` | `https://api.promptguard.co/api/v1` | API base URL |

Global credentials are stored in `~/.promptguard/credentials.json`. Project-level configuration is stored in `.promptguard.json` in the project root.

## Development

```bash
cargo build              # Debug build
cargo build --release    # Optimized release (5.3MB)
cargo test               # Run tests
```

### Project Structure

```
src/
├── main.rs              # CLI entry point (Clap)
├── scanner/             # Recursive file scanning
├── detector/            # AST-based SDK detection (Tree-sitter)
├── transformer/         # AST-based code transformation
├── config/              # JSON configuration (Serde)
├── backup/              # Backup/restore system
├── api/                 # HTTP API client (Reqwest)
├── local_rules/         # Regex rule pack for `scan --offline` (generated)
└── commands/            # CLI commands (init, scan, mcp, redteam, policy)
```

## Uninstallation

```bash
curl -fsSL https://raw.githubusercontent.com/acebot712/promptguard-cli/main/uninstall.sh | sh
```

Or manually: `sudo rm /usr/local/bin/promptguard && rm -rf ~/.promptguard`

## Links

- [Documentation](https://docs.promptguard.co/tools/cli)
- [MCP Server Docs](https://docs.promptguard.co/tools/mcp)
- [Homepage](https://promptguard.co)
- [Dashboard](https://app.promptguard.co)

Apache 2.0 — See [LICENSE](LICENSE)
