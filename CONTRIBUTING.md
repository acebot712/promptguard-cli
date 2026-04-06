# Contributing to PromptGuard CLI

## Prerequisites

| Tool | Version | Install |
|------|---------|---------|
| Rust | stable (latest) | [rustup.rs](https://rustup.rs/) |
| cargo-audit | latest | `cargo install cargo-audit` |
| cargo-watch | latest (optional) | `cargo install cargo-watch` |

## Quick Start

```bash
git clone https://github.com/acebot712/promptguard-cli.git
cd promptguard-cli
make setup              # Install git pre-commit hook
make build              # Build debug binary
make test               # Run all tests
```

## Development Workflow

### Build

```bash
make build              # Debug binary (fast compile)
make release            # Optimized release binary (slower compile, smaller output)
make install            # Install to ~/.cargo/bin/promptguard
make uninstall          # Remove from ~/.cargo/bin
```

### Dev Loop

```bash
make dev                # Watch mode - rebuilds on file changes (requires cargo-watch)
```

### Code Quality

```bash
make format             # Format code with rustfmt
make fmt-check          # Check formatting (CI uses this)
make lint               # Run clippy with -D warnings (warnings are errors)
make check              # Quick cargo check (faster than full build)
make ci                 # Run everything CI runs: fmt-check + lint + test + build
```

The pre-commit hook (`make setup`) runs `make ci` before every commit.

### Lint Rules

The project enforces strict Clippy rules in `Cargo.toml`:

- `unsafe_code = "forbid"` -- no unsafe code allowed
- `panic`, `exit`, `unwrap_used`, `expect_used` = `"deny"` -- use `anyhow::Result` instead
- `todo`, `unimplemented` = `"deny"` -- no placeholder code
- `pedantic = "warn"` -- comprehensive quality checks enabled

## Testing

### Running Tests

```bash
make test                                     # All tests
cargo test --all-features --verbose           # Same thing, directly
cargo test --test runtime_shim_tests          # Runtime shim integration tests only
cargo test --test environment_scanner_tests   # Environment scanner tests only
cargo test --test command_tests               # CLI command tests only
cargo test --test api_command_tests           # API command tests only
```

### Test Files

| File | What it tests |
|---|---|
| `tests/runtime_shim_tests.rs` | OpenAI/Anthropic SDK interception and proxy shimming |
| `tests/environment_scanner_tests.rs` | Detection of LLM SDKs, env files, and project structure |
| `tests/command_tests.rs` | CLI command parsing and execution |
| `tests/api_command_tests.rs` | API subcommand behavior |

### Test Fixtures

Fixture projects live in `tests/fixtures/`:

```
tests/fixtures/
  openai-hello-world/       # Minimal OpenAI project (TS, JS, Python)
  anthropic-hello-world/    # Minimal Anthropic project (TS, JS, Python)
```

Each fixture contains sample code, `package.json`/`requirements.txt`, and `.env` files used by integration tests to verify SDK detection and transformation.

## CI/CD

CI runs on every push to `main` and on PRs (`.github/workflows/ci.yml`):

| Job | What it does |
|---|---|
| **Code Quality** | `cargo fmt --check` + `cargo clippy -D warnings` |
| **Test Suite** | `cargo test` on `{ubuntu, macos}` x `{stable, beta}` Rust |
| **Security Audit** | `cargo audit` via `rustsec/audit-check` |

Reproduce CI locally:

```bash
make ci                 # fmt-check + lint + test + build
cargo audit             # Security audit (run separately)
```

## Releasing

Releases are triggered by pushing a tag matching `cli-v*`:

```bash
# 1. Update version in Cargo.toml
# 2. Commit and push to main
git tag cli-v2.1.0
git push origin cli-v2.1.0
```

The release workflow (`.github/workflows/release-cli.yml`):

1. Cross-compiles for macOS ARM64, macOS x86_64, Linux x86_64, Windows x86_64
2. Generates SHA256 checksums for each binary
3. Creates a GitHub Release with all binaries attached

### Local Cross-Compile

```bash
make cross-compile      # Build for all platforms into dist/
```

Requires the appropriate Rust targets installed:

```bash
rustup target add x86_64-apple-darwin aarch64-apple-darwin x86_64-unknown-linux-gnu x86_64-pc-windows-msvc
```

## PR Checklist

- [ ] `make ci` passes (format, lint, test, build)
- [ ] `cargo audit` has no new advisories
- [ ] New functionality has tests
- [ ] No `unwrap()`, `expect()`, `panic!()`, or `todo!()` in production code
- [ ] PR description explains the change

## Reporting Issues

Open an issue at https://github.com/acebot712/promptguard-cli/issues with:

- OS and architecture
- Rust toolchain version (`rustc --version`)
- CLI version (`promptguard --version`)
- Minimal reproduction steps
