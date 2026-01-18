// Public library interface for PromptGuard CLI
// This allows integration tests to access internal modules
#![allow(clippy::unused_self)] // Many methods may need self in future
#![allow(clippy::if_not_else)] // Boolean not is clearer in some contexts
#![allow(clippy::too_many_lines)] // Some functions legitimately complex
#![allow(clippy::unnecessary_wraps)] // Consistent Result returns aid refactoring
#![allow(clippy::manual_let_else)] // Match syntax can be clearer
#![allow(clippy::assigning_clones)] // Micro-optimization not worth the noise
#![allow(clippy::trivially_copy_pass_by_ref)] // API consistency more important

pub mod analyzer;
pub mod config;
pub mod detector;
pub mod error;
pub mod scanner;
pub mod shim;
pub mod transformer;
pub mod types;

// Re-export commonly used types
pub use types::{Language, Provider};
