mod typescript;
mod python;

pub use typescript::TypeScriptTransformer;
pub use python::PythonTransformer;

use crate::error::Result;
use crate::types::{Language, Provider, TransformResult};
use std::path::Path;

pub trait Transformer {
    fn transform_file(
        &self,
        file_path: &Path,
        provider: Provider,
        proxy_url: &str,
        api_key_env_var: &str,
    ) -> Result<TransformResult>;

    fn language(&self) -> Language;
}

pub fn transform_file(
    file_path: &Path,
    provider: Provider,
    proxy_url: &str,
    api_key_env_var: &str,
) -> Result<TransformResult> {
    let ext = file_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");

    let language = Language::from_extension(ext);
    if language.is_none() {
        return Ok(TransformResult {
            file_path: file_path.to_path_buf(),
            success: false,
            modified: false,
            error: Some("Unsupported file type".to_string()),
        });
    }

    let transformer: Box<dyn Transformer> = match language.unwrap() {
        Language::TypeScript | Language::JavaScript => {
            Box::new(TypeScriptTransformer::new())
        }
        Language::Python => Box::new(PythonTransformer::new()),
    };

    transformer.transform_file(file_path, provider, proxy_url, api_key_env_var)
}
