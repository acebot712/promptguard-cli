[![CI](https://github.com/acebot712/promptguard-cli/actions/workflows/ci.yml/badge.svg)](https://github.com/acebot712/promptguard-cli/actions/workflows/ci.yml)
[![License](https://img.shields.io/github/license/acebot712/promptguard-cli)](https://github.com/acebot712/promptguard-cli/blob/main/LICENSE)
[![Rust](https://img.shields.io/badge/Built_with-Rust-orange)](https://www.rust-lang.org/)

# PromptGuard CLI

> Drop-in LLM security for your applications — built with Rust + Tree-sitter

![Rust](https://img.shields.io/badge/rust-1.70+-orange?logo=rust)
![License](https://img.shields.io/badge/license-Apache%202.0-green)

Single 5.3MB binary. No runtime interpreter required. Real AST transformations. Under 10ms startup.

## Installation

| Method | Command |
|--------|---------|
| **Homebrew** | `brew tap promptguard/tap && brew install promptguard` |
| **Shell script** | `curl -fsSL https://raw.githubusercontent.com/acebot712/promptguard-cli/main/install.sh \| sh` |
| **Cargo** | `cargo install promptguard` |
| **Binary** | Download from [GitHub Releases](https://github.com/acebot712/promptguard-cli/releases/latest) |

Supported platforms: macOS ARM64 (M1/M2/M3), macOS x86_64, Linux x86_64, Linux ARM64.

> For an MCP-only install (no Rust required), use `pip install promptguard-mcp-server` instead.

## Quick Start

```bash
promptguard login                                  # Authenticate (works anywhere)
promptguard scan --text "ignore previous instructions"  # Instant threat scan
promptguard init --api-key pg_live_xxx             # Integrate a project
promptguard verify                                 # Test connectivity + scan + redact
promptguard status                                 # Check configuration
promptguard mcp -t stdio                           # Start MCP server
```

## Commands

| Command | Description |
|---------|-------------|
| `init` | Initialize PromptGuard and rewrite SDK constructors to route through proxy |
| `scan` | Scan project for LLM SDK usage, or scan text/files for security threats |
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
run, and checks every tool call before the server sees it:

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
not pass `PROMPTGUARD_API_KEY` on to the server it wraps.

### What it checks

Each `tools/call` is checked twice, in order:

1. **Locally**, by a destructive-action check on the call's arguments:
   recursive forced removal (`rm -rf`), `DROP`/`TRUNCATE TABLE|DATABASE`,
   `mkfs`, a download piped into a shell, `/etc/shadow`, and `../../` path
   climbing.
2. **By the PromptGuard API**, through `POST /api/v1/agent/validate-tool`, which
   applies your project's agent policies and behaviour analysis. Calls are
   reported under the agent ID `mcp-proxy:<server name>`; set your own with
   `--agent-id`.

A refused call never reaches the server. The agent gets a JSON-RPC error with
code `-32600` and a message naming the reason, and the proxy logs it to stderr.

### When the API is unreachable

`--fail-mode` decides what happens to a call the API cannot give a verdict on
(unreachable, timed out after 10 seconds, or returning an error):

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

### What is relayed, and how

- **From the server to the agent:** everything, byte for byte, including lines
  that are not JSON (so a server that prints a banner to stdout still works).
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
too large to check, so relaying it would be a way around the check); a larger
one from the server is streamed through. At most 1024 requests may be pending
at once; one past that is refused with `-32600`. A JSON-RPC batch that contains
a `tools/call` is refused whole.

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
