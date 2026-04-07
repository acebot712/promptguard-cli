#![allow(clippy::unwrap_used, clippy::expect_used, dead_code)]
//! Contract tests that validate CLI types against api-contract.json.
//!
//! These tests ensure that `ErrorDetail` deserialization, request
//! field names, and response shapes stay in sync with the backend API.

use serde::Deserialize;
use std::collections::HashMap;
use std::fs;

/// Mirrors the CLI's internal `ErrorResponse` for deserialization testing.
#[derive(Debug, Deserialize)]
struct ErrorResponse {
    error: ErrorDetail,
}

/// Mirrors the CLI's internal `ErrorDetail` struct.
#[derive(Debug, Deserialize)]
struct ErrorDetail {
    code: String,
    message: String,
    #[serde(rename = "type")]
    error_type: Option<String>,
    upgrade_url: Option<String>,
    current_plan: Option<String>,
    requests_used: Option<u64>,
    requests_limit: Option<u64>,
}

/// Mirrors the CLI's `RedactResponse` struct.
#[derive(Debug, Deserialize)]
struct RedactResponse {
    original: String,
    redacted: String,
    #[serde(default, rename = "piiFound")]
    pii_found: Vec<String>,
}

/// Mirrors the CLI's `SecurityScanResponse` struct.
#[derive(Debug, Deserialize)]
struct SecurityScanResponse {
    blocked: bool,
    decision: String,
    confidence: f64,
    reason: String,
    #[serde(default, rename = "threatType")]
    threat_type: Option<String>,
    #[serde(default, rename = "eventId")]
    event_id: Option<String>,
    #[serde(default, rename = "processingTimeMs")]
    processing_time_ms: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct Contract {
    error_responses: ErrorResponsesSection,
    security_scan: EndpointSection,
    security_redact: EndpointSection,
    sdk_headers: SdkHeadersSection,
}

#[derive(Debug, Deserialize)]
struct ErrorResponsesSection {
    cases: Vec<ErrorCase>,
}

#[derive(Debug, Deserialize)]
struct ErrorCase {
    name: String,
    status_code: u16,
    body: serde_json::Value,
    #[serde(default)]
    expect: Option<HashMap<String, serde_json::Value>>,
    #[serde(default)]
    expect_has_detail: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct EndpointSection {
    request_fields: FieldSpec,
    response_fields: FieldSpec,
    #[serde(default)]
    cases: Vec<EndpointCase>,
}

#[derive(Debug, Deserialize)]
struct FieldSpec {
    required: Vec<String>,
    #[serde(default)]
    optional: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct EndpointCase {
    name: String,
    #[serde(default)]
    response: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct SdkHeadersSection {
    required_headers: HashMap<String, String>,
    recommended_headers: HashMap<String, String>,
}

fn load_contract() -> Contract {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/api-contract.json");
    let data = fs::read_to_string(path).expect("Failed to read api-contract.json");
    serde_json::from_str(&data).expect("Failed to parse api-contract.json")
}

// ── Error deserialization ──────────────────────────────────────────────

#[test]
fn error_detail_deserializes_all_contract_cases() {
    let contract = load_contract();

    for case in &contract.error_responses.cases {
        if case.expect_has_detail == Some(true) {
            continue;
        }

        let body_str = serde_json::to_string(&case.body).unwrap();
        let parsed: Result<ErrorResponse, _> = serde_json::from_str(&body_str);
        assert!(
            parsed.is_ok(),
            "Failed to deserialize error case '{}': {:?}",
            case.name,
            parsed.err()
        );

        let detail = parsed.unwrap().error;
        let expect = case.expect.as_ref().expect("missing expect");

        assert_eq!(
            detail.code,
            expect["code"].as_str().unwrap(),
            "code mismatch in '{}'",
            case.name
        );
        assert_eq!(
            detail.message,
            expect["message"].as_str().unwrap(),
            "message mismatch in '{}'",
            case.name
        );

        if let Some(t) = expect.get("type") {
            assert_eq!(
                detail.error_type.as_deref(),
                t.as_str(),
                "type mismatch in '{}'",
                case.name
            );
        }
        if let Some(url) = expect.get("upgrade_url") {
            assert_eq!(
                detail.upgrade_url.as_deref(),
                url.as_str(),
                "upgrade_url mismatch in '{}'",
                case.name
            );
        }
        if let Some(plan) = expect.get("current_plan") {
            assert_eq!(
                detail.current_plan.as_deref(),
                plan.as_str(),
                "current_plan mismatch in '{}'",
                case.name
            );
        }
        if let Some(used) = expect.get("requests_used") {
            assert_eq!(
                detail.requests_used,
                used.as_u64(),
                "requests_used mismatch in '{}'",
                case.name
            );
        }
        if let Some(limit) = expect.get("requests_limit") {
            assert_eq!(
                detail.requests_limit,
                limit.as_u64(),
                "requests_limit mismatch in '{}'",
                case.name
            );
        }
    }
}

// ── Scan response deserialization ──────────────────────────────────────

#[test]
fn scan_response_deserializes_contract_cases() {
    let contract = load_contract();

    for case in &contract.security_scan.cases {
        if case.response.is_null() {
            continue;
        }
        let resp_str = serde_json::to_string(&case.response).unwrap();
        let parsed: Result<SecurityScanResponse, _> = serde_json::from_str(&resp_str);
        assert!(
            parsed.is_ok(),
            "Failed to deserialize scan case '{}': {:?}",
            case.name,
            parsed.err()
        );
    }
}

// ── Redact response deserialization ────────────────────────────────────

#[test]
fn redact_response_deserializes_contract_cases() {
    let contract = load_contract();

    for case in &contract.security_redact.cases {
        if case.response.is_null() {
            continue;
        }
        let resp_str = serde_json::to_string(&case.response).unwrap();
        let parsed: Result<RedactResponse, _> = serde_json::from_str(&resp_str);
        assert!(
            parsed.is_ok(),
            "Failed to deserialize redact case '{}': {:?}",
            case.name,
            parsed.err()
        );
    }
}

// ── Request field contract ─────────────────────────────────────────────

#[test]
fn scan_request_uses_content_not_text() {
    let contract = load_contract();
    let required = &contract.security_scan.request_fields.required;
    assert!(
        required.contains(&"content".to_string()),
        "scan request should require 'content'"
    );
    assert!(
        !required.contains(&"text".to_string()),
        "scan request should NOT use 'text'"
    );
}

#[test]
fn redact_request_uses_content_not_text() {
    let contract = load_contract();
    let required = &contract.security_redact.request_fields.required;
    assert!(
        required.contains(&"content".to_string()),
        "redact request should require 'content'"
    );
    assert!(
        !required.contains(&"text".to_string()),
        "redact request should NOT use 'text'"
    );
}

// ── SDK header contract ────────────────────────────────────────────────

#[test]
fn contract_requires_x_api_key_header() {
    let contract = load_contract();
    assert!(
        contract
            .sdk_headers
            .required_headers
            .contains_key("X-API-Key"),
        "X-API-Key must be a required header"
    );
}
