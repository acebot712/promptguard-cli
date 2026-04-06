use super::core::{transform_file_generic, TransformConfig};
use crate::detector::{get_typescript_query, ProviderInfo};
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
    let info = ProviderInfo::get(provider);
    let object_text = &source[object_node.start_byte()..object_node.end_byte()];

    object_text.contains(&format!("{}:", info.ts_base_url_param))
        || object_text.contains(&format!("\"{}\": ", info.ts_base_url_param))
        || object_text.contains("base_url:")
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

    let info = ProviderInfo::get(provider);
    let object_text = &source[object_node.start_byte()..object_node.end_byte()];
    let inner = object_text
        .trim_start_matches('{')
        .trim_end_matches('}')
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
        let config = TransformConfig {
            parser_language: tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            language_name: "TypeScript",
        };
        let query_str = get_typescript_query(provider);

        transform_file_generic(
            file_path,
            &config,
            &query_str,
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
