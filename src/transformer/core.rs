use crate::detector::registry::PROVIDERS;
use crate::detector::{get_python_transform_query, get_typescript_query, Grammar};
use crate::error::{PromptGuardError, Result};
use crate::types::{Provider, TransformResult};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::LazyLock;
use streaming_iterator::StreamingIterator;
use tree_sitter::{Parser, Query, QueryCursor};

const ALL_GRAMMARS: [Grammar; 3] = [Grammar::TypeScript, Grammar::Tsx, Grammar::Python];

/// Compiled transform queries for every (grammar, provider) pair, built once.
///
/// Mirrors the detector's `QUERY_CACHE`: previously `transform_file_generic`
/// recompiled a fresh `Query` on every call, i.e. once per provider per file.
static TRANSFORM_QUERY_CACHE: LazyLock<HashMap<(Grammar, Provider), Query>> = LazyLock::new(|| {
    let mut cache = HashMap::new();
    for info in PROVIDERS {
        let provider = info.provider;
        for grammar in ALL_GRAMMARS {
            let query_str = match grammar {
                Grammar::Python => get_python_transform_query(provider),
                Grammar::TypeScript | Grammar::Tsx => get_typescript_query(provider),
            };
            // Query templates are static; a compile failure is a programming
            // error surfaced by `transform_query_cache_is_complete` in tests.
            if let Ok(query) = Query::new(&grammar.ts_language(), &query_str) {
                cache.insert((grammar, provider), query);
            }
        }
    }
    cache
});

pub fn transform_file_generic<F, G>(
    file_path: &Path,
    grammar: Grammar,
    provider: Provider,
    extract_modification: F,
    finalize: G,
    dry_run: bool,
) -> Result<TransformResult>
where
    F: Fn(&str, tree_sitter::Node) -> Option<(usize, usize, String)>,
    G: Fn(String) -> String,
{
    let language = grammar.ts_language();
    let language_name = grammar.language_name();

    let query = TRANSFORM_QUERY_CACHE
        .get(&(grammar, provider))
        .ok_or_else(|| {
            PromptGuardError::Parse(format!(
                "No compiled transform query for {language_name}/{provider:?}"
            ))
        })?;

    let source = fs::read_to_string(file_path)?;
    // Capture the source's original line ending before any edits: the
    // in-place replacements and `finalize` splice in LF-terminated snippets,
    // so the final output is renormalized back to this ending.
    let source_eol = crate::text::dominant_line_ending(&source);

    let mut parser = Parser::new();
    parser
        .set_language(&language)
        .map_err(|_| PromptGuardError::Parse(format!("Failed to set {language_name} language")))?;

    let tree = parser
        .parse(&source, None)
        .ok_or_else(|| PromptGuardError::Parse(format!("Failed to parse {language_name} file")))?;

    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(query, tree.root_node(), source.as_bytes());

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
        return Ok(TransformResult {
            modified: false,
            needs_manual_routing: 0,
        });
    }

    modifications.sort_by_key(|(start, _, _)| std::cmp::Reverse(*start));

    let mut new_source = source;
    for (start, end, replacement) in modifications {
        new_source.replace_range(start..end, &replacement);
    }

    new_source = finalize(new_source);
    // Renormalize: replacements/finalize spliced in LF snippets, so a CRLF
    // source would otherwise end up with mixed endings.
    new_source = crate::text::reencode_line_endings(&new_source, source_eol);

    // Dry-run computes the full transformation (so callers can report exactly
    // what would change) but must never touch the file on disk.
    if !dry_run {
        fs::write(file_path, &new_source)?;
    }

    Ok(TransformResult {
        modified: true,
        needs_manual_routing: 0,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    /// Every (grammar, provider) transform query must compile — a missing
    /// entry means a transform query template failed to compile.
    #[test]
    fn transform_query_cache_is_complete() {
        for info in PROVIDERS {
            for grammar in ALL_GRAMMARS {
                assert!(
                    TRANSFORM_QUERY_CACHE.contains_key(&(grammar, info.provider)),
                    "no compiled transform query for {:?} / {:?}",
                    grammar,
                    info.provider
                );
            }
        }
    }
}
