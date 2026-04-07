//! Redact Command - Remove PII and sensitive data from text
//!
//! Calls the `PromptGuard` `/security/redact` API endpoint to redact
//! sensitive information like emails, phone numbers, SSNs, etc.

use crate::api::PromptGuardClient;
use crate::config::ConfigManager;
use crate::error::{PromptGuardError, Result};
use crate::output::Output;
use serde::{Deserialize, Serialize};
use std::fs;

/// Response from the /security/redact endpoint.
///
/// The backend returns `{ original, redacted, piiFound }`.
#[derive(Debug, Deserialize, Serialize)]
pub struct RedactResponse {
    pub original: String,
    pub redacted: String,
    #[serde(default, rename = "piiFound")]
    pub pii_found: Vec<String>,
}

pub struct RedactCommand {
    /// Text to redact
    pub text: Option<String>,
    /// File path to read and redact
    pub file: Option<String>,
    /// Output file path (if not provided, prints to stdout)
    pub output: Option<String>,
    /// Output as JSON
    pub json: bool,
}

impl RedactCommand {
    pub fn execute(&self) -> Result<()> {
        // Get content to redact
        let content = if let Some(ref text) = self.text {
            text.clone()
        } else if let Some(ref file_path) = self.file {
            fs::read_to_string(file_path).map_err(|e| {
                PromptGuardError::Io(std::io::Error::new(
                    e.kind(),
                    format!("Failed to read file '{file_path}': {e}"),
                ))
            })?
        } else {
            return Err(PromptGuardError::Custom(
                "Either --text or --file must be provided".to_string(),
            ));
        };

        // Get API key from config
        let config_manager = ConfigManager::new(None)?;
        let config = config_manager.load()?;

        let client = PromptGuardClient::new(config.api_key, Some(config.proxy_url))?;

        if !self.json {
            Output::header(&format!(
                "🛡️  PromptGuard CLI v{}",
                env!("CARGO_PKG_VERSION")
            ));
            Output::section("PII Redaction", "🔒");
            Output::info(&format!("Processing {} characters...", content.len()));
        }

        let response: RedactResponse = client.post(
            "/security/redact",
            &serde_json::json!({
                "content": content,
            }),
        )?;

        if let Some(ref output_path) = self.output {
            fs::write(output_path, &response.redacted).map_err(|e| {
                PromptGuardError::Io(std::io::Error::new(
                    e.kind(),
                    format!("Failed to write output file '{output_path}': {e}"),
                ))
            })?;

            if !self.json {
                Output::success(&format!("Redacted content written to {output_path}"));
                println!();
                println!("PII types redacted: {}", response.pii_found.len());
                for pii_type in &response.pii_found {
                    println!("  • {pii_type}");
                }
            }
        } else if self.json {
            println!(
                "{}",
                serde_json::to_string_pretty(&response).unwrap_or_default()
            );
        } else {
            println!();
            println!("Redacted Text:");
            println!("─────────────────────────────────────────────────");
            println!("{}", response.redacted);
            println!("─────────────────────────────────────────────────");
            println!();

            if response.pii_found.is_empty() {
                Output::info("No sensitive entities detected.");
            } else {
                Output::success(&format!(
                    "{} PII type(s) redacted",
                    response.pii_found.len()
                ));
                println!();
                println!("PII types found:");
                for pii_type in &response.pii_found {
                    println!("  • {pii_type}");
                }
            }
        }

        Ok(())
    }
}
