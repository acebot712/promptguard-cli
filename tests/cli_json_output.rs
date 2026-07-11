#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! End-to-end tests that run the real `promptguard` binary and assert its
//! `--json` contract: JSON-mode output on stdout must always be parseable,
//! both on success (`doctor --json`) and on failure (`scan --json` with no
//! credentials). Regression coverage for two terminal-UX fixes:
//!   * `doctor --json` previously printed the human 🩺 report before the JSON.
//!   * `--json` errors previously printed a plain "Error: …" line, not JSON.

use std::process::Command;
use tempfile::TempDir;

/// Path to the binary under test (set by Cargo for integration tests).
fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_promptguard")
}

/// Run the binary in an isolated dir with no resolvable credentials.
fn run_isolated(dir: &TempDir, args: &[&str]) -> std::process::Output {
    Command::new(bin())
        .args(args)
        .current_dir(dir.path())
        // Isolate credential resolution: no env key, no global creds.
        .env_remove("PROMPTGUARD_API_KEY")
        .env("HOME", dir.path())
        .env("USERPROFILE", dir.path())
        .output()
        .expect("failed to spawn promptguard")
}

/// `doctor --json` must emit ONLY a JSON object on stdout (no human 🩺 block),
/// so `doctor --json | jq` works.
#[test]
fn doctor_json_stdout_is_valid_json() {
    let dir = TempDir::new().unwrap();
    let out = run_isolated(&dir, &["doctor", "--json"]);

    let stdout = String::from_utf8_lossy(&out.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("doctor --json stdout was not valid JSON: {e}\n---\n{stdout}"));

    assert!(
        parsed.get("health").is_some(),
        "doctor JSON should have a `health` field, got: {stdout}"
    );
    // No stray human markers should leak onto stdout in JSON mode.
    assert!(
        !stdout.contains("Diagnostics") && !stdout.contains('•'),
        "human report leaked into doctor --json stdout: {stdout}"
    );
}

/// A `--json` command that fails (here: `scan --json --text` with no API key)
/// must still emit a JSON error object on stdout and exit non-zero.
#[test]
fn scan_json_error_is_valid_json() {
    let dir = TempDir::new().unwrap();
    let out = run_isolated(&dir, &["scan", "--json", "--text", "hello"]);

    assert!(
        !out.status.success(),
        "scan with no credentials should exit non-zero"
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap_or_else(|e| {
        panic!("scan --json error stdout was not valid JSON: {e}\n---\n{stdout}")
    });

    assert!(
        parsed
            .get("error")
            .and_then(serde_json::Value::as_str)
            .is_some(),
        "error JSON should have a string `error` field, got: {stdout}"
    );
    assert!(
        parsed
            .get("code")
            .and_then(serde_json::Value::as_str)
            .is_some(),
        "error JSON should have a string `code` field, got: {stdout}"
    );
}
