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
    let object_text = &source[object_node.start_byte()..object_node.end_byte()];
    // Strip exactly ONE outer `{` / `}` — the delimiters of this `object`
    // node. `trim_end_matches('}')` stripped *every* trailing brace, so a
    // nested object literal like `{apiKey:k, defaults:{timeout:1}}` lost its
    // inner closing brace and became invalid.
    let inner = object_text
        .strip_prefix('{')
        .and_then(|s| s.strip_suffix('}'))
        .unwrap_or(object_text)
        .trim();

    let mut new_object = String::from("{\n");

    if inner.is_empty() {
        let _ = writeln!(
            new_object,
            "  {}: process.env.{api_key_env_var},",
            info.ts_api_key_param
        );
        let _ = writeln!(new_object, "  {}: \"{proxy_url}\"", info.ts_base_url_param);
    } else {
        let trimmed = inner.trim();
        new_object.push_str("  ");
        new_object.push_str(trimmed);
        if !trimmed.ends_with(',') {
            new_object.push(',');
        }
        new_object.push('\n');
        let _ = writeln!(new_object, "  {}: \"{proxy_url}\"", info.ts_base_url_param);
    }

    new_object.push('}');
    Some(new_object)
}

impl Transformer for TypeScriptTransformer {
    fn transform_file(
        &self,
        file_path: &Path,
        provider: Provider,
        proxy_url: &str,
        api_key_env_var: &str,
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
                None
            },
            |s| s,
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
