# PromptGuard CLI

> **Scan, detect, and secure LLM SDK usage in your codebase** - Built with Rust + Tree-sitter

![Version](https://img.shields.io/badge/version-1.1.1-blue)
![Rust](https://img.shields.io/badge/rust-1.70+-orange?logo=rust)
![License](https://img.shields.io/badge/license-Apache%202.0-green)
![Status](https://img.shields.io/badge/status-production%20ready-success)

**Single 5.3MB binary • Zero dependencies • Real AST transformations • <10ms startup**

---

## Production Status

✅ **PRODUCTION READY** - All features implemented and tested

- ✅ **Core Features**: Init, Scan, Apply, Revert, Disable, Enable
- ✅ **AST Transformations**: TypeScript, JavaScript, Python (Tree-sitter powered)
- ✅ **Provider Support**: OpenAI, Anthropic, Cohere, HuggingFace, Gemini, Groq, AWS Bedrock
- ✅ **Backup/Restore**: Automatic backups with safe revert
- ✅ **Configuration**: Persistent config with enabled/disabled states
- ✅ **Management**: Config viewer, API key management, status checks
- ✅ **Release Build**: Optimized binary (5.3MB) with LTO and strip

**Tested workflows**:
- Full init → transform → revert cycle
- Disable → enable toggle workflow
- Backup creation and restoration
- TypeScript/JavaScript transformations (baseURL injection)
- Python transformations (base_url injection + import os)

---

## Why This CLI?

This is a complete **Rust rewrite** using proper Tree-sitter AST parsing. Unlike regex-based tools, it provides:

- ✅ **True AST transformations** - Never breaks your code
- ✅ **Zero false positives** - Precise detection and modification
- ✅ **Single static binary** - No Python, Node.js, or runtime dependencies
- ✅ **Instant startup** - <10ms cold start
- ✅ **7 Providers** - OpenAI, Anthropic, Cohere, HuggingFace, Gemini, Groq, AWS Bedrock

---

## Installation

### One-Line Install (Recommended)

**macOS / Linux:**
```bash
curl -fsSL https://raw.githubusercontent.com/acebot712/promptguard-cli/main/install.sh | sh
```

The install script will:
- Detect your OS and architecture automatically
- Download the appropriate binary from GitHub releases
- Verify checksums
- Install to `/usr/local/bin/promptguard`
- Test the installation

### Manual Installation

**1. Download binary for your platform:**

Visit [GitHub Releases](https://github.com/acebot712/promptguard-cli/releases/latest) and download:
- `promptguard-macos-arm64` - macOS Apple Silicon (M1/M2/M3)
- `promptguard-macos-x86_64` - macOS Intel
- `promptguard-linux-x86_64` - Linux 64-bit
- `promptguard-linux-arm64` - Linux ARM64

**2. Install:**
```bash
# Make executable
chmod +x promptguard-*

# Move to PATH
sudo mv promptguard-* /usr/local/bin/promptguard

# Verify
promptguard --version
```

### Package Managers (Coming Soon)

```bash
# Homebrew (macOS)
brew install promptguard/tap/promptguard

# Cargo (Rust)
cargo install promptguard-cli

# npm (JavaScript ecosystem)
npm install -g promptguard
```

---

## Uninstallation

### Uninstall (if installed via install.sh)

**macOS / Linux:**
```bash
curl -fsSL https://raw.githubusercontent.com/acebot712/promptguard-cli/main/uninstall.sh | sh
```

The uninstall script will:
- Remove the binary from `/usr/local/bin/promptguard`
- Ask if you want to remove configuration files
- Verify successful removal

**Manual uninstall:**
```bash
# Remove binary
sudo rm /usr/local/bin/promptguard

# Optionally remove configuration
rm -rf ~/.promptguard
```

### Uninstall (if installed via make install)

**From the project directory:**
```bash
make uninstall
```

**Or using cargo directly:**
```bash
cargo uninstall promptguard-cli
```

**Optionally remove configuration:**
```bash
rm -rf ~/.promptguard
```

---

## Quick Start

### Usage

```bash
# Initialize PromptGuard in your project
promptguard init --api-key pg_sk_test_xxx

# Scan for LLM SDKs (dry-run)
promptguard scan

# Check status
promptguard status

# Diagnostics
promptguard doctor
```

---

## Integration Options

The CLI provides **proxy-mode integration** — it rewrites SDK constructors to route traffic through PromptGuard. This is one of several ways to integrate:

| Method | Best For | How |
|--------|----------|-----|
| **SDK Auto-Instrumentation** (recommended) | New projects, all providers | `promptguard.init()` in Python/Node.js |
| **CLI Proxy Transform** (this tool) | Existing codebases, bulk migration | `promptguard init` rewrites constructors |
| **HTTP Proxy** | Any language, OpenAI-compatible APIs | Change `baseURL` manually |
| **Guard API** | Custom pipelines, any framework | `POST /api/v1/guard` |

For new projects, the [PromptGuard SDK](https://docs.promptguard.co/sdks/python) with auto-instrumentation is the recommended approach — one line of code secures all LLM calls with no code rewrites.

## How the CLI Works

### Before
```typescript
const openai = new OpenAI({ 
  apiKey: process.env.OPENAI_API_KEY 
});
```

### After `promptguard init`
```typescript
const openai = new OpenAI({
  apiKey: process.env.PROMPTGUARD_API_KEY,
  baseURL: "https://api.promptguard.co/api/v1"
});
```

**Result:** All LLM requests now go through PromptGuard's security layer!

---

## Commands

| Command | Description |
|---------|-------------|
| `init` | Initialize PromptGuard in this project |
| `scan` | Scan project for LLM SDK usage |
| `status` | Show current configuration |
| `doctor` | Diagnose common issues |
| `apply` | Apply pending changes |
| `disable` | Temporarily disable |
| `enable` | Re-enable |
| `revert` | Complete removal |
| `redteam` | Red team testing tools |
| `redteam --autonomous` | Run autonomous red team agent (LLM-powered) |
| `policy` | Manage guardrails as YAML (apply/diff/export) |

---

## Architecture

### True AST Parsing (Not Regex!)

```rust
// Tree-sitter query for detecting OpenAI constructor
(new_expression
    constructor: (identifier) @constructor
    (#eq? @constructor "OpenAI")
    arguments: (arguments) @args
) @new_expr
```

**Why this matters:**
- ✅ Never matches patterns in strings or comments
- ✅ Handles complex nested structures correctly
- ✅ Validates syntax automatically
- ✅ Zero false positives

### Supported Languages & Providers

| Provider | TypeScript | JavaScript | Python | Proxy Transform |
|----------|------------|------------|--------|-----------------|
| OpenAI | ✅ | ✅ | ✅ | ✅ |
| Anthropic | ✅ | ✅ | ✅ | ✅ |
| Cohere | ✅ | ✅ | ✅ | ✅ |
| HuggingFace | ✅ | ✅ | ✅ | ✅ |
| Gemini | ✅ | ✅ | ✅ | ✅ |
| Groq | ✅ | ✅ | ✅ | ✅ |
| AWS Bedrock | ✅ | ✅ | ✅ | — (use SDK) |

> **Note:** AWS Bedrock uses the AWS SDK command pattern and cannot be transformed via proxy URL injection. For Bedrock, use the [PromptGuard SDK auto-instrumentation](https://docs.promptguard.co/sdks/python) instead.

---

## Development

### Build

```bash
# Debug build
cargo build

# Release build (optimized, 4.3MB)
cargo build --release

# Run tests
cargo test
```

### Project Structure

```
src/
├── main.rs              # CLI entry point (Clap)
├── scanner/             # Recursive file scanning
├── detector/            # AST-based SDK detection
│   ├── typescript.rs    # Tree-sitter TypeScript/JavaScript
│   └── python.rs        # Tree-sitter Python
├── transformer/         # AST-based code transformation
│   ├── typescript.rs    # TS/JS transformer
│   └── python.rs        # Python transformer
├── config/              # JSON configuration (Serde)
├── backup/              # Backup/restore system
├── env/                 # .env file manager
├── api/                 # HTTP API client (Reqwest)
└── commands/            # 17 CLI commands (incl. redteam, policy.rs)
```

**Total:** 2,325 LOC Rust • 11 modules • Clean architecture

---

## Why Rust?

### vs Python CLI
- ✅ **10x faster** (<10ms vs ~100ms startup)
- ✅ **Single binary** (no Python interpreter)
- ✅ **Zero pip/venv hell** (works on fresh systems)

### vs Go CLI
- ✅ **Better AST libraries** (Tree-sitter is first-class)
- ✅ **No cgo needed** (Go requires cgo for Tree-sitter)
- ✅ **Type safety** (Rust catches bugs at compile time)

---

## Binary Metrics

| Metric | Value |
|--------|-------|
| Size (release) | 4.3MB |
| Dependencies | 0 runtime |
| Startup time | <10ms |
| Cross-platform | ✅ macOS, Linux, Windows |

---

## Examples

### Scan a project
```bash
$ promptguard scan

🛡️  PromptGuard CLI v1.1.0

📊 LLM SDK Detection Report

OpenAI SDK (3 files, 5 instances)
├── src/api/chat.ts
├── src/services/embeddings.ts
└── lib/openai.ts

Summary:
  • Total files scanned: 247
  • Total instances: 5

Providers detected:
  ✓ openai

Next: promptguard init
```

### Initialize with dry-run
```bash
promptguard init --api-key pg_sk_test_xxx --dry-run

# Shows what would change without modifying files
```

### Check status
```bash
$ promptguard status

Status: ✓ Active
API Key: pg_sk_test_*** (configured)
Proxy URL: https://api.promptguard.co/api/v1

Configuration:
  • Files managed: 3
  • Providers: openai
```

### Autonomous Red Team

```bash
# Run autonomous agent with default budget (100 iterations)
promptguard redteam --autonomous --preset strict

# Higher budget for thorough testing
promptguard redteam --autonomous --budget 500 --preset support_bot:strict

# JSON output for CI
promptguard redteam --autonomous --format json
```

### Policy-as-Code

Define guardrails in YAML and version them in git:

```bash
# Export current config as YAML
promptguard policy export --project-id proj_abc123 > policy.yaml

# Preview changes
promptguard policy diff policy.yaml --project-id proj_abc123

# Apply changes (with dry-run)
promptguard policy apply policy.yaml --project-id proj_abc123 --dry-run

# Apply for real
promptguard policy apply policy.yaml --project-id proj_abc123
```

Example `policy.yaml`:

```yaml
guardrails:
  prompt_injection:
    level: strict
  pii_detection:
    level: strict
    mode: redact
  toxicity:
    threshold: 0.7
  data_exfiltration:
    level: moderate
```

---

## Makefile Targets

```bash
make build          # Build debug binary
make release        # Build optimized release
make install        # Install to ~/.cargo/bin
make test           # Run tests
make clean          # Clean artifacts
```

---

## License

Apache 2.0 - See [LICENSE](LICENSE)

---

## Links

- **Homepage**: https://promptguard.co
- **Documentation**: https://docs.promptguard.co
- **SDK Docs**: https://docs.promptguard.co/sdks/python
- **Dashboard**: https://app.promptguard.co
- **GitHub**: https://github.com/acebot712/promptguard-cli

---

**Built with Rust 🦀 | Powered by Tree-sitter 🌳 | Zero Compromises ⚡**
