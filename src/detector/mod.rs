mod core;
mod python;
mod typescript;

pub use python::PythonDetector;
pub use typescript::TypeScriptDetector;

use crate::error::Result;
use crate::types::{DetectionResult, Language, Provider};
use std::path::Path;

pub trait Detector {
    fn detect_in_file(&self, file_path: &Path, provider: Provider) -> Result<DetectionResult>;
    fn language(&self) -> Language;
}

pub fn detect_all_providers(file_path: &Path) -> Result<Vec<(Provider, DetectionResult)>> {
    let ext = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");

    let language = Language::from_extension(ext);
    let Some(language) = language else {
        return Ok(Vec::new());
    };
    let providers = vec![
        Provider::OpenAI,
        Provider::Anthropic,
        Provider::Cohere,
        Provider::HuggingFace,
    ];

    let mut results = Vec::new();

    for provider in providers {
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
