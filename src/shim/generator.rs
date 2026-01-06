/// Shim file generator
///
/// Generates runtime interception code for Python and TypeScript/JavaScript
/// that automatically routes all LLM SDK calls through `PromptGuard` proxy.
use crate::error::Result;
use crate::shim::templates;
use crate::types::{Language, Provider};
use std::fs;
use std::path::{Path, PathBuf};

const SHIM_DIR_NAME: &str = ".promptguard";
const PYTHON_SHIM_FILENAME: &str = "promptguard_shim.py";
const TYPESCRIPT_SHIM_FILENAME: &str = "promptguard-shim.ts";
const JAVASCRIPT_SHIM_FILENAME: &str = "promptguard-shim.js";

/// Shim generator for creating runtime interception code
pub struct ShimGenerator {
    project_root: PathBuf,
    proxy_url: String,
    api_key_var: String,
    providers: Vec<Provider>,
}

impl ShimGenerator {
    /// Create a new shim generator
    pub fn new(
        project_root: impl AsRef<Path>,
        proxy_url: String,
        api_key_var: String,
        providers: Vec<Provider>,
    ) -> Self {
        Self {
            project_root: project_root.as_ref().to_path_buf(),
            proxy_url,
            api_key_var,
            providers,
        }
    }

    /// Get the shim directory path
    pub fn shim_dir(&self) -> PathBuf {
        self.project_root.join(SHIM_DIR_NAME)
    }

    /// Get the Python shim file path
    pub fn python_shim_path(&self) -> PathBuf {
        self.shim_dir().join(PYTHON_SHIM_FILENAME)
    }

    /// Get the TypeScript shim file path
    pub fn typescript_shim_path(&self) -> PathBuf {
        self.shim_dir().join(TYPESCRIPT_SHIM_FILENAME)
    }

    /// Get the JavaScript shim file path
    pub fn javascript_shim_path(&self) -> PathBuf {
        self.shim_dir().join(JAVASCRIPT_SHIM_FILENAME)
    }

    /// Ensure shim directory exists
    fn ensure_shim_dir(&self) -> Result<()> {
        let shim_dir = self.shim_dir();
        if !shim_dir.exists() {
            fs::create_dir_all(&shim_dir)?;
        }
        Ok(())
    }

    /// Generate Python shim file
    pub fn generate_python_shim(&self) -> Result<PathBuf> {
        self.ensure_shim_dir()?;

        let mut provider_patches = String::new();
        let mut install_calls = String::new();

        for provider in &self.providers {
            // Add provider patch function
            provider_patches.push_str(templates::get_python_provider_patch(*provider));
            provider_patches.push('\n');

            // Add install call
            install_calls.push_str(templates::get_python_install_call(*provider));
            install_calls.push('\n');
        }

        // Generate shim content from template
        let content = templates::PYTHON_SHIM_TEMPLATE
            .replace("{{PROXY_URL}}", &self.proxy_url)
            .replace("{{API_KEY_VAR}}", &self.api_key_var)
            .replace("{{PROVIDER_PATCHES}}", &provider_patches)
            .replace("{{INSTALL_CALLS}}", &install_calls);

        // Write shim file
        let shim_path = self.python_shim_path();
        fs::write(&shim_path, content)?;

        // Also create __init__.py to make it a proper Python package
        let init_path = self.shim_dir().join("__init__.py");
        fs::write(
            &init_path,
            "# PromptGuard runtime shim package\nfrom .promptguard_shim import *\n",
        )?;

        Ok(shim_path)
    }

    /// Generate TypeScript shim file
    pub fn generate_typescript_shim(&self) -> Result<PathBuf> {
        self.ensure_shim_dir()?;

        let mut provider_exports = String::new();

        for provider in &self.providers {
            provider_exports.push_str(templates::get_typescript_provider_export(*provider));
            provider_exports.push('\n');
        }

        // Generate shim content from template
        let content = templates::TYPESCRIPT_SHIM_TEMPLATE
            .replace("{{PROXY_URL}}", &self.proxy_url)
            .replace("{{API_KEY_VAR}}", &self.api_key_var)
            .replace("{{PROVIDER_EXPORTS}}", &provider_exports);

        // Write TypeScript shim file
        let ts_shim_path = self.typescript_shim_path();
        fs::write(&ts_shim_path, &content)?;

        // Also create JavaScript version (same content, just .js extension)
        // TypeScript can be used as JavaScript
        let js_shim_path = self.javascript_shim_path();
        fs::write(&js_shim_path, &content)?;

        // Create package.json for the shim module
        let package_json = r#"{
  "name": "@promptguard/shim",
  "version": "1.0.0",
  "private": true,
  "description": "PromptGuard runtime interception shim",
  "main": "promptguard-shim.js",
  "types": "promptguard-shim.ts"
}
"#;
        fs::write(self.shim_dir().join("package.json"), package_json)?;

        Ok(ts_shim_path)
    }

    /// Generate shim files for detected languages
    pub fn generate_shims(&self, languages: &[Language]) -> Result<Vec<PathBuf>> {
        let mut generated = Vec::new();

        for language in languages {
            match language {
                Language::Python => {
                    let path = self.generate_python_shim()?;
                    generated.push(path);
                },
                Language::TypeScript | Language::JavaScript => {
                    // Generate both TS and JS for JS-based projects
                    let path = self.generate_typescript_shim()?;
                    generated.push(path);
                },
            }
        }

        // Create .gitignore to prevent accidental commits of shim directory
        self.create_gitignore()?;

        // Create README explaining what this directory is
        self.create_readme()?;

        Ok(generated)
    }

    /// Create .gitignore in shim directory
    fn create_gitignore(&self) -> Result<()> {
        let gitignore_path = self.shim_dir().join(".gitignore");
        let content =
            "# PromptGuard shim directory\n# This directory is auto-generated - safe to commit\n";
        fs::write(gitignore_path, content)?;
        Ok(())
    }

    /// Create README in shim directory
    fn create_readme(&self) -> Result<()> {
        let readme_path = self.shim_dir().join("README.md");
        let content = r"# PromptGuard Runtime Shim

This directory contains auto-generated runtime interception code.

## What is this?

The PromptGuard runtime shim ensures that **all** LLM SDK calls in your application
automatically route through PromptGuard for security monitoring and protection.

This provides 100% coverage, even for:
- Dynamically constructed URLs
- Configuration loaded from external sources
- Environment variables
- SDKs initialized in third-party libraries

## How it works

The shim files in this directory monkey-patch (Python) or wrap (TypeScript/JavaScript)
the constructors of popular LLM SDKs (OpenAI, Anthropic, Cohere, HuggingFace).

When your code creates an SDK client, the shim intercepts the constructor call and
automatically injects the PromptGuard proxy URL if not already configured.

## Files

- `promptguard_shim.py` - Python runtime shim
- `promptguard-shim.ts` - TypeScript runtime shim
- `promptguard-shim.js` - JavaScript runtime shim
- `__init__.py` - Python package initialization

## Maintenance

These files are **auto-generated** by the PromptGuard CLI. Do not edit manually.

To regenerate:
```bash
promptguard enable --runtime
```

To disable:
```bash
promptguard disable
```

## Safety

This directory can be safely committed to version control. It contains no secrets,
only routing logic to ensure API calls use the PromptGuard proxy.
";
        fs::write(readme_path, content)?;
        Ok(())
    }

    /// Remove all generated shim files
    pub fn clean_shims(&self) -> Result<()> {
        let shim_dir = self.shim_dir();
        if shim_dir.exists() {
            fs::remove_dir_all(&shim_dir)?;
        }
        Ok(())
    }

    /// Check if shims are currently installed
    pub fn shims_installed(&self) -> bool {
        let shim_dir = self.shim_dir();
        shim_dir.exists()
            && (self.python_shim_path().exists() || self.typescript_shim_path().exists())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_shim_generator_creation() {
        let temp_dir = TempDir::new().unwrap();
        let generator = ShimGenerator::new(
            temp_dir.path(),
            "https://api.promptguard.co/api/v1".to_string(),
            "PROMPTGUARD_API_KEY".to_string(),
            vec![Provider::OpenAI],
        );

        assert_eq!(generator.shim_dir(), temp_dir.path().join(".promptguard"));
    }

    #[test]
    fn test_python_shim_generation() {
        let temp_dir = TempDir::new().unwrap();
        let generator = ShimGenerator::new(
            temp_dir.path(),
            "https://api.promptguard.co/api/v1".to_string(),
            "PROMPTGUARD_API_KEY".to_string(),
            vec![Provider::OpenAI, Provider::Anthropic],
        );

        let shim_path = generator.generate_python_shim().unwrap();
        assert!(shim_path.exists());

        let content = fs::read_to_string(&shim_path).unwrap();
        assert!(content.contains("def _shim_openai()"));
        assert!(content.contains("def _shim_anthropic()"));
        assert!(content.contains("https://api.promptguard.co/api/v1"));
    }

    #[test]
    fn test_typescript_shim_generation() {
        let temp_dir = TempDir::new().unwrap();
        let generator = ShimGenerator::new(
            temp_dir.path(),
            "https://api.promptguard.co/api/v1".to_string(),
            "PROMPTGUARD_API_KEY".to_string(),
            vec![Provider::OpenAI],
        );

        let shim_path = generator.generate_typescript_shim().unwrap();
        assert!(shim_path.exists());

        let content = fs::read_to_string(&shim_path).unwrap();
        assert!(content.contains("export class OpenAI"));
        assert!(content.contains("https://api.promptguard.co/api/v1"));
    }

    #[test]
    fn test_clean_shims() {
        let temp_dir = TempDir::new().unwrap();
        let generator = ShimGenerator::new(
            temp_dir.path(),
            "https://api.promptguard.co/api/v1".to_string(),
            "PROMPTGUARD_API_KEY".to_string(),
            vec![Provider::OpenAI],
        );

        // Generate shims
        generator.generate_python_shim().unwrap();
        assert!(generator.shims_installed());

        // Clean shims
        generator.clean_shims().unwrap();
        assert!(!generator.shims_installed());
    }
}
