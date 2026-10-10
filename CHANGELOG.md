# Changelog

All notable changes to this project are documented here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and
this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

`Unreleased` holds work that is merged but not yet published. Move entries into
a dated version section when a release goes out — an `Unreleased` block that
survives three releases is a changelog nobody is maintaining.


## [Unreleased]

## [2.1.2] - 2026-10-10

**No change to the CLI's own code.** No source file changed since 2.1.1; this
release rebuilds the binaries against newer patch versions of four direct
dependencies, so the published binaries match what `main` has been building
and testing.

### Changed

- **clap 4.6.6 → 4.6.7, reqwest 0.13.4 → 0.13.5, thiserror 2.0.20 → 2.0.21 and
  open 5.4.2 → 5.4.4** (#45). Patch releases of the argument parser, the HTTP
  client, the error-derive macro and the browser launcher. reqwest 0.13.5 moves
  to base64 0.23.1, so the lockfile carries that alongside 0.22.1. None of the
  four is a security fix: no advisory applied to the versions 2.1.1 shipped.
- The `dtolnay/rust-toolchain` action that installs Rust in CI and in the
  release build is pinned to a newer commit (#44). It still installs `stable`.

## [2.1.1] - 2026-09-18

### Security

- **rustls 0.23.45, for RUSTSEC-2026-0285.** The TLS stack every HTTP call in
  this CLI goes through. Published so the released binaries carry the patched
  version rather than only the source tree.


## [2.1.0] - 2026-09-06

### Added

- **`policy apply` accepts `tokenize` as a `pii_detection.mode`.** It validated
  the field against a hand-written list of `redact`/`mask`/`block` and rejected
  `tokenize`, telling you a value the API accepts is invalid. Tokenize is the
  reversible mode — the proxy tokenises on the way out and restores on the way
  back, streaming included — so the one PII mode whose effect can be undone was
  the one unreachable from policy-as-code.

  The platform had this same bug: `tokenize` was implemented end to end and then
  omitted from the write schema, which made a finished feature unreachable for
  every customer. This list is a third copy of that vocabulary and stays
  hand-maintained, so it now records where the source of truth is and that a new
  mode has to be added here too.

## [2.0.0] - 2026-08-24

### Fixed

- **`redteam` was unreachable for every customer.** Its subcommands targeted
  `/internal/redteam`, the platform-admin plane, which rejects an API key
  outright — so the command could not have worked for anyone using the CLI as
  documented. It now targets the customer-facing `/api/v1/security-testing`.

### Removed

- **BREAKING — `logs` and `events` are gone.** Both called endpoints the API
  does not serve, so they only ever failed. Scripts invoking `promptguard logs`
  or `promptguard events` will now exit with an unrecognised-subcommand error
  rather than a request failure. There is no replacement command; security
  events are available in the dashboard.
- **BREAKING — `redteam --autonomous` and the intelligence-stats output are
  gone**, for the same reason: the endpoints behind them do not exist.

## [1.7.2] - 2026-08-19

### Security

- **h2 bumped to 0.4.17** for RUSTSEC-2026-0258. Versions below 0.4.16 accept
  and queue empty HTTP/2 DATA frames without limit — unbounded memory if
  streams are not drained, or a panic when the length overflows. It reaches the
  CLI transitively through `reqwest` -> `hyper`, so it sits under every API call
  the CLI makes. DoS only; no data exposure, no API change.

  Caught by this repo's nightly `cargo-deny` run, not by Dependabot, which
  reported no open alerts on this repository while the advisory was live.

