[![CI](https://github.com/acebot712/promptguard-cli/actions/workflows/ci.yml/badge.svg)](https://github.com/acebot712/promptguard-cli/actions/workflows/ci.yml)
[![License](https://img.shields.io/github/license/acebot712/promptguard-cli)](https://github.com/acebot712/promptguard-cli/blob/main/LICENSE)
[![Rust](https://img.shields.io/badge/Built_with-Rust-orange)](https://www.rust-lang.org/)

# PromptGuard CLI

> Drop-in LLM security for your applications — built with Rust + Tree-sitter

![Version](https://img.shields.io/badge/version-1.5.1-blue)
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
promptguard init --api-key pg_sk_prod_YOUR_KEY    # Configure
promptguard verify                                 # Test connectivity + scan + redact
promptguard scan                                   # Find LLM SDK usage
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
| `redteam` | Red team testing (manual or `--autonomous` with LLM agent) |
| `verify` | End-to-end integration check: connectivity, auth, scan, and redact |
| `policy` | Policy-as-code: `apply`, `diff`, `export` YAML guardrail configs |
| `login` / `logout` | Authenticate with PromptGuard cloud |
| `whoami` | Show current authenticated user |
| `projects` | `list` / `select` active project |
| `config` | View or update local configuration |
| `key` | Manage API keys |
| `logs` | View recent security event logs |
| `events` | View security events |
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

| Provider | TypeScript | JavaScript | Python |
|----------|:---:|:---:|:---:|
| OpenAI | Yes | Yes | Yes |
| Anthropic | Yes | Yes | Yes |
| Cohere | Yes | Yes | Yes |
| HuggingFace | Yes | Yes | Yes |
| Gemini | Yes | Yes | Yes |
| Groq | Yes | Yes | Yes |
| AWS Bedrock | Yes | Yes | Yes |

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
