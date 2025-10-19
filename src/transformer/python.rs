use crate::error::{PromptGuardError, Result};
use crate::transformer::Transformer;
use crate::types::{Provider, TransformResult};
use std::fs;
use std::path::Path;
use tree_sitter::{Node, Parser, Query, QueryCursor};

pub struct PythonTransformer;

impl PythonTransformer {
    pub fn new() -> Self {
        Self
    }

    fn get_query_for_provider(&self, provider: Provider) -> &'static str {
        match provider {
            Provider::OpenAI => {
                r#"
                (call
                    function: (identifier) @function
                    (#eq? @function "OpenAI")
                    arguments: (argument_list) @args
                ) @call_expr
            "#
            },
            Provider::Anthropic => {
                r#"
                (call
                    function: (identifier) @function
                    (#eq? @function "Anthropic")
                    arguments: (argument_list) @args
                ) @call_expr
            "#
            },
            Provider::Cohere => {
                r#"
                (call
                    function: (identifier) @function
                    (#eq? @function "CohereClient")
                    arguments: (argument_list) @args
                ) @call_expr
            "#
            },
            Provider::HuggingFace => {
                r#"
                (call
                    function: (identifier) @function
                    (#eq? @function "InferenceClient")
                    arguments: (argument_list) @args
                ) @call_expr
            "#
            },
            Provider::Gemini => {
                r#"
                (call
                    function: (attribute
                        object: (identifier) @module
                        (#eq? @module "genai")
                        attribute: (identifier) @class
                        (#eq? @class "Client")
                    )
                    arguments: (argument_list) @args
                ) @call_expr
            "#
            },
            Provider::Groq => {
                r#"
                (call
                    function: (identifier) @function
                    (#eq? @function "Groq")
                    arguments: (argument_list) @args
                ) @call_expr
            "#
            },
        }
    }

    fn has_base_url(&self, source: &str, args_node: Node) -> bool {
        let args_text = &source[args_node.start_byte()..args_node.end_byte()];
        args_text.contains("base_url=") || args_text.contains("base_url =")
    }

    fn transform_args(
        &self,
        source: &str,
        args_node: Node,
        proxy_url: &str,
        api_key_env_var: &str,
    ) -> Option<String> {
        if self.has_base_url(source, args_node) {
            return None;
        }

        let args_start = args_node.start_byte();
        let args_end = args_node.end_byte();
        let args_text = &source[args_start..args_end];

        let inner = args_text
            .trim_start_matches('(')
            .trim_end_matches(')')
            .trim();

        let mut new_args = String::from("(\n");

        if inner.is_empty() {
            new_args.push_str(&format!(
                "    api_key=os.environ.get(\"{api_key_env_var}\"),\n"
            ));
            new_args.push_str(&format!("    base_url=\"{proxy_url}\"\n"));
        } else {
            let trimmed = inner.trim();
            new_args.push_str("    ");
            new_args.push_str(trimmed);
            if !trimmed.ends_with(',') {
                new_args.push(',');
            }
            new_args.push('\n');
            new_args.push_str(&format!("    base_url=\"{proxy_url}\"\n"));
        }

        new_args.push(')');
        Some(new_args)
    }

    fn ensure_os_import(&self, source: &str) -> String {
        if source.contains("import os") {
            return source.to_string();
        }

        format!("import os\n\n{source}")
    }
}

impl Transformer for PythonTransformer {
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
            .set_language(tree_sitter_python::language())
            .map_err(|_| PromptGuardError::Parse("Failed to set Python language".to_string()))?;

        let tree = parser
            .parse(&source, None)
            .ok_or_else(|| PromptGuardError::Parse("Failed to parse Python file".to_string()))?;

        let query_str = self.get_query_for_provider(provider);
        let query = Query::new(tree_sitter_python::language(), query_str)
            .map_err(|e| PromptGuardError::Parse(format!("Query error: {e}")))?;

        let mut cursor = QueryCursor::new();
        let matches = cursor.matches(&query, tree.root_node(), source.as_bytes());

        let mut modifications = Vec::new();

        for match_ in matches {
            let args_node = match match_
                .captures
                .iter()
                .find(|c| query.capture_names()[c.index as usize] == "args")
            {
                Some(capture) => capture.node,
                None => continue,
            };

            if let Some(new_args) =
                self.transform_args(&source, args_node, proxy_url, api_key_env_var)
            {
                modifications.push((args_node.start_byte(), args_node.end_byte(), new_args));
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

        new_source = self.ensure_os_import(&new_source);

        fs::write(file_path, &new_source)?;

        Ok(TransformResult {
            file_path: file_path.to_path_buf(),
            success: true,
            modified: true,
            error: None,
        })
    }
}
