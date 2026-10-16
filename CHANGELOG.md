# Changelog

All notable changes to this project are documented here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and
this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

`Unreleased` holds work that is merged but not yet published. Move entries into
a dated version section when a release goes out — an `Unreleased` block that
survives three releases is a changelog nobody is maintaining.


## [Unreleased]

### Added

- **`promptguard mcp-proxy -- <server command>`** wraps a stdio MCP server and
  checks every tool call before the server sees it: first a local
  destructive-action check on the arguments (the earlier rule set's pattern, which we
  extended to catch `rm -r -f`, `rm --recursive --force` in any order,
  `rm -rfv`, `find -delete`, `find -exec rm`, `DROP SCHEMA`, `TRUNCATE`,
  `DELETE FROM` with no `WHERE`, `dd`/`shred`/`wipefs` on a disk, a recursive
  `chmod`/`chown` of `/`, the fork bomb, `curl | sudo bash`, `base64 -d | sh`,
  `sh -c "$(curl ...)"`, a download piped into `python`/`perl`/`ruby`/`node`,
  `DROP INDEX`, `ALTER TABLE ... DROP`, `rm --rec --forc`,
  `Remove-Item -Recurse -Force`, `del /s`, `rmdir /s`, `git clean -f`,
  `git reset --hard` and a forced `git push`, after NFKC normalisation and with zero-width characters
  removed), then the API's
  `POST /agent/validate-tool`. A refused call is answered with JSON-RPC
  `-32600` and never reaches the server.
  - `--fail-mode open|closed|mixed` (default `mixed`) decides what happens when
    the API cannot be reached (no answer, a timeout or a 5xx; a 429 plan limit
    too, logged as such): `mixed` refuses calls the local check matches and
    relays the rest, logged to stderr as unchecked. A rejected key, another
    4xx or a malformed answer refuses the call in every mode, and a key the API
    rejects at startup stops the proxy before it starts the server.
  - Agent messages reach the server as the proxy parsed them, never as the
    agent's bytes; an agent line that is not JSON, or holds a bare carriage
    return, is dropped. A server that parses differently cannot see a tool
    call the proxy did not check.
  - Server output and stderr pass through untouched. Messages are bounded at
    4 MiB; at most 1024 requests may be pending, and a request reusing a
    pending id (`1` and `1.0` are one id) is refused, as are a tool call
    without an id and a batch holding anything but JSON-RPC objects.
  - The wrapped server does not inherit `PROMPTGUARD_API_KEY`, and logs name
    the server's program, never its arguments (acebot712/promptguard#943).

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

