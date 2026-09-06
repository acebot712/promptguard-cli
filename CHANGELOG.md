# Changelog

All notable changes to this project are documented here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and
this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

`Unreleased` holds work that is merged but not yet published. Move entries into
a dated version section when a release goes out — an `Unreleased` block that
survives three releases is a changelog nobody is maintaining.


## [Unreleased]

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

