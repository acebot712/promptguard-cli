use super::core::transform_file_generic;
use crate::detector::{typescript_object_has_base_url, Grammar, ProviderInfo};
use crate::transformer::Transformer;
use crate::types::{Provider, TransformResult};
use std::fmt::Write;
use std::path::Path;

pub struct TypeScriptTransformer;

impl Default for TypeScriptTransformer {
    fn default() -> Self {
        Self::new()
    }
}

impl TypeScriptTransformer {
    pub fn new() -> Self {
        Self
    }
}

fn ts_has_base_url(source: &str, object_node: tree_sitter::Node, provider: Provider) -> bool {
    let Some(info) = ProviderInfo::get(provider) else {
        return false;
    };
    let object_text = &source[object_node.start_byte()..object_node.end_byte()];
    typescript_object_has_base_url(object_text, info)
}

fn transform_ts_object(
    source: &str,
    object_node: tree_sitter::Node,
    provider: Provider,
    proxy_url: &str,
    api_key_env_var: &str,
) -> Option<String> {
    if ts_has_base_url(source, object_node, provider) {
        return None;
    }

    // No registry metadata: skip the transform rather than guessing
    // another provider's parameter names.
    let info = ProviderInfo::get(provider)?;

    // Detect-only providers (e.g. Bedrock) have empty parameter names in
    // the registry. Skip entirely: interpolating empty names produced
    // `{ : ..., : "..." }`, a guaranteed syntax error.
    if info.ts_base_url_param.is_empty() || info.ts_api_key_param.is_empty() {
        return None;
    }
    let object_text = &source[object_node.start_byte()..object_node.end_byte()];
    // Strip exactly ONE outer `{` / `}` — the delimiters of this `object`
    // node. `trim_end_matches('}')` stripped *every* trailing brace, so a
    // nested object literal like `{apiKey:k, defaults:{timeout:1}}` lost its
    // inner closing brace and became invalid.
    let inner = object_text
        .strip_prefix('{')
        .and_then(|s| s.strip_suffix('}'))
        .unwrap_or(object_text);

    // End of the last real property (comments excluded): the injected comma
    // must go right after it. Appending the comma to the raw text swallowed
    // it into any trailing line comment (`apiKey: k // note` became
    // `apiKey: k // note,`) and produced a syntax error.
    let last_prop_end = last_non_comment_child_end(object_node);

    let mut new_object = String::from("{\n");

    match last_prop_end {
        Some(end) if object_text.starts_with('{') => {
            let split_at = end - object_node.start_byte() - 1;
            let (props_part, tail) = inner.split_at(split_at);

            let mut tail = tail.trim_start();
            if let Some(rest) = tail.strip_prefix(',') {
                tail = rest.trim_start();
            }
            let tail = tail.trim_end();

            new_object.push_str("  ");
            new_object.push_str(props_part.trim());
            new_object.push(',');
            if !tail.is_empty() {
                new_object.push_str("  ");
                new_object.push_str(tail);
            }
            new_object.push('\n');
            let _ = writeln!(new_object, "  {}: \"{proxy_url}\"", info.ts_base_url_param);
        },
        _ => {
            // No properties: inject both the api key and the proxy URL. Keep
            // any comment-only content after the injected properties.
            let _ = writeln!(
                new_object,
                "  {}: process.env.{api_key_env_var},",
                info.ts_api_key_param
            );
            let _ = writeln!(new_object, "  {}: \"{proxy_url}\"", info.ts_base_url_param);
            let comments = inner.trim();
            if !comments.is_empty() {
                let _ = writeln!(new_object, "  {comments}");
            }
        },
    }

    new_object.push('}');
    Some(new_object)
}

/// Build a replacement for an EMPTY argument list — `new OpenAI()` — by
/// synthesizing an options object carrying the api key and proxy URL,
/// mirroring the Python transformer's empty-args behavior. Previously these
/// calls were detected but silently reported "(no changes needed)" and never
/// routed through the proxy.
///
/// Returns `None` (leave the call untouched) when the argument list has any
/// non-comment argument (an identifier, spread, call, …: the options come
/// from elsewhere) or the provider is detect-only.
fn synthesize_ts_options(
    source: &str,
    args_node: tree_sitter::Node,
    provider: Provider,
    proxy_url: &str,
    api_key_env_var: &str,
) -> Option<String> {
    if last_non_comment_child_end(args_node).is_some() {
        return None;
    }

    let info = ProviderInfo::get(provider)?;

    // Detect-only providers (e.g. Bedrock) have empty parameter names in the
    // registry — same guard as transform_ts_object.
    if info.ts_base_url_param.is_empty() || info.ts_api_key_param.is_empty() {
        return None;
    }

    let args_text = &source[args_node.start_byte()..args_node.end_byte()];
    let inner = args_text
        .strip_prefix('(')
        .and_then(|s| s.strip_suffix(')'))
        .unwrap_or(args_text);

    let mut new_args = String::from("({\n");
    let _ = writeln!(
        new_args,
        "  {}: process.env.{api_key_env_var},",
        info.ts_api_key_param
    );
    let _ = writeln!(new_args, "  {}: \"{proxy_url}\"", info.ts_base_url_param);
    // Keep any comment-only content from the original argument list.
    let comments = inner.trim();
    if !comments.is_empty() {
        let _ = writeln!(new_args, "  {comments}");
    }
    new_args.push_str("})");
    Some(new_args)
}

/// Byte offset just past the last named child of `node` that is not a
/// comment, or `None` when there is no such child.
fn last_non_comment_child_end(node: tree_sitter::Node) -> Option<usize> {
    let mut cursor = node.walk();
    let mut last_end = None;
    for child in node.named_children(&mut cursor) {
        if child.kind() != "comment" {
            last_end = Some(child.end_byte());
        }
    }
    last_end
}

impl Transformer for TypeScriptTransformer {
    fn transform_file(
        &self,
        file_path: &Path,
        provider: Provider,
        proxy_url: &str,
        api_key_env_var: &str,
        dry_run: bool,
    ) -> crate::error::Result<TransformResult> {
        // .tsx/.jsx need the TSX grammar: JSX syntax does not parse under
        // the plain TypeScript grammar (and .ts must not use TSX, where
        // generics are ambiguous with JSX).
        let grammar = match file_path.extension().and_then(|e| e.to_str()) {
            Some("tsx" | "jsx") => Grammar::Tsx,
            _ => Grammar::TypeScript,
        };

        transform_file_generic(
            file_path,
            grammar,
            provider,
            |source, args_node| {
                let mut cursor = args_node.walk();
                for child in args_node.children(&mut cursor) {
                    if child.kind() == "object" {
                        return transform_ts_object(
                            source,
                            child,
                            provider,
                            proxy_url,
                            api_key_env_var,
                        )
                        .map(|new_obj| (child.start_byte(), child.end_byte(), new_obj));
                    }
                }
                // No object literal argument: synthesize an options object
                // for empty argument lists (`new OpenAI()`), which were
                // previously detected but never transformed.
                synthesize_ts_options(source, args_node, provider, proxy_url, api_key_env_var)
                    .map(|new_args| (args_node.start_byte(), args_node.end_byte(), new_args))
            },
            |s| s,
            dry_run,
        )
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;
    use tree_sitter::Parser;

    /// Parse `source` as TypeScript and assert zero ERROR nodes.
    fn assert_reparses_clean(source: &str) {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
            .expect("set typescript language");
        let tree = parser.parse(source, None).expect("parse");
        assert!(
            !tree.root_node().has_error(),
            "transformed TypeScript has tree-sitter ERROR node(s):\n{source}"
        );
    }

    fn transform_ts(input: &str) -> String {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("client.ts");
        fs::write(&path, input).unwrap();
        TypeScriptTransformer::new()
            .transform_file(
                &path,
                Provider::OpenAI,
                "https://api.promptguard.co/api/v1",
                "PROMPTGUARD_API_KEY",
                false,
            )
            .unwrap();
        fs::read_to_string(&path).unwrap()
    }

    /// Regression: `trim_end_matches('}')` stripped the nested object's
    /// closing brace too, producing an unbalanced/invalid object literal.
    #[test]
    fn nested_object_literal_keeps_balanced_braces() {
        let input = "import OpenAI from \"openai\";\nconst client = new OpenAI({apiKey: k, defaultHeaders: {timeout: 1}});\n";
        let out = transform_ts(input);
        assert!(
            out.contains("defaultHeaders: {timeout: 1}"),
            "nested object must survive:\n{out}"
        );
        assert!(out.contains("baseURL: \"https://api.promptguard.co/api/v1\""));
        assert_reparses_clean(&out);
    }

    #[test]
    fn empty_options_object_reparses_clean() {
        let input = "import OpenAI from \"openai\";\nconst client = new OpenAI({});\n";
        let out = transform_ts(input);
        assert!(out.contains("baseURL:"));
        assert_reparses_clean(&out);
    }

    /// Regression: appending the comma to the raw object text swallowed it
    /// into a trailing line comment (`apiKey: k // note,`) → syntax error.
    #[test]
    fn trailing_line_comment_keeps_comma_out_of_comment() {
        let input = "import OpenAI from \"openai\";\nconst client = new OpenAI({\n  apiKey: k // from env\n});\n";
        let out = transform_ts(input);
        assert!(out.contains("baseURL: \"https://api.promptguard.co/api/v1\""));
        assert!(
            out.contains("apiKey: k,"),
            "comma must land after the property, not inside the comment:\n{out}"
        );
        assert!(out.contains("// from env"), "comment kept:\n{out}");
        assert_reparses_clean(&out);
    }

    /// Trailing comma plus trailing comment must not produce a double comma.
    #[test]
    fn trailing_comma_and_comment_reparses_clean() {
        let input = "import OpenAI from \"openai\";\nconst client = new OpenAI({\n  apiKey: k, // note\n});\n";
        let out = transform_ts(input);
        assert!(!out.contains(",,"), "no double comma:\n{out}");
        assert!(out.contains("// note"));
        assert_reparses_clean(&out);
    }

    /// Bedrock has empty parameter names in the registry (detect-only).
    /// Interpolating them produced `{ : ..., : "..." }` — a syntax error —
    /// so the transformer must leave `BedrockRuntimeClient` untouched.
    #[test]
    fn bedrock_runtime_client_is_left_untouched() {
        let input = "import { BedrockRuntimeClient } from \"@aws-sdk/client-bedrock-runtime\";\nconst client = new BedrockRuntimeClient({ region: \"us-east-1\" });\n";

        let dir = TempDir::new().unwrap();
        let path = dir.path().join("bedrock.ts");
        fs::write(&path, input).unwrap();
        let result = TypeScriptTransformer::new()
            .transform_file(
                &path,
                Provider::Bedrock,
                "https://api.promptguard.co/api/v1",
                "PROMPTGUARD_API_KEY",
                false,
            )
            .unwrap();

        assert!(!result.modified, "bedrock must be detect-only");
        let out = fs::read_to_string(&path).unwrap();
        assert_eq!(out, input, "BedrockRuntimeClient must be untouched");
    }

    /// Regression: `new OpenAI()` with NO options object was detected but
    /// silently reported "(no changes needed)" — the call never routed
    /// through the proxy. An options object must be synthesized, mirroring
    /// the Python empty-args behavior.
    #[test]
    fn no_arguments_synthesizes_options_object() {
        let input = "import OpenAI from \"openai\";\nconst client = new OpenAI();\n";
        let out = transform_ts(input);
        assert!(
            out.contains("apiKey: process.env.PROMPTGUARD_API_KEY"),
            "api key must be injected:\n{out}"
        );
        assert!(
            out.contains("baseURL: \"https://api.promptguard.co/api/v1\""),
            "proxy URL must be injected:\n{out}"
        );
        assert_reparses_clean(&out);
    }

    /// Comment-only argument lists must keep the comment and stay valid.
    #[test]
    fn comment_only_arguments_synthesize_and_keep_comment() {
        let input =
            "import OpenAI from \"openai\";\nconst client = new OpenAI(/* configured later */);\n";
        let out = transform_ts(input);
        assert!(out.contains("baseURL:"));
        assert!(
            out.contains("/* configured later */"),
            "comment kept:\n{out}"
        );
        assert_reparses_clean(&out);
    }

    /// A non-object argument (identifier, call, spread, …) means the options
    /// come from elsewhere — the call must be left untouched.
    #[test]
    fn identifier_argument_is_left_untouched() {
        let input = "import OpenAI from \"openai\";\nconst client = new OpenAI(config);\n";
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("client.ts");
        fs::write(&path, input).unwrap();
        let result = TypeScriptTransformer::new()
            .transform_file(
                &path,
                Provider::OpenAI,
                "https://api.promptguard.co/api/v1",
                "PROMPTGUARD_API_KEY",
                false,
            )
            .unwrap();
        assert!(!result.modified, "identifier args must not be rewritten");
        assert_eq!(fs::read_to_string(&path).unwrap(), input);
    }

    /// Bedrock is detect-only: an empty argument list must NOT get a
    /// synthesized options object either.
    #[test]
    fn bedrock_empty_arguments_left_untouched() {
        let input = "import { BedrockRuntimeClient } from \"@aws-sdk/client-bedrock-runtime\";\nconst client = new BedrockRuntimeClient();\n";
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("bedrock.ts");
        fs::write(&path, input).unwrap();
        let result = TypeScriptTransformer::new()
            .transform_file(
                &path,
                Provider::Bedrock,
                "https://api.promptguard.co/api/v1",
                "PROMPTGUARD_API_KEY",
                false,
            )
            .unwrap();
        assert!(!result.modified, "bedrock must stay detect-only");
        assert_eq!(fs::read_to_string(&path).unwrap(), input);
    }

    #[test]
    fn simple_options_object_reparses_clean() {
        let input =
            "import OpenAI from \"openai\";\nconst client = new OpenAI({apiKey: process.env.KEY});\n";
        let out = transform_ts(input);
        assert!(out.contains("apiKey: process.env.KEY"));
        assert!(out.contains("baseURL:"));
        assert_reparses_clean(&out);
    }
}
