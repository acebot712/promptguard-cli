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
