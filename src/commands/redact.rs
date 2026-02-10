//! Redact Command - Remove PII and sensitive data from text
//!
//! Calls the PromptGuard `/security/redact` API endpoint to redact
//! sensitive information like emails, phone numbers, SSNs, etc.

use crate::api::PromptGuardClient;
use crate::config::ConfigManager;
use crate::error::{PromptGuardError, Result};
use crate::output::Output;
use serde::{Deserialize, Serialize};
use std::fs;

/// Response from the /security/redact endpoint
#[derive(Debug, Deserialize, Serialize)]
pub struct RedactResponse {
    pub redacted_text: String,
    #[serde(default)]
    pub entities_found: Vec<RedactedEntity>,
    #[serde(default)]
    pub entity_count: usize,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct RedactedEntity {
    #[serde(rename = "type")]
    pub entity_type: String,
    pub original: String,
    pub replacement: String,
    #[serde(default)]
    pub start: usize,
    #[serde(default)]
    pub end: usize,
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
                    format!("Failed to read file '{}': {}", file_path, e),
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

        // Call the redact endpoint
        let response: RedactResponse = client.post(
            "/security/redact",
            &serde_json::json!({
                "text": content,
            }),
        )?;

        // Handle output
        if let Some(ref output_path) = self.output {
            fs::write(output_path, &response.redacted_text).map_err(|e| {
                PromptGuardError::Io(std::io::Error::new(
                    e.kind(),
                    format!("Failed to write output file '{}': {}", output_path, e),
                ))
            })?;

            if !self.json {
                Output::success(&format!("Redacted content written to {}", output_path));
                println!();
                println!("Entities redacted: {}", response.entity_count);
                for entity in &response.entities_found {
                    println!("  • {} → {}", entity.entity_type, entity.replacement);
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
            println!("{}", response.redacted_text);
            println!("─────────────────────────────────────────────────");
            println!();

            if response.entity_count > 0 {
                Output::success(&format!(
                    "{} sensitive entities redacted",
                    response.entity_count
                ));
                println!();
                println!("Entities found:");
                for entity in &response.entities_found {
                    println!(
                        "  • {} '{}' → '{}'",
                        entity.entity_type, entity.original, entity.replacement
                    );
                }
            } else {
                Output::info("No sensitive entities detected.");
            }
        }

        Ok(())
    }
}
