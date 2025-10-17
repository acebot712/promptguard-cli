use crate::error::{PromptGuardError, Result};
use crate::transformer::Transformer;
use crate::types::{Language, Provider, TransformResult};
use std::fs;
use std::path::Path;
use tree_sitter::{Node, Parser, Query, QueryCursor};

pub struct TypeScriptTransformer;

impl TypeScriptTransformer {
    pub fn new() -> Self {
        Self
    }

    fn get_query_for_provider(&self, provider: Provider) -> &'static str {
        match provider {
            Provider::OpenAI => r#"
                (new_expression
                    constructor: (identifier) @constructor
                    (#eq? @constructor "OpenAI")
                    arguments: (arguments) @args
                ) @new_expr
            "#,
            Provider::Anthropic => r#"
                (new_expression
                    constructor: (identifier) @constructor
                    (#eq? @constructor "Anthropic")
                    arguments: (arguments) @args
                ) @new_expr
            "#,
            Provider::Cohere => r#"
                (new_expression
                    constructor: (identifier) @constructor
                    (#eq? @constructor "CohereClient")
                    arguments: (arguments) @args
                ) @new_expr
            "#,
            Provider::HuggingFace => r#"
                (new_expression
                    constructor: (identifier) @constructor
                    (#eq? @constructor "HfInference")
                    arguments: (arguments) @args
                ) @new_expr
            "#,
        }
    }

    fn has_base_url(&self, source: &str, object_node: Node, provider: Provider) -> bool {
        let base_url_param = provider.base_url_param();
        let object_text = &source[object_node.start_byte()..object_node.end_byte()];

        object_text.contains(&format!("{}:", base_url_param))
            || object_text.contains(&format!("\"{}\": ", base_url_param))
            || object_text.contains("base_url:")
    }

    fn transform_object(&self, source: &str, object_node: Node, provider: Provider, proxy_url: &str, api_key_env_var: &str) -> Option<String> {
        if self.has_base_url(source, object_node, provider) {
            return None;
        }

        let base_url_param = provider.base_url_param();
        let api_key_param = provider.api_key_param();

        let object_start = object_node.start_byte();
        let object_end = object_node.end_byte();
        let object_text = &source[object_start..object_end];

        let inner = object_text
            .trim_start_matches('{')
            .trim_end_matches('}')
            .trim();

        let mut new_object = String::from("{\n");

        if inner.is_empty() {
            new_object.push_str(&format!("  {}: process.env.{},\n", api_key_param, api_key_env_var));
            new_object.push_str(&format!("  {}: \"{}\"\n", base_url_param, proxy_url));
        } else {
            let trimmed = inner.trim();
            new_object.push_str("  ");
            new_object.push_str(trimmed);
            if !trimmed.ends_with(',') {
                new_object.push(',');
            }
            new_object.push('\n');
            new_object.push_str(&format!("  {}: \"{}\"\n", base_url_param, proxy_url));
        }

        new_object.push('}');
        Some(new_object)
    }
}

impl Transformer for TypeScriptTransformer {
    fn transform_file(
        &self,
        file_path: &Path,
        provider: Provider,
        proxy_url: &str,
        api_key_env_var: &str,
    ) -> Result<TransformResult> {
        let source = fs::read_to_string(file_path)?;

        let mut parser = Parser::new();
        parser
            .set_language(tree_sitter_typescript::language_typescript())
            .map_err(|_| PromptGuardError::Parse("Failed to set TypeScript language".to_string()))?;

        let tree = parser
            .parse(&source, None)
            .ok_or_else(|| PromptGuardError::Parse("Failed to parse TypeScript file".to_string()))?;

        let query_str = self.get_query_for_provider(provider);
        let query = Query::new(tree_sitter_typescript::language_typescript(), query_str)
            .map_err(|e| PromptGuardError::Parse(format!("Query error: {}", e)))?;

        let mut cursor = QueryCursor::new();
        let matches = cursor.matches(&query, tree.root_node(), source.as_bytes());

        let mut modifications = Vec::new();

        for match_ in matches {
            let args_node = match match_.captures.iter().find(|c| query.capture_names()[c.index as usize] == "args") {
                Some(capture) => capture.node,
                None => continue,
            };

            let mut object_node = None;
            let mut cursor = args_node.walk();
            for child in args_node.children(&mut cursor) {
                if child.kind() == "object" {
                    object_node = Some(child);
                    break;
                }
            }

            if let Some(obj_node) = object_node {
                if let Some(new_object) = self.transform_object(&source, obj_node, provider, proxy_url, api_key_env_var) {
                    modifications.push((obj_node.start_byte(), obj_node.end_byte(), new_object));
                }
            }
        }

        if modifications.is_empty() {
            return Ok(TransformResult {
                file_path: file_path.to_path_buf(),
                success: true,
                modified: false,
                error: None,
            });
        }

        modifications.sort_by_key(|(start, _, _)| std::cmp::Reverse(*start));

        let mut new_source = source;
        for (start, end, replacement) in modifications {
            new_source.replace_range(start..end, &replacement);
        }

        fs::write(file_path, &new_source)?;

        Ok(TransformResult {
            file_path: file_path.to_path_buf(),
            success: true,
            modified: true,
            error: None,
        })
    }

    fn language(&self) -> Language {
        Language::TypeScript
    }
}
