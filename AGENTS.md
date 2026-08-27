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

**Enable the tracked git hooks once per clone** — git will not turn on a repo's
own hooks for you:

```bash
git config core.hooksPath .githooks
git config promptguard.pushGate true    # optional: also run `make ci` on push
```

`.githooks/pre-push` scans the commits you are pushing for secrets. It is
scoped to that range on purpose: this repo is public, so a credential on `main`
is world-readable at once, while scanning the whole tree or the history fails on
its first run and teaches everyone `--no-verify`. Fixtures that are meant to
look like credentials — the env-var redaction tests — are exempted by path in
`.gitleaks.toml`, where the exemption is reviewable. `make ci` is opt-in
because fmt-check + clippy + test + build is minutes on this workspace.

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

## API types are hand-written, deliberately

`promptguard-python` and `promptguard-node` generate their API types from the
published OpenAPI spec, on a weekly `sync-from-api.yml` that opens a PR when the
spec moves. **This repo does not, and that is a decision, not an oversight.**
Recorded 2026-08-11 after checking the spec rather than guessing.

The response structs live in `src/commands/*.rs` (`SecurityScanResponse`,
`RedactResponse`, `GuardrailsResponse`, `LogsResponse`, `RedTeamTestResult`) and
stay hand-written, because generating them would cost machinery and return
nothing:

- **The spec does not carry the information worth generating.** The one field
  where a generated type would help is `threatType`, and in
  `openapi-developer.json` it is `{"anyOf": [{"type": "string"}, {"type":
  "null"}]}` — no enum. The real values live in the backend's `ThreatType`
  StrEnum and never reach the published spec. A generator would emit
  `Option<String>`, which is what `SecurityScanResponse` already declares by
  hand. Same for `decision` and `reason`: bare strings.
- **Part of the surface is not in the published spec at all.** `promptguard
  redteam` calls `/internal/redteam/*`, which is internal-domain and appears
  only in the full `openapi.json` — a spec that is `.mintignore`d and not served
  publicly. `/health` is in neither spec. A sync would cover some endpoints and
  silently skip others, which is worse than covering none: it reads as complete.
- **The surface is small and slow.** Seven endpoints, and the structs pin the
  serde renames (`threatType`, `eventId`, `processingTimeMs`, `piiFound`) that
  the wire format actually uses. Those renames are the part that breaks, and a
  generator would reproduce them from the same spec that already agrees.

**What protects this repo instead:** the structs are `#[derive(Deserialize)]`
without `deny_unknown_fields`, so an added field is ignored and a *removed* or
renamed one fails at parse time in the integration tests — loudly, at the call
site, in the repo that owns the code.

**Revisit this if** the developer spec starts publishing real enums for
`threatType`/`decision`, or the CLI's endpoint count grows past a handful of
hand-maintainable structs. At that point copy the SDKs' `sync-from-api.yml`
wholesale; do not invent a second mechanism.

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

## Agent skills

### Issue tracker

Issues live in this repo's GitHub Issues, worked via the `gh` CLI. See `docs/agents/issue-tracker.md`.

### Triage labels

The five canonical triage roles, each label string equal to its name. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: `CONTEXT.md` plus `docs/adr/` at the repo root, both created lazily. See `docs/agents/domain.md`.
