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

/// `doctor --json` must include a per-check `checks` array (each entry
/// {name, status, message}) so CI can see WHICH check warned/failed, not just
/// the aggregate counts.
#[test]
fn doctor_json_includes_checks_array() {
    let dir = TempDir::new().unwrap();
    let out = run_isolated(&dir, &["doctor", "--json"]);

    let stdout = String::from_utf8_lossy(&out.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("doctor --json stdout was not valid JSON: {e}\n---\n{stdout}"));

    let checks = parsed
        .get("checks")
        .and_then(serde_json::Value::as_array)
        .unwrap_or_else(|| panic!("doctor --json missing `checks` array: {stdout}"));
    assert!(
        !checks.is_empty(),
        "checks array should not be empty: {stdout}"
    );
    for check in checks {
        for field in ["name", "status", "message"] {
            assert!(
                check
                    .get(field)
                    .and_then(serde_json::Value::as_str)
                    .is_some(),
                "each check needs a string `{field}`, got: {check}"
            );
        }
        let status = check
            .get("status")
            .and_then(serde_json::Value::as_str)
            .unwrap();
        assert!(
            matches!(status, "ok" | "warning" | "error"),
            "unexpected check status {status:?} in {check}"
        );
    }
}

/// Every command that resolves credentials to hit the API must surface the
/// SAME canonical code + message when none are resolvable. Regression guard
/// against logs/events/scan/redact drifting to different remedies (logs used
/// to demand "Run 'promptguard init' first" while the rest said "login").
#[test]
fn missing_credentials_error_is_unified() {
    let dir = TempDir::new().unwrap();
    let cases: [&[&str]; 4] = [
        &["logs", "--json"],
        &["events", "--json"],
        &["scan", "--json", "--text", "hi"],
        &["redact", "--json", "--text", "hi"],
    ];
    for args in cases {
        let out = run_isolated(&dir, args);
        assert!(
            !out.status.success(),
            "{args:?} should exit non-zero with no credentials"
        );
        let stdout = String::from_utf8_lossy(&out.stdout);
        let parsed: serde_json::Value = serde_json::from_str(stdout.trim())
            .unwrap_or_else(|e| panic!("{args:?} --json was not valid JSON: {e}\n---\n{stdout}"));

        assert_eq!(
            parsed.get("code").and_then(serde_json::Value::as_str),
            Some("no_credentials"),
            "{args:?} should report code=no_credentials, got: {stdout}"
        );
        assert_eq!(
            parsed.get("error").and_then(serde_json::Value::as_str),
            Some("No API key found. Run 'promptguard login' or set PROMPTGUARD_API_KEY"),
            "{args:?} message drifted from the canonical text, got: {stdout}"
        );
    }
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
