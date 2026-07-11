/// Core detection logic shared across all languages.
///
/// Parses each file once and runs every provider's pre-compiled query
/// against the shared tree (previously each of the 7 providers re-read,
/// re-parsed, and re-compiled its query per file).
use crate::detector::queries::{get_python_detection_query, get_typescript_query};
use crate::detector::registry::PROVIDERS;
use crate::error::{PromptGuardError, Result};
use crate::types::{DetectionInstance, DetectionResult, Provider};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::LazyLock;
use streaming_iterator::StreamingIterator;
use tree_sitter::{Language as TSLanguage, Parser, Query, QueryCursor, Tree};

/// Concrete tree-sitter grammar used to parse a file.
///
/// Distinct from [`crate::types::Language`]: `.tsx`/`.jsx` files need the
/// TSX grammar (JSX syntax is not valid under the plain TypeScript grammar),
/// while `.ts` must NOT use TSX (generic syntax is ambiguous with JSX).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Grammar {
    TypeScript,
    Tsx,
    Python,
}

impl Grammar {
    pub fn for_extension(ext: &str) -> Option<Self> {
        match ext {
            "ts" | "js" => Some(Self::TypeScript),
            "tsx" | "jsx" => Some(Self::Tsx),
            "py" => Some(Self::Python),
            _ => None,
        }
    }

    pub fn ts_language(self) -> TSLanguage {
        match self {
            Self::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Self::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
            Self::Python => tree_sitter_python::LANGUAGE.into(),
        }
    }

    /// Human-readable grammar name, used in transform error messages.
    pub fn language_name(self) -> &'static str {
        match self {
            Self::TypeScript => "TypeScript",
            Self::Tsx => "TSX",
            Self::Python => "Python",
        }
    }

    fn capture_name(self) -> &'static str {
        match self {
            Self::Python => "call_expr",
            Self::TypeScript | Self::Tsx => "new_expr",
        }
    }

    fn is_python(self) -> bool {
        matches!(self, Self::Python)
    }
}

const ALL_GRAMMARS: [Grammar; 3] = [Grammar::TypeScript, Grammar::Tsx, Grammar::Python];

/// Compiled queries for every (grammar, provider) pair, built once.
static QUERY_CACHE: LazyLock<HashMap<(Grammar, Provider), Query>> = LazyLock::new(|| {
    let mut cache = HashMap::new();
    for info in PROVIDERS {
        let provider = info.provider;
        for grammar in ALL_GRAMMARS {
            let query_str = if grammar.is_python() {
                get_python_detection_query(provider)
            } else {
                get_typescript_query(provider)
            };
            // Query templates are static; a compile failure is a programming
            // error surfaced by `query_cache_is_complete` in tests.
            if let Ok(query) = Query::new(&grammar.ts_language(), &query_str) {
                cache.insert((grammar, provider), query);
            }
        }
    }
    cache
});

/// Whether a Python constructor argument list already sets `base_url`.
///
/// Shared by the detector and the transformer so the two never drift on what
/// counts as "already configured" (a drift that would let the transformer
/// double-inject a `base_url`).
pub fn python_args_have_base_url(args_text: &str) -> bool {
    args_text.contains("base_url=") || args_text.contains("base_url =")
}

/// Whether a TypeScript options object already sets the provider's base-URL
/// parameter. Shared by the detector and the transformer.
pub fn typescript_object_has_base_url(
    object_text: &str,
    info: &crate::detector::registry::ProviderInfo,
) -> bool {
    object_text.contains(&format!("{}:", info.ts_base_url_param))
        || object_text.contains(&format!("\"{}\": ", info.ts_base_url_param))
        || object_text.contains(&format!("'{}': ", info.ts_base_url_param))
        || object_text.contains("base_url:")
}

fn python_check_base_url(source: &str, args_node: tree_sitter::Node) -> (bool, Option<String>) {
    let args_text = &source[args_node.start_byte()..args_node.end_byte()];
    let has_base_url = python_args_have_base_url(args_text);
    let current = has_base_url.then(|| "(configured)".to_string());
    (has_base_url, current)
}

fn typescript_check_base_url(
    source: &str,
    args_node: tree_sitter::Node,
    provider: Provider,
) -> (bool, Option<String>) {
    let Some(info) = crate::detector::registry::ProviderInfo::get(provider) else {
        return (false, None);
    };
    let args_text = &source[args_node.start_byte()..args_node.end_byte()];

    let has_base_url = typescript_object_has_base_url(args_text, info);

    let current = has_base_url.then(|| "(configured)".to_string());
    (has_base_url, current)
}

/// Run one provider's cached query against an already-parsed tree.
fn detect_in_tree(
    file_path: &Path,
    source: &str,
    tree: &Tree,
    grammar: Grammar,
    provider: Provider,
    query: &Query,
) -> DetectionResult {
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(query, tree.root_node(), source.as_bytes());

    let capture_name = grammar.capture_name();
    let mut instances = Vec::new();

    while let Some(match_) = matches.next() {
        for capture in match_.captures {
            if query.capture_names()[capture.index as usize] != capture_name {
                continue;
            }
            let node = capture.node;
            let start_position = node.start_position();

            let (has_base_url, current_base_url) = match_
                .captures
                .iter()
                .find(|c| query.capture_names()[c.index as usize] == "args")
                .map_or((false, None), |c| {
                    if grammar.is_python() {
                        python_check_base_url(source, c.node)
                    } else {
                        typescript_check_base_url(source, c.node, provider)
                    }
                });

            instances.push(DetectionInstance {
                file_path: file_path.to_path_buf(),
                line: start_position.row + 1,
                column: start_position.column + 1,
                has_base_url,
                current_base_url,
            });
        }
    }

    DetectionResult { instances }
}

/// Detect all providers' SDK usage in a single file.
///
/// The file is read and parsed exactly once; each provider's pre-compiled
/// query runs against the shared tree.
pub fn detect_all_providers_in_file(file_path: &Path) -> Result<Vec<(Provider, DetectionResult)>> {
    let ext = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let Some(grammar) = Grammar::for_extension(ext) else {
        return Ok(Vec::new());
    };

    let source = fs::read_to_string(file_path)?;

    let mut parser = Parser::new();
    parser
        .set_language(&grammar.ts_language())
        .map_err(|_| PromptGuardError::Parse("Failed to set language".to_string()))?;

    let tree = parser.parse(&source, None).ok_or_else(|| {
        PromptGuardError::Parse(format!("Failed to parse {}", file_path.display()))
    })?;

    let mut results = Vec::new();
    for info in PROVIDERS {
        let provider = info.provider;
        let Some(query) = QUERY_CACHE.get(&(grammar, provider)) else {
            continue;
        };
        let result = detect_in_tree(file_path, &source, &tree, grammar, provider, query);
        if !result.instances.is_empty() {
            results.push((provider, result));
        }
    }

    Ok(results)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    /// Every (grammar, provider) pair must have a compiled query — a missing
    /// entry means a query template failed to compile for that grammar.
    #[test]
    fn query_cache_is_complete() {
        for info in PROVIDERS {
            for grammar in ALL_GRAMMARS {
                assert!(
                    QUERY_CACHE.contains_key(&(grammar, info.provider)),
                    "no compiled query for {:?} / {:?}",
                    grammar,
                    info.provider
                );
            }
        }
    }

    /// Bedrock detection must only fire for "bedrock-runtime" boto3 clients:
    /// boto3.client(...) constructs clients for every AWS service.
    #[test]
    fn bedrock_detects_only_bedrock_runtime_clients() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("aws.py");
        std::fs::write(
            &path,
            "import boto3\n\
             s3 = boto3.client(\"s3\")\n\
             dynamo = boto3.client(\"dynamodb\", region_name=\"us-east-1\")\n\
             br1 = boto3.client(\"bedrock-runtime\")\n\
             br2 = boto3.client(service_name=\"bedrock-runtime\", region_name=\"us-east-1\")\n",
        )
        .unwrap();

        let results = detect_all_providers_in_file(&path).unwrap();
        let bedrock = results
            .iter()
            .find(|(p, _)| *p == Provider::Bedrock)
            .map(|(_, r)| r);

        let instances = bedrock.map_or(0, |r| r.instances.len());
        assert_eq!(
            instances, 2,
            "only the two bedrock-runtime clients must be detected"
        );
    }

    /// Gemini detection must match genai.Client / google.genai.Client but
    /// not arbitrary bare Client(...) calls (database clients, HTTP
    /// clients, ...), which used to be reported as Gemini SDK usage.
    #[test]
    fn gemini_ignores_bare_client_calls() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("clients.py");
        std::fs::write(
            &path,
            "from qdrant_client import Client\n\
             db = Client(host=\"localhost\")\n\
             import google.genai as genai\n\
             g1 = genai.Client(api_key=key)\n\
             import google\n\
             g2 = google.genai.Client(api_key=key)\n",
        )
        .unwrap();

        let results = detect_all_providers_in_file(&path).unwrap();
        let gemini = results
            .iter()
            .find(|(p, _)| *p == Provider::Gemini)
            .map(|(_, r)| r);

        assert_eq!(
            gemini.map_or(0, |r| r.instances.len()),
            2,
            "only genai.Client and google.genai.Client must match"
        );
    }

    /// Async client classes (`AsyncOpenAI`/`AsyncAnthropic`/`AsyncGroq`/
    /// `AsyncInferenceClient`) must be detected — previously they were
    /// invisible while `enable --runtime` claimed full coverage.
    #[test]
    fn async_client_classes_are_detected() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("async_clients.py");
        std::fs::write(
            &path,
            "from openai import AsyncOpenAI\n\
             from anthropic import AsyncAnthropic\n\
             from groq import AsyncGroq\n\
             from huggingface_hub import AsyncInferenceClient\n\
             c1 = AsyncOpenAI()\n\
             c2 = AsyncAnthropic(api_key=key)\n\
             c3 = AsyncGroq(api_key=key)\n\
             c4 = AsyncInferenceClient(token=key)\n",
        )
        .unwrap();

        let results = detect_all_providers_in_file(&path).unwrap();
        for provider in [
            Provider::OpenAI,
            Provider::Anthropic,
            Provider::Groq,
            Provider::HuggingFace,
        ] {
            let instances = results
                .iter()
                .find(|(p, _)| *p == provider)
                .map_or(0, |(_, r)| r.instances.len());
            assert_eq!(
                instances, 1,
                "async client class for {provider:?} must be detected"
            );
        }
    }

    /// Module-qualified async constructor calls must also be detected.
    #[test]
    fn module_qualified_async_client_is_detected() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("async_attr.py");
        std::fs::write(
            &path,
            "import openai\nclient = openai.AsyncOpenAI(api_key=key)\n",
        )
        .unwrap();

        let results = detect_all_providers_in_file(&path).unwrap();
        let openai = results
            .iter()
            .find(|(p, _)| *p == Provider::OpenAI)
            .map_or(0, |(_, r)| r.instances.len());
        assert_eq!(openai, 1, "openai.AsyncOpenAI(...) must be detected");
    }

    #[test]
    fn grammar_selection_by_extension() {
        assert_eq!(Grammar::for_extension("ts"), Some(Grammar::TypeScript));
        assert_eq!(Grammar::for_extension("js"), Some(Grammar::TypeScript));
        assert_eq!(Grammar::for_extension("tsx"), Some(Grammar::Tsx));
        assert_eq!(Grammar::for_extension("jsx"), Some(Grammar::Tsx));
        assert_eq!(Grammar::for_extension("py"), Some(Grammar::Python));
        assert_eq!(Grammar::for_extension("rs"), None);
    }
}
