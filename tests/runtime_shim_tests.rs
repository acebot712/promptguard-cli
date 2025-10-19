#![allow(clippy::unwrap_used, clippy::expect_used)]
/// Integration tests for runtime shim system
///
/// These tests verify the complete shim generation and injection workflow
/// to ensure 100% coverage of SDK calls in production environments.
use std::fs;
use tempfile::TempDir;

// Import from the main crate
use promptguard::shim::{ShimGenerator, ShimInjector};
use promptguard::types::{Language, Provider};

/// Test that Python shim is generated correctly
#[test]
fn test_python_shim_generation() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");

    let generator = ShimGenerator::new(
        temp_dir.path(),
        "https://api.promptguard.co/v1".to_string(),
        "PROMPTGUARD_API_KEY".to_string(),
        vec![Provider::OpenAI, Provider::Anthropic],
    );

    // Generate Python shim
    let shim_path = generator
        .generate_python_shim()
        .expect("Failed to generate Python shim");

    // Verify shim file was created
    assert!(shim_path.exists(), "Python shim file should exist");

    // Verify content
    let content = fs::read_to_string(&shim_path).expect("Failed to read shim file");

    // Should contain OpenAI patch
    assert!(
        content.contains("def _shim_openai()"),
        "Shim should contain OpenAI patch function"
    );

    // Should contain Anthropic patch
    assert!(
        content.contains("def _shim_anthropic()"),
        "Shim should contain Anthropic patch function"
    );

    // Should contain proxy URL
    assert!(
        content.contains("https://api.promptguard.co/v1"),
        "Shim should contain proxy URL"
    );

    // Should contain monkey-patching logic
    assert!(
        content.contains("openai.OpenAI = PatchedOpenAI"),
        "Shim should monkey-patch OpenAI"
    );

    // Verify __init__.py was created
    let init_path = temp_dir.path().join(".promptguard").join("__init__.py");
    assert!(init_path.exists(), "__init__.py should exist");
}

/// Test that TypeScript shim is generated correctly
#[test]
fn test_typescript_shim_generation() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");

    let generator = ShimGenerator::new(
        temp_dir.path(),
        "https://api.promptguard.co/v1".to_string(),
        "PROMPTGUARD_API_KEY".to_string(),
        vec![Provider::OpenAI],
    );

    // Generate TypeScript shim
    let shim_path = generator
        .generate_typescript_shim()
        .expect("Failed to generate TypeScript shim");

    // Verify shim file was created
    assert!(shim_path.exists(), "TypeScript shim file should exist");

    // Verify content
    let content = fs::read_to_string(&shim_path).expect("Failed to read shim file");

    // Should contain OpenAI wrapper
    assert!(
        content.contains("export class OpenAI"),
        "Shim should export OpenAI wrapper"
    );

    // Should contain proxy URL
    assert!(
        content.contains("https://api.promptguard.co/v1"),
        "Shim should contain proxy URL"
    );

    // Should contain ensureBaseURL logic
    assert!(
        content.contains("ensureBaseURL"),
        "Shim should contain ensureBaseURL function"
    );

    // Verify package.json was created
    let package_json = temp_dir.path().join(".promptguard").join("package.json");
    assert!(package_json.exists(), "package.json should exist");
}

/// Test that multiple shims are generated for multi-language projects
#[test]
fn test_multi_language_shim_generation() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");

    let generator = ShimGenerator::new(
        temp_dir.path(),
        "https://api.promptguard.co/v1".to_string(),
        "PROMPTGUARD_API_KEY".to_string(),
        vec![Provider::OpenAI, Provider::Anthropic, Provider::Cohere],
    );

    // Generate shims for both Python and TypeScript
    let languages = vec![Language::Python, Language::TypeScript];
    let shim_files = generator
        .generate_shims(&languages)
        .expect("Failed to generate shims");

    // Should have generated at least 2 files (Python + TypeScript)
    assert!(
        shim_files.len() >= 2,
        "Should generate shims for both languages"
    );

    // Verify Python shim exists and contains all providers
    let python_shim = temp_dir
        .path()
        .join(".promptguard")
        .join("promptguard_shim.py");
    assert!(python_shim.exists(), "Python shim should exist");

    let python_content = fs::read_to_string(&python_shim).expect("Failed to read Python shim");
    assert!(
        python_content.contains("_shim_openai"),
        "Python shim should have OpenAI"
    );
    assert!(
        python_content.contains("_shim_anthropic"),
        "Python shim should have Anthropic"
    );
    assert!(
        python_content.contains("_shim_cohere"),
        "Python shim should have Cohere"
    );

    // Verify README was created
    let readme = temp_dir.path().join(".promptguard").join("README.md");
    assert!(readme.exists(), "README should exist");
}

/// Test Python entry point detection
#[test]
fn test_python_entry_point_detection() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");

    // Create main.py
    fs::write(
        temp_dir.path().join("main.py"),
        "#!/usr/bin/env python3\nprint('Hello')",
    )
    .expect("Failed to create main.py");

    // Create app.py with __main__ check
    fs::write(
        temp_dir.path().join("app.py"),
        "if __name__ == \"__main__\":\n    print('App')",
    )
    .expect("Failed to create app.py");

    // Create a non-entry file
    fs::write(temp_dir.path().join("utils.py"), "def helper(): pass")
        .expect("Failed to create utils.py");

    let injector = ShimInjector::new(temp_dir.path());
    let entry_points = injector
        .detect_python_entry_points()
        .expect("Failed to detect entry points");

    // Should find main.py and app.py
    assert!(
        entry_points.len() >= 2,
        "Should detect at least 2 entry points"
    );

    let has_main = entry_points
        .iter()
        .any(|p| p.file_name().unwrap() == "main.py");
    let has_app = entry_points
        .iter()
        .any(|p| p.file_name().unwrap() == "app.py");

    assert!(has_main, "Should detect main.py");
    assert!(has_app, "Should detect app.py with __main__");
}

/// Test Python shim injection
#[test]
fn test_python_shim_injection() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");

    // Create a test Python file
    let test_file = temp_dir.path().join("main.py");
    fs::write(
        &test_file,
        "#!/usr/bin/env python3\n\nfrom openai import OpenAI\n\nclient = OpenAI()",
    )
    .expect("Failed to create test file");

    let injector = ShimInjector::new(temp_dir.path());

    // Inject shim import
    let injected = injector
        .inject_python_shim(&test_file)
        .expect("Failed to inject shim");

    assert!(injected, "Should report injection success");

    // Verify injection
    let content = fs::read_to_string(&test_file).expect("Failed to read file");

    assert!(
        content.contains("# PromptGuard runtime shim - auto-injected"),
        "Should contain shim marker"
    );
    assert!(
        content.contains("import promptguard_shim"),
        "Should import shim"
    );
    assert!(
        content.contains("sys.path.insert"),
        "Should add shim to path"
    );

    // Shim should be injected after shebang
    let lines: Vec<&str> = content.lines().collect();
    assert!(lines[0].starts_with("#!"), "First line should be shebang");
    assert!(
        lines[1].contains("PromptGuard") || lines[2].contains("PromptGuard"),
        "Shim should be near the top"
    );
}

/// Test Python shim removal
#[test]
fn test_python_shim_removal() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");

    // Create a test file
    let test_file = temp_dir.path().join("main.py");
    let original_content =
        "#!/usr/bin/env python3\n\nfrom openai import OpenAI\n\nclient = OpenAI()";
    fs::write(&test_file, original_content).expect("Failed to create test file");

    let injector = ShimInjector::new(temp_dir.path());

    // Inject then remove
    injector
        .inject_python_shim(&test_file)
        .expect("Failed to inject");
    let removed = injector
        .remove_python_shim(&test_file)
        .expect("Failed to remove");

    assert!(removed, "Should report removal success");

    // Verify removal
    let content = fs::read_to_string(&test_file).expect("Failed to read file");
    assert!(
        !content.contains("PromptGuard"),
        "Should not contain shim marker"
    );
    assert!(
        !content.contains("import promptguard_shim"),
        "Should not import shim"
    );
}

/// Test idempotent injection (don't inject twice)
#[test]
fn test_idempotent_shim_injection() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");

    let test_file = temp_dir.path().join("main.py");
    fs::write(&test_file, "print('hello')").expect("Failed to create test file");

    let injector = ShimInjector::new(temp_dir.path());

    // First injection
    let first = injector
        .inject_python_shim(&test_file)
        .expect("Failed to inject");
    assert!(first, "First injection should succeed");

    let content_after_first = fs::read_to_string(&test_file).expect("Failed to read file");
    let first_count = content_after_first.matches("PromptGuard").count();

    // Second injection should not duplicate
    let second = injector
        .inject_python_shim(&test_file)
        .expect("Failed to inject");
    assert!(!second, "Second injection should report already injected");

    let content_after_second = fs::read_to_string(&test_file).expect("Failed to read file");
    let second_count = content_after_second.matches("PromptGuard").count();

    assert_eq!(
        first_count, second_count,
        "Should not inject multiple times"
    );
}

/// Test shim cleanup
#[test]
fn test_shim_cleanup() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");

    let generator = ShimGenerator::new(
        temp_dir.path(),
        "https://api.promptguard.co/v1".to_string(),
        "PROMPTGUARD_API_KEY".to_string(),
        vec![Provider::OpenAI],
    );

    // Generate shims
    generator
        .generate_python_shim()
        .expect("Failed to generate shim");

    assert!(
        generator.shims_installed(),
        "Shims should be installed after generation"
    );

    // Clean up
    generator.clean_shims().expect("Failed to clean shims");

    assert!(
        !generator.shims_installed(),
        "Shims should not be installed after cleanup"
    );

    // Verify directory is gone
    let shim_dir = temp_dir.path().join(".promptguard");
    assert!(!shim_dir.exists(), "Shim directory should be removed");
}

/// Test TypeScript entry point detection
#[test]
fn test_typescript_entry_point_detection() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");

    // Create package.json with main field
    fs::write(
        temp_dir.path().join("package.json"),
        r#"{"main": "dist/index.js", "scripts": {"start": "node dist/server.js"}}"#,
    )
    .expect("Failed to create package.json");

    // Create the referenced files
    fs::create_dir_all(temp_dir.path().join("dist")).expect("Failed to create dist");
    fs::write(
        temp_dir.path().join("dist").join("index.js"),
        "console.log('main')",
    )
    .expect("Failed to create index.js");
    fs::write(
        temp_dir.path().join("dist").join("server.js"),
        "console.log('server')",
    )
    .expect("Failed to create server.js");

    // Create src/index.ts
    fs::create_dir_all(temp_dir.path().join("src")).expect("Failed to create src");
    fs::write(
        temp_dir.path().join("src").join("index.ts"),
        "console.log('typescript')",
    )
    .expect("Failed to create index.ts");

    let injector = ShimInjector::new(temp_dir.path());
    let entry_points = injector
        .detect_typescript_entry_points()
        .expect("Failed to detect entry points");

    // Should find multiple entry points
    assert!(
        !entry_points.is_empty(),
        "Should detect TypeScript entry points"
    );
}

/// Test that shim works with all supported providers
#[test]
fn test_all_providers_in_shim() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");

    let all_providers = vec![
        Provider::OpenAI,
        Provider::Anthropic,
        Provider::Cohere,
        Provider::HuggingFace,
    ];

    let generator = ShimGenerator::new(
        temp_dir.path(),
        "https://api.promptguard.co/v1".to_string(),
        "PROMPTGUARD_API_KEY".to_string(),
        all_providers.clone(),
    );

    // Generate Python shim
    let shim_path = generator
        .generate_python_shim()
        .expect("Failed to generate shim");

    let content = fs::read_to_string(&shim_path).expect("Failed to read shim");

    // Verify all providers are included
    for provider in all_providers {
        let provider_name = match provider {
            Provider::OpenAI => "openai",
            Provider::Anthropic => "anthropic",
            Provider::Cohere => "cohere",
            Provider::HuggingFace => "huggingface",
        };

        assert!(
            content.contains(&format!("_shim_{provider_name}")),
            "Shim should contain {provider_name} patch"
        );
    }
}

/// Test shim with custom proxy URL
#[test]
fn test_custom_proxy_url() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let custom_url = "https://custom.proxy.example.com/api/v2";

    let generator = ShimGenerator::new(
        temp_dir.path(),
        custom_url.to_string(),
        "MY_API_KEY".to_string(),
        vec![Provider::OpenAI],
    );

    let shim_path = generator
        .generate_python_shim()
        .expect("Failed to generate shim");

    let content = fs::read_to_string(&shim_path).expect("Failed to read shim");

    assert!(
        content.contains(custom_url),
        "Shim should use custom proxy URL"
    );
    assert!(
        content.contains("MY_API_KEY"),
        "Shim should use custom API key var"
    );
}
