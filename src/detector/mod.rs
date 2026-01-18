mod core;
mod python;
pub mod queries;
mod registry;
mod typescript;

pub use python::PythonDetector;
pub use queries::{get_python_transform_query, get_typescript_query};
pub use registry::PROVIDERS;
pub use typescript::TypeScriptDetector;

use crate::error::Result;
use crate::types::{DetectionResult, Language, Provider};
use std::path::Path;

pub trait Detector {
    fn detect_in_file(&self, file_path: &Path, provider: Provider) -> Result<DetectionResult>;

    /// Returns the language this detector handles (for trait completeness)
    #[allow(dead_code)]
    fn language(&self) -> Language;
}

pub fn detect_all_providers(file_path: &Path) -> Result<Vec<(Provider, DetectionResult)>> {
    let ext = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");

    let language = Language::from_extension(ext);
    let Some(language) = language else {
        return Ok(Vec::new());
    };

    // Use registry instead of hardcoded list
    let mut results = Vec::new();

    for provider_info in PROVIDERS {
        let provider = provider_info.provider;
        let result = match language {
            Language::TypeScript | Language::JavaScript => {
                let detector = TypeScriptDetector::new();
                detector.detect_in_file(file_path, provider)?
            },
            Language::Python => {
                let detector = PythonDetector::new();
                detector.detect_in_file(file_path, provider)?
            },
        };

        if !result.instances.is_empty() {
            results.push((provider, result));
        }
    }

    Ok(results)
}
