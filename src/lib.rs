// Public library interface for PromptGuard CLI
#![allow(clippy::if_not_else)]
#![allow(clippy::too_many_lines)]
#![allow(clippy::manual_let_else)]
#![allow(clippy::assigning_clones)]
#![allow(clippy::trivially_copy_pass_by_ref)]
#![allow(clippy::unused_self)]
#![allow(clippy::unnecessary_wraps)]

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
