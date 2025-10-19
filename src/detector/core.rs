/// Core detection logic shared across all language detectors.
///
/// This module eliminates the massive duplication between Python and TypeScript detectors
/// by extracting common tree-sitter parsing and query logic.
use crate::error::{PromptGuardError, Result};
use crate::types::{DetectionInstance, DetectionResult, Language, Provider};
use std::fs;
use std::path::Path;
use tree_sitter::{Language as TSLanguage, Parser, Query, QueryCursor};

/// Configuration for a language-specific detector
pub struct DetectorConfig {
    pub ts_language: TSLanguage,
    pub language: Language,
    pub capture_name: &'static str,
}

/// Generic tree-sitter based detection implementation.
///
/// This function encapsulates the common pattern:
/// 1. Parse source file with tree-sitter
/// 2. Execute provider-specific query
/// 3. Extract detection instances from matches
/// 4. Check for `base_url` configuration
pub fn detect_in_file_generic(
    file_path: &Path,
    provider: Provider,
    config: &DetectorConfig,
    query_str: &str,
    check_base_url: impl Fn(&str, tree_sitter::Node, Provider) -> (bool, Option<String>),
) -> Result<DetectionResult> {
    let source = fs::read_to_string(file_path)?;

    let mut parser = Parser::new();
    parser
        .set_language(config.ts_language)
        .map_err(|_| PromptGuardError::Parse("Failed to set language".to_string()))?;

    let tree = parser.parse(&source, None).ok_or_else(|| {
        PromptGuardError::Parse(format!("Failed to parse {} file", config.language.as_str()))
    })?;

    let query = Query::new(config.ts_language, query_str)
        .map_err(|e| PromptGuardError::Parse(format!("Query error: {e}")))?;

    let mut cursor = QueryCursor::new();
    let matches = cursor.matches(&query, tree.root_node(), source.as_bytes());

    let mut instances = Vec::new();

    for match_ in matches {
        for capture in match_.captures {
            if query.capture_names()[capture.index as usize] == config.capture_name {
                let node = capture.node;
                let start_position = node.start_position();

                let has_base_url = match_
                    .captures
                    .iter()
                    .find(|c| query.capture_names()[c.index as usize] == "args")
                    .map_or((false, None), |c| check_base_url(&source, c.node, provider));

                instances.push(DetectionInstance {
                    file_path: file_path.to_path_buf(),
                    line: start_position.row + 1,
                    column: start_position.column + 1,
                    provider,
                    language: config.language,
                    has_base_url: has_base_url.0,
                    current_base_url: has_base_url.1,
                });
            }
        }
    }

    Ok(DetectionResult { instances })
}
