use super::core::{detect_in_file_generic, DetectorConfig};
use super::queries::get_python_detection_query;
use super::Detector;
use crate::error::Result;
use crate::types::{DetectionResult, Language, Provider};
use std::path::Path;

pub struct PythonDetector;

impl PythonDetector {
    pub fn new() -> Self {
        Self
    }

    fn check_has_base_url(
        source: &str,
        args_node: tree_sitter::Node,
        _provider: Provider,
    ) -> (bool, Option<String>) {
        let args_text = &source[args_node.start_byte()..args_node.end_byte()];
        let has_base_url = args_text.contains("base_url=") || args_text.contains("base_url =");

        let current_base_url = if has_base_url {
            Some("(configured)".to_string())
        } else {
            None
        };

        (has_base_url, current_base_url)
    }
}

impl Detector for PythonDetector {
    fn detect_in_file(&self, file_path: &Path, provider: Provider) -> Result<DetectionResult> {
        let config = DetectorConfig {
            ts_language: tree_sitter_python::language(),
            language: Language::Python,
            capture_name: "call_expr",
        };

        let query_str = get_python_detection_query(provider);

        detect_in_file_generic(
            file_path,
            provider,
            &config,
            query_str,
            Self::check_has_base_url,
        )
    }

    fn language(&self) -> Language {
        Language::Python
    }
}
