#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! End-to-end tests for the YAML that `promptguard policy` reads and writes,
//! run against the real binary and a loopback stand-in for the guardrails API.
//!
//! The policy command has exactly two YAML paths:
//!
//! - **read**: `policy apply` and `policy diff` parse a policy file into the
//!   JSON the API takes (`load_yaml`);
//! - **write**: `policy export` turns the live JSON config into YAML.
//!
//! These tests pin both, byte for byte where the output is user-visible, so
//! the YAML library underneath can change without a policy file that already
//! works changing meaning, and without `export` output changing shape.
//! `diff` against an empty live config prints each desired top-level section
//! as one JSON object, which makes it a direct window onto what the read path
//! produced. The expected strings were captured from `serde_yaml` 0.9, the
//! library these paths used before.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::process::{Command, Output};
use tempfile::TempDir;

/// A loopback HTTP server that answers every request with `body` as JSON,
/// until the test process exits.
fn serve_json(body: &'static str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            // Read the request head; GET carries no body.
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                    break;
                }
            }
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\
                 Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    url
}

fn run_policy(dir: &TempDir, url: &str, args: &[&str]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_promptguard"));
    cmd.arg("policy")
        .args(args)
        .args(["--project-id", "proj_test", "--api-key", "pg_test_key"])
        .args(["--base-url", url])
        .current_dir(dir.path())
        .env("HOME", dir.path())
        .env("USERPROFILE", dir.path())
        .env_remove("PROMPTGUARD_API_KEY")
        .env_remove("PROMPTGUARD_BASE_URL")
        .env("NO_COLOR", "1");
    for var in [
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "http_proxy",
        "https_proxy",
        "all_proxy",
    ] {
        cmd.env_remove(var);
    }
    cmd.output().unwrap()
}

/// `policy diff <yaml>` against an empty live config: every desired section is
/// printed as JSON, so the output shows exactly what the YAML parsed to.
fn diff_against_empty(yaml: &str) -> Output {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("policy.yaml"), yaml).unwrap();
    let url = serve_json(r#"{"guardrails": {}}"#);
    run_policy(&dir, &url, &["diff", "policy.yaml"])
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// The policy file as the docs (`security/policy-as-code.mdx`) show it.
const DOCUMENTED_POLICY: &str = "\
guardrails:
  prompt_injection:
    level: strict
  pii_detection:
    level: strict
    mode: redact
  data_exfiltration:
    level: moderate
  toxicity:
    threshold: 0.7
  secret_key_detection:
    level: moderate
";

#[test]
fn documented_policy_file_parses_to_the_expected_json() {
    let out = diff_against_empty(DOCUMENTED_POLICY);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let s = stdout(&out);
    for line in [
        "  data_exfiltration:\n    - (none)\n    + {\"level\":\"moderate\"}\n",
        "  pii_detection:\n    - (none)\n    + {\"level\":\"strict\",\"mode\":\"redact\"}\n",
        "  prompt_injection:\n    - (none)\n    + {\"level\":\"strict\"}\n",
        "  secret_key_detection:\n    - (none)\n    + {\"level\":\"moderate\"}\n",
        "  toxicity:\n    - (none)\n    + {\"threshold\":0.7}\n",
    ] {
        assert!(s.contains(line), "missing {line:?} in:\n{s}");
    }
}

/// Scalars a hand-written policy file can contain, and what each must become.
/// YAML 1.2 core schema: `yes`/`on` are strings, not booleans; `~` and `null`
/// are null; integers stay integers and floats stay floats.
#[test]
fn scalar_types_keep_their_meaning() {
    let out = diff_against_empty(
        "\
guardrails:
  custom:
    as_int: 3
    as_float: 0.25
    as_exp: 1.0e-2
    as_true: true
    as_false: false
    as_null: null
    as_tilde: ~
    as_yes: yes
    as_on: on
    quoted_number: \"0.7\"
    single_quoted: 'it''s'
    with_comment: strict # trailing comment
    negative: -1
    big: 9007199254740993
",
    );
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert!(
        stdout(&out).contains(
            "  custom:\n    - (none)\n    + {\"as_exp\":0.01,\"as_false\":false,\
             \"as_float\":0.25,\"as_int\":3,\"as_null\":null,\"as_on\":\"on\",\
             \"as_tilde\":null,\"as_true\":true,\"as_yes\":\"yes\",\
             \"big\":9007199254740993,\"negative\":-1,\"quoted_number\":\"0.7\",\
             \"single_quoted\":\"it's\",\"with_comment\":\"strict\"}\n"
        ),
        "{}",
        stdout(&out)
    );
}

/// Flow style, block sequences, anchors and aliases are ordinary YAML that a
/// policy file written by hand or by another tool may use.
#[test]
fn flow_style_sequences_and_aliases_resolve() {
    let out = diff_against_empty(
        "\
defaults: &strict_level
  level: strict
guardrails:
  prompt_injection: *strict_level
  pii_detection: {level: moderate, mode: mask}
  blocked_topics:
    - weapons
    - \"self harm\"
  allowed_models: [gpt-4o, claude]
",
    );
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let s = stdout(&out);
    for line in [
        "  prompt_injection:\n    - (none)\n    + {\"level\":\"strict\"}\n",
        "  pii_detection:\n    - (none)\n    + {\"level\":\"moderate\",\"mode\":\"mask\"}\n",
        "  blocked_topics:\n    - (none)\n    + [\"weapons\",\"self harm\"]\n",
        "  allowed_models:\n    - (none)\n    + [\"gpt-4o\",\"claude\"]\n",
    ] {
        assert!(s.contains(line), "missing {line:?} in:\n{s}");
    }
}

/// An integer mapping key reaches the API as a JSON string key, as it did.
#[test]
fn non_string_keys_become_json_string_keys() {
    let out = diff_against_empty("guardrails:\n  custom:\n    1: one\n");
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert!(
        stdout(&out).contains("  custom:\n    - (none)\n    + {\"1\":\"one\"}\n"),
        "{}",
        stdout(&out)
    );
}

#[test]
fn validation_still_runs_on_parsed_yaml() {
    let out = diff_against_empty("guardrails:\n  prompt_injection:\n    level: extreme\n");
    assert!(!out.status.success());
    assert!(
        stderr(&out).contains("guardrails.prompt_injection.level: must be one of"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn a_file_without_guardrails_is_rejected() {
    for yaml in [
        "prompt_injection:\n  level: strict\n",
        "",
        "# only a comment\n",
    ] {
        let out = diff_against_empty(yaml);
        assert!(!out.status.success(), "{yaml:?} should be rejected");
        assert!(
            stderr(&out).contains("YAML must have a top-level 'guardrails' key"),
            "{yaml:?}: {}",
            stderr(&out)
        );
    }
}

#[test]
fn malformed_yaml_is_a_parse_error() {
    for yaml in [
        "guardrails:\n  prompt_injection: [unclosed\n",
        "guardrails:\n\tprompt_injection: {}\n",
        "guardrails:\n  a: 1\n  a: 2\n",
        "guardrails: {}\n---\nguardrails: {}\n",
    ] {
        let out = diff_against_empty(yaml);
        assert!(!out.status.success(), "{yaml:?} should be rejected");
        assert!(
            stderr(&out).contains("YAML parse error:"),
            "{yaml:?}: {}",
            stderr(&out)
        );
    }
}

/// `apply` reads through the same path as `diff`.
#[test]
fn apply_dry_run_reads_the_policy_file() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("policy.yaml"), DOCUMENTED_POLICY).unwrap();
    let url = serve_json(r#"{"guardrails": {"toxicity": {"threshold": 0.5}}}"#);
    let out = run_policy(&dir, &url, &["apply", "policy.yaml", "--dry-run"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let s = stdout(&out);
    assert!(s.contains("  toxicity.threshold: 0.5 -> 0.7\n"), "{s}");
    assert!(s.contains("(dry-run) No changes applied."), "{s}");
}

const LIVE_CONFIG: &str = r#"{"guardrails": {
    "prompt_injection": {"level": "strict", "enabled": true},
    "pii_detection": {"level": "moderate", "mode": "tokenize", "entities": ["email", "phone"]},
    "toxicity": {"threshold": 0.7, "max_retries": 3},
    "data_exfiltration": {"level": "permissive", "note": null},
    "custom": {"needs_quoting": "yes", "numeric_string": "0.5", "empty": "", "nested": {"list": []}, "multi": "a: b"}
}}"#;

/// `export` output is what users redirect into `policy.yaml` and commit, so
/// its exact shape is part of the contract. Note `needs_quoting: yes` is
/// written bare: under YAML 1.2 it reads back as the string, which
/// `export_round_trips_through_diff` holds the reader to.
#[test]
fn export_output_is_unchanged() {
    let dir = TempDir::new().unwrap();
    let url = serve_json(LIVE_CONFIG);
    let out = run_policy(&dir, &url, &["export"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert_eq!(
        stdout(&out),
        "\
guardrails:
  custom:
    empty: ''
    multi: 'a: b'
    needs_quoting: yes
    nested:
      list: []
    numeric_string: '0.5'
  data_exfiltration:
    level: permissive
    note: null
  pii_detection:
    entities:
    - email
    - phone
    level: moderate
    mode: tokenize
  prompt_injection:
    enabled: true
    level: strict
  toxicity:
    max_retries: 3
    threshold: 0.7
"
    );
}

/// What `export` writes, `diff` reads back as identical to the live config.
#[test]
fn export_round_trips_through_diff() {
    let dir = TempDir::new().unwrap();
    let url = serve_json(LIVE_CONFIG);
    let exported = run_policy(&dir, &url, &["export"]);
    assert!(exported.status.success(), "stderr: {}", stderr(&exported));
    std::fs::write(dir.path().join("policy.yaml"), &exported.stdout).unwrap();
    let out = run_policy(&dir, &url, &["diff", "policy.yaml"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert!(
        stdout(&out).contains("No differences - policy matches live config."),
        "{}",
        stdout(&out)
    );
}
