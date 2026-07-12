mod core;
pub mod queries;
pub mod registry;

pub use core::{python_args_have_base_url, typescript_object_has_base_url, Grammar};
pub use queries::{get_python_transform_query, get_typescript_query};
pub use registry::ProviderInfo;

use crate::error::Result;
use crate::types::{DetectionResult, Provider};
use std::path::Path;

/// Detect all providers' SDK usage in a single file.
///
/// Parses the file once (with a grammar chosen by extension — TSX for
/// `.tsx`/`.jsx`) and runs every provider's cached, pre-compiled query
/// against the shared tree.
pub fn detect_all_providers(file_path: &Path) -> Result<Vec<(Provider, DetectionResult)>> {
    core::detect_all_providers_in_file(file_path)
}
