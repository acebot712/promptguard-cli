# AGENTS.md

## Overview

Rust CLI for PromptGuard. Provides AST-based scanning for unprotected LLM calls, auto-fix code transforms, MCP server mode, red team testing, and policy management. Published as a single binary.

## Repository Layout

```
src/
├── main.rs            # CLI entry point (Clap)
├── scanner/           # AST-based LLM call detection
├── detector/          # Pattern matching and detection
├── transformer/       # Code transformation (auto-fix)
├── config/            # Configuration management
├── backup/            # File backup before transforms
├── api/               # PromptGuard API client
└── commands/          # CLI subcommands

tests/
├── runtime_shim_tests.rs
├── environment_scanner_tests.rs
├── command_tests.rs
├── api_command_tests.rs
└── fixtures/          # Test project fixtures
    ├── openai-hello-world/
    └── anthropic-hello-world/
```

## Setup

Requires Rust toolchain. Install via [rustup](https://rustup.rs/).

```bash
cargo build
```

## Building and Testing

```bash
cargo build                          # Debug build
cargo build --release                # Release build
cargo test                           # All tests
cargo test --test command_tests      # Single test file
cargo test test_name -- --nocapture  # Single test with output

# Via Makefile
make build
make test
make release
make ci                              # fmt-check + lint + test + build
```

## Code Quality

```bash
make format                          # rustfmt
make lint                            # Clippy with -D warnings
make fmt-check                       # Check formatting without modifying

# Or directly
cargo fmt
cargo clippy -- -D warnings
```

Always run `cargo fmt` and `cargo clippy` after editing Rust files.

## Coding Standards

- Rust 2021 edition
- Clippy lints configured in `Cargo.toml` under `[lints.*]`
- Use `clap` derive macros for CLI argument parsing
- Error handling: use `anyhow::Result` for application errors, custom error types for library code
- Tests go in `tests/` as integration tests, not inline `#[cfg(test)]` modules (unless testing private functions)
- Test fixtures in `tests/fixtures/` are real project directories used by the scanner

## Commit Messages

- Imperative mood: "Add X" not "Added X"
- Focus on what changed from the user's perspective

## Boundaries

### Ask first
- Adding new Cargo dependencies
- Changing the CLI interface (subcommands, flags, output format)
- Modifying Tree-sitter grammar usage

### Never
- Commit API keys, tokens, or credentials
- Break the CLI interface without a major version bump
- Skip `cargo clippy` warnings
