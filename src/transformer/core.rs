use crate::error::{PromptGuardError, Result};
use crate::types::TransformResult;
use std::fs;
use std::path::Path;
use streaming_iterator::StreamingIterator;
use tree_sitter::{Language as TSLanguage, Parser, Query, QueryCursor};

pub struct TransformConfig {
    pub parser_language: TSLanguage,
    pub language_name: &'static str,
}

pub fn transform_file_generic<F, G>(
    file_path: &Path,
    config: &TransformConfig,
    query_str: &str,
    extract_modification: F,
    finalize: G,
) -> Result<TransformResult>
where
    F: Fn(&str, tree_sitter::Node) -> Option<(usize, usize, String)>,
    G: Fn(String) -> String,
{
    let source = fs::read_to_string(file_path)?;

    let mut parser = Parser::new();
    parser.set_language(&config.parser_language).map_err(|_| {
        PromptGuardError::Parse(format!("Failed to set {} language", config.language_name))
    })?;

    let tree = parser.parse(&source, None).ok_or_else(|| {
        PromptGuardError::Parse(format!("Failed to parse {} file", config.language_name))
    })?;

    let query = Query::new(&config.parser_language, query_str)
        .map_err(|e| PromptGuardError::Parse(format!("Query error: {e}")))?;

    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, tree.root_node(), source.as_bytes());

    let mut modifications: Vec<(usize, usize, String)> = Vec::new();

    while let Some(match_) = matches.next() {
        let args_node = match match_
            .captures
            .iter()
            .find(|c| query.capture_names()[c.index as usize] == "args")
        {
            Some(capture) => capture.node,
            None => continue,
        };

        if let Some(modification) = extract_modification(&source, args_node) {
            modifications.push(modification);
        }
    }

    if modifications.is_empty() {
        return Ok(TransformResult { modified: false });
    }

    modifications.sort_by_key(|(start, _, _)| std::cmp::Reverse(*start));

    let mut new_source = source;
    for (start, end, replacement) in modifications {
        new_source.replace_range(start..end, &replacement);
    }

    new_source = finalize(new_source);

    fs::write(file_path, &new_source)?;

    Ok(TransformResult { modified: true })
}
