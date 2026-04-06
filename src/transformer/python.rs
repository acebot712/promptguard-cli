use super::core::{transform_file_generic, TransformConfig};
use crate::detector::get_python_transform_query;
use crate::transformer::Transformer;
use crate::types::{Provider, TransformResult};
use std::fmt::Write;
use std::path::Path;

pub struct PythonTransformer;

impl Default for PythonTransformer {
    fn default() -> Self {
        Self::new()
    }
}

impl PythonTransformer {
    pub fn new() -> Self {
        Self
    }
}

fn has_base_url(source: &str, args_node: tree_sitter::Node) -> bool {
    let args_text = &source[args_node.start_byte()..args_node.end_byte()];
    args_text.contains("base_url=") || args_text.contains("base_url =")
}

fn transform_args(
    source: &str,
    args_node: tree_sitter::Node,
    proxy_url: &str,
    api_key_env_var: &str,
) -> Option<String> {
    if has_base_url(source, args_node) {
        return None;
    }

    let args_text = &source[args_node.start_byte()..args_node.end_byte()];
    let inner = args_text
        .trim_start_matches('(')
        .trim_end_matches(')')
        .trim();

    let mut new_args = String::from("(\n");

    if inner.is_empty() {
        let _ = writeln!(
            new_args,
            "    api_key=os.environ.get(\"{api_key_env_var}\"),"
        );
        let _ = writeln!(new_args, "    base_url=\"{proxy_url}\"");
    } else {
        let trimmed = inner.trim();
        new_args.push_str("    ");
        new_args.push_str(trimmed);
        if !trimmed.ends_with(',') {
            new_args.push(',');
        }
        new_args.push('\n');
        let _ = writeln!(new_args, "    base_url=\"{proxy_url}\"");
    }

    new_args.push(')');
    Some(new_args)
}

fn ensure_os_import(source: String) -> String {
    if source.contains("import os") {
        return source;
    }
    format!("import os\n\n{source}")
}

impl Transformer for PythonTransformer {
    fn transform_file(
        &self,
        file_path: &Path,
        provider: Provider,
        proxy_url: &str,
        api_key_env_var: &str,
    ) -> crate::error::Result<TransformResult> {
        let config = TransformConfig {
            parser_language: tree_sitter_python::LANGUAGE.into(),
            language_name: "Python",
        };
        let query_str = get_python_transform_query(provider);

        transform_file_generic(
            file_path,
            &config,
            &query_str,
            |source, args_node| {
                transform_args(source, args_node, proxy_url, api_key_env_var)
                    .map(|new_args| (args_node.start_byte(), args_node.end_byte(), new_args))
            },
            ensure_os_import,
        )
    }
}
