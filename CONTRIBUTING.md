# Contributing to PromptGuard CLI

Thank you for your interest in contributing to PromptGuard!

## Development Setup

```bash
git clone https://github.com/acebot712/promptguard-cli.git
cd promptguard-cli
cargo build
```

## Code Quality

```bash
# Format
cargo fmt

# Lint
cargo clippy --all-targets --all-features -- -D warnings

# Security audit
cargo audit
```

## Running Tests

```bash
# All tests
cargo test --all-features --verbose

# Integration tests
cargo test --test runtime_shim_tests
cargo test --test environment_scanner_tests
```

## Pull Requests

1. Fork the repo and create a feature branch from `main`.
2. Write tests for any new functionality.
3. Ensure `cargo fmt -- --check` and `cargo clippy` pass with zero errors.
4. Ensure `cargo test --all-features` passes.
5. Open a PR with a clear description of the change.

## Reporting Issues

Open an issue at https://github.com/acebot712/promptguard-cli/issues with:
- OS and architecture
- Rust toolchain version (`rustc --version`)
- CLI version (`promptguard --version`)
- Minimal reproduction steps
