// Public library interface for PromptGuard CLI
// This allows integration tests to access internal modules
#![allow(clippy::if_not_else)] // Boolean not is clearer in some contexts
#![allow(clippy::too_many_lines)] // Some functions legitimately complex
#![allow(clippy::manual_let_else)] // Match syntax can be clearer
#![allow(clippy::assigning_clones)] // Micro-optimization not worth the noise
#![allow(clippy::trivially_copy_pass_by_ref)] // API consistency more important
#![allow(clippy::unused_self)] // Instance methods for API consistency
#![allow(clippy::unnecessary_wraps)] // Result wrapping for API consistency
#![allow(dead_code)] // Struct fields used via Debug/Clone derives or future use

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
