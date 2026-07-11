mod core;
mod python;
mod typescript;

pub use python::PythonTransformer;
pub use typescript::TypeScriptTransformer;

use crate::error::Result;
use crate::types::{Language, Provider, TransformResult};
use std::path::Path;

pub trait Transformer {
    /// Transform `file_path` in place. With `dry_run` the full transformation
    /// is computed (so `modified` is accurate) but the file is never written.
    fn transform_file(
        &self,
        file_path: &Path,
        provider: Provider,
        proxy_url: &str,
        api_key_env_var: &str,
        dry_run: bool,
    ) -> Result<TransformResult>;
}

pub fn transform_file(
    file_path: &Path,
    provider: Provider,
    proxy_url: &str,
    api_key_env_var: &str,
    dry_run: bool,
) -> Result<TransformResult> {
    let ext = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");

    let language = Language::from_extension(ext);
    let Some(language) = language else {
        return Ok(TransformResult { modified: false });
    };

    let transformer: Box<dyn Transformer> = match language {
        Language::TypeScript | Language::JavaScript => Box::new(TypeScriptTransformer::new()),
        Language::Python => Box::new(PythonTransformer::new()),
    };

    transformer.transform_file(file_path, provider, proxy_url, api_key_env_var, dry_run)
}
