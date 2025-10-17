use crate::detector::Detector;
use crate::error::{PromptGuardError, Result};
use crate::types::{DetectionInstance, DetectionResult, Language, Provider};
use std::fs;
use std::path::Path;
use tree_sitter::{Parser, Query, QueryCursor};

pub struct PythonDetector;

impl PythonDetector {
    pub fn new() -> Self {
        Self
    }

    fn get_query_for_provider(&self, provider: Provider) -> &'static str {
        match provider {
            Provider::OpenAI => r#"
                (call
                    function: (identifier) @function
                    (#eq? @function "OpenAI")
                    arguments: (argument_list) @args
                ) @call_expr
            "#,
            Provider::Anthropic => r#"
                (call
                    function: (identifier) @function
                    (#eq? @function "Anthropic")
                    arguments: (argument_list) @args
                ) @call_expr
            "#,
            Provider::Cohere => r#"
                (call
                    function: (identifier) @function
                    (#eq? @function "CohereClient")
                    arguments: (argument_list) @args
                ) @call_expr
            "#,
            Provider::HuggingFace => r#"
                (call
                    function: (identifier) @function
                    (#eq? @function "InferenceClient")
                    arguments: (argument_list) @args
                ) @call_expr
            "#,
        }
    }

    fn check_has_base_url(&self, source: &str, args_node: tree_sitter::Node, _provider: Provider) -> (bool, Option<String>) {
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
        let source = fs::read_to_string(file_path)?;

        let mut parser = Parser::new();
        parser.set_language(tree_sitter_python::language())
            .map_err(|_| PromptGuardError::Parse("Failed to set language".to_string()))?;

        let tree = parser
            .parse(&source, None)
            .ok_or_else(|| PromptGuardError::Parse("Failed to parse Python file".to_string()))?;

        let query_str = self.get_query_for_provider(provider);
        let query = Query::new(tree_sitter_python::language(), query_str)
            .map_err(|e| PromptGuardError::Parse(format!("Query error: {}", e)))?;

        let mut cursor = QueryCursor::new();
        let matches = cursor.matches(&query, tree.root_node(), source.as_bytes());

        let mut instances = Vec::new();

        for match_ in matches {
            for capture in match_.captures {
                if query.capture_names()[capture.index as usize] == "call_expr" {
                    let node = capture.node;
                    let start_position = node.start_position();

                    let has_base_url = match_.captures.iter()
                        .find(|c| query.capture_names()[c.index as usize] == "args")
                        .map(|c| self.check_has_base_url(&source, c.node, provider))
                        .unwrap_or((false, None));

                    instances.push(DetectionInstance {
                        file_path: file_path.to_path_buf(),
                        line: start_position.row + 1,
                        column: start_position.column + 1,
                        provider,
                        language: self.language(),
                        has_base_url: has_base_url.0,
                        current_base_url: has_base_url.1,
                    });
                }
            }
        }

        Ok(DetectionResult { instances })
    }

    fn language(&self) -> Language {
        Language::Python
    }
}
