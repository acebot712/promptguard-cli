/// Code analysis utilities
///
/// This module provides analyzers for understanding how LLM SDKs are used
/// in a codebase, including environment variable usage and data flow.
pub mod envscanner;

pub use envscanner::EnvScanner;
