#![allow(clippy::unwrap_used, clippy::expect_used)]
/// Integration tests for environment variable scanner
///
/// These tests verify that the environment scanner correctly detects
/// and reports API-related environment variables in projects.
use std::fs;
use tempfile::TempDir;

use promptguard::analyzer::EnvScanner;

/// Test scanning .env files
#[test]
fn test_find_env_files() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");

    // Create multiple .env files
    fs::write(temp_dir.path().join(".env"), "KEY=value").expect("Failed to create .env");
    fs::write(temp_dir.path().join(".env.local"), "KEY=local")
        .expect("Failed to create .env.local");
    fs::write(temp_dir.path().join(".env.production"), "KEY=prod")
        .expect("Failed to create .env.production");

    let scanner = EnvScanner::new(temp_dir.path());
    let env_files = scanner.find_env_files().expect("Failed to find env files");

    assert!(env_files.len() >= 3, "Should find at least 3 .env files");
}

/// Test parsing .env file with various formats
#[test]
fn test_parse_env_file() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let env_file = temp_dir.path().join(".env");

    let content = r#"
# Comment line
OPENAI_API_KEY=sk-test123
OPENAI_BASE_URL="https://api.example.com"
ANTHROPIC_API_KEY='claude-key'

# Empty line above
DATABASE_URL=postgres://localhost
"#;

    fs::write(&env_file, content).expect("Failed to create .env file");

    let scanner = EnvScanner::new(temp_dir.path());
    let vars = scanner
        .parse_env_file(&env_file)
        .expect("Failed to parse env file");

    assert_eq!(vars.len(), 4, "Should parse 4 variables");

    // Verify OPENAI_API_KEY
    let openai_key = vars.iter().find(|v| v.name == "OPENAI_API_KEY").unwrap();
    assert_eq!(openai_key.value.as_ref().unwrap(), "sk-test123");

    // Verify quoted value is unquoted
    let openai_url = vars.iter().find(|v| v.name == "OPENAI_BASE_URL").unwrap();
    assert_eq!(
        openai_url.value.as_ref().unwrap(),
        "https://api.example.com"
    );

    // Verify single-quoted value
    let anthropic_key = vars.iter().find(|v| v.name == "ANTHROPIC_API_KEY").unwrap();
    assert_eq!(anthropic_key.value.as_ref().unwrap(), "claude-key");
}

/// Test finding API-related variables
#[test]
fn test_find_api_related_vars() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let env_file = temp_dir.path().join(".env");

    let content = r"
OPENAI_API_KEY=sk-test
OPENAI_BASE_URL=https://api.openai.com
DATABASE_URL=postgres://localhost
NODE_ENV=development
ANTHROPIC_API_KEY=claude-key
RANDOM_VARIABLE=some_value
COHERE_API_TOKEN=cohere-token
";

    fs::write(&env_file, content).expect("Failed to create .env");

    let scanner = EnvScanner::new(temp_dir.path());
    let api_vars = scanner
        .find_api_related_vars()
        .expect("Failed to find API vars");

    // Should find API-related vars but not DATABASE_URL, NODE_ENV, RANDOM_VARIABLE
    assert!(
        api_vars.len() >= 4,
        "Should find at least 4 API-related variables"
    );

    let var_names: Vec<String> = api_vars.iter().map(|v| v.name.clone()).collect();

    assert!(var_names.contains(&"OPENAI_API_KEY".to_string()));
    assert!(var_names.contains(&"OPENAI_BASE_URL".to_string()));
    assert!(var_names.contains(&"ANTHROPIC_API_KEY".to_string()));
    assert!(var_names.contains(&"COHERE_API_TOKEN".to_string()));
}

/// Test scanning Python code for environment variable usage
#[test]
fn test_scan_python_env_usage() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");

    // Create Python file with env var usage
    let py_file = temp_dir.path().join("config.py");
    let content = r#"
import os

api_key = os.environ["OPENAI_API_KEY"]
base_url = os.getenv("OPENAI_BASE_URL")
secret = os.environ.get("SECRET_TOKEN", "default")
"#;

    fs::write(&py_file, content).expect("Failed to create Python file");

    let scanner = EnvScanner::new(temp_dir.path());
    let usages = scanner
        .scan_python_env_usage()
        .expect("Failed to scan Python usage");

    assert!(usages.len() >= 3, "Should find at least 3 env var usages");

    let var_names: Vec<String> = usages.iter().map(|u| u.var_name.clone()).collect();

    assert!(var_names.contains(&"OPENAI_API_KEY".to_string()));
    assert!(var_names.contains(&"OPENAI_BASE_URL".to_string()));
    assert!(var_names.contains(&"SECRET_TOKEN".to_string()));
}

/// Test scanning TypeScript code for environment variable usage
#[test]
fn test_scan_typescript_env_usage() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");

    // Create TypeScript file with env var usage
    let ts_file = temp_dir.path().join("config.ts");
    let content = r#"
const apiKey = process.env.OPENAI_API_KEY;
const baseURL = process.env["ANTHROPIC_BASE_URL"];
const token = process.env['COHERE_TOKEN'];
"#;

    fs::write(&ts_file, content).expect("Failed to create TypeScript file");

    let scanner = EnvScanner::new(temp_dir.path());
    let usages = scanner
        .scan_typescript_env_usage()
        .expect("Failed to scan TypeScript usage");

    assert!(usages.len() >= 3, "Should find at least 3 env var usages");

    let var_names: Vec<String> = usages.iter().map(|u| u.var_name.clone()).collect();

    assert!(var_names.contains(&"OPENAI_API_KEY".to_string()));
    assert!(var_names.contains(&"ANTHROPIC_BASE_URL".to_string()));
    assert!(var_names.contains(&"COHERE_TOKEN".to_string()));
}

/// Test environment variable report generation
#[test]
fn test_generate_env_report() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");

    // Create .env file
    let env_file = temp_dir.path().join(".env");
    fs::write(&env_file, "OPENAI_API_KEY=sk-test\n").expect("Failed to create .env");

    // Create Python file using env var
    let py_file = temp_dir.path().join("app.py");
    fs::write(&py_file, "import os\nkey = os.getenv('OPENAI_API_KEY')\n")
        .expect("Failed to create Python file");

    let scanner = EnvScanner::new(temp_dir.path());
    let report = scanner
        .generate_report()
        .expect("Failed to generate report");

    assert!(
        report.contains("OPENAI_API_KEY"),
        "Report should mention OPENAI_API_KEY"
    );
    assert!(
        report.contains("Defined in .env"),
        "Report should show where defined"
    );
    assert!(
        report.contains("Used in app.py"),
        "Report should show where used"
    );
}

/// Test empty project (no env vars)
#[test]
fn test_empty_project_env_scan() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");

    let scanner = EnvScanner::new(temp_dir.path());
    let report = scanner
        .generate_report()
        .expect("Failed to generate report");

    assert!(
        report.contains("No environment variables"),
        "Should report no env vars"
    );
}

/// Test that scanner ignores non-source directories
#[test]
fn test_scanner_ignores_build_dirs() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");

    // Create .env in root (should be found)
    fs::write(temp_dir.path().join(".env"), "ROOT_VAR=value").expect("Failed to create .env");

    // Create .env in node_modules (should be ignored)
    fs::create_dir_all(temp_dir.path().join("node_modules"))
        .expect("Failed to create node_modules");
    fs::write(
        temp_dir.path().join("node_modules").join(".env"),
        "NODE_MODULES_VAR=value",
    )
    .expect("Failed to create node_modules .env");

    // Create .env in venv (should be ignored)
    fs::create_dir_all(temp_dir.path().join("venv")).expect("Failed to create venv");
    fs::write(temp_dir.path().join("venv").join(".env"), "VENV_VAR=value")
        .expect("Failed to create venv .env");

    let scanner = EnvScanner::new(temp_dir.path());
    let vars = scanner
        .scan_env_variables()
        .expect("Failed to scan env variables");

    // Should only find ROOT_VAR, not vars in ignored directories
    assert_eq!(vars.len(), 1, "Should only find root .env file");
    assert_eq!(vars[0].name, "ROOT_VAR");
}

/// Test multi-language project with env vars
#[test]
fn test_multi_language_env_detection() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");

    // Create .env file
    fs::write(
        temp_dir.path().join(".env"),
        "SHARED_API_KEY=sk-test\nSHARED_URL=https://api.example.com\n",
    )
    .expect("Failed to create .env");

    // Create Python file
    let py_file = temp_dir.path().join("backend.py");
    fs::write(&py_file, "import os\nkey = os.getenv('SHARED_API_KEY')\n")
        .expect("Failed to create Python file");

    // Create TypeScript file
    let ts_file = temp_dir.path().join("frontend.ts");
    fs::write(&ts_file, "const url = process.env.SHARED_URL;\n")
        .expect("Failed to create TypeScript file");

    let scanner = EnvScanner::new(temp_dir.path());

    // Scan both languages
    let py_usage = scanner
        .scan_python_env_usage()
        .expect("Failed to scan Python");
    let ts_usage = scanner
        .scan_typescript_env_usage()
        .expect("Failed to scan TypeScript");

    // Should find usage in both languages
    assert!(!py_usage.is_empty(), "Should find Python env var usage");
    assert!(!ts_usage.is_empty(), "Should find TypeScript env var usage");

    // Generate comprehensive report
    let report = scanner
        .generate_report()
        .expect("Failed to generate report");

    assert!(report.contains("SHARED_API_KEY"));
    assert!(report.contains("SHARED_URL"));
    assert!(report.contains("backend.py"));
    assert!(report.contains("frontend.ts"));
}
