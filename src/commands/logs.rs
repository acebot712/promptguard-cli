use crate::api::PromptGuardClient;
use crate::config::ConfigManager;
use crate::error::{PromptGuardError, Result};
use crate::output::Output;
use serde::{Deserialize, Serialize};
use std::fmt::Write;

/// Log entry from the API
#[derive(Debug, Deserialize, Serialize)]
pub struct LogEntry {
    pub id: String,
    pub timestamp: String,
    #[serde(rename = "type")]
    pub log_type: String,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub decision: Option<String>,
    #[serde(default)]
    pub threat_type: Option<String>,
    #[serde(default)]
    pub confidence: Option<f64>,
    #[serde(default)]
    pub latency_ms: Option<u64>,
    #[serde(default)]
    pub details: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct LogsResponse {
    logs: Vec<LogEntry>,
    #[serde(default)]
    total: usize,
    #[serde(default)]
    has_more: bool,
}

pub struct LogsCommand {
    pub limit: usize,
    pub log_type: Option<String>,
    pub json: bool,
}

impl Default for LogsCommand {
    fn default() -> Self {
        Self {
            limit: 20,
            log_type: None,
            json: false,
        }
    }
}

impl LogsCommand {
    pub fn execute(&self) -> Result<()> {
        // Resolve the credential + base URL through the shared precedence
        // (env > project > global) and the key-exfiltration guard, exactly
        // like `events`/`scan`/`redact`. This command does NOT require a
        // project to be initialized: a missing key yields the canonical
        // "No API key found …" guidance, not a divergent "Run init first".
        let (api_key, base_url) = crate::auth::resolve_session()?;
        let client = PromptGuardClient::new(api_key, Some(base_url))?;

        // Scope the query to the project when a project config exists; absent
        // one, fetch account-wide logs rather than erroring.
        let project_id = ConfigManager::new(None)
            .ok()
            .filter(ConfigManager::exists)
            .and_then(|m| m.load().ok())
            .and_then(|c| c.project_id);

        if !self.json {
            Output::header("Activity Logs");
            Output::info("Fetching logs from PromptGuard API...");
        }

        // Build query parameters (user/config-provided values are
        // percent-encoded so they cannot smuggle extra parameters)
        let mut endpoint = format!("/logs?limit={}", self.limit);
        if let Some(ref log_type) = self.log_type {
            let _ = write!(
                endpoint,
                "&type={}",
                crate::api::encode_query_param(log_type)
            );
        }
        if let Some(ref project_id) = project_id {
            let _ = write!(
                endpoint,
                "&project_id={}",
                crate::api::encode_query_param(project_id)
            );
        }

        // Try to fetch logs from the API
        match client.get::<LogsResponse>(&endpoint) {
            Ok(response) => {
                if self.json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&response.logs).unwrap_or_default()
                    );
                } else {
                    self.print_logs(&response.logs);

                    if response.has_more {
                        println!();
                        Output::info(&format!(
                            "Showing {} of {} logs. Use --limit to see more.",
                            response.logs.len(),
                            response.total
                        ));
                    }
                }
            },
            Err(e) => {
                // Graceful fallback if the logs endpoint isn't available yet
                if !self.json {
                    Output::warning(&format!("Could not fetch logs from API: {e}"));
                    println!();
                    println!("View your complete activity logs at:");
                    println!("  https://app.promptguard.co/dashboard/activity");

                    if let Some(ref project_id) = project_id {
                        println!("\nProject: {project_id}");
                    }

                    println!("\nFor real-time monitoring:");
                    println!("  Visit the dashboard at https://app.promptguard.co/dashboard");
                } else {
                    return Err(PromptGuardError::Api(format!("Failed to fetch logs: {e}")));
                }
            },
        }

        Ok(())
    }

    fn print_logs(&self, logs: &[LogEntry]) {
        if logs.is_empty() {
            println!();
            Output::info("No logs found.");
            return;
        }

        println!();
        println!("Recent Activity:");
        println!("─────────────────────────────────────────────────────────────");

        for log in logs {
            let icon = match log.log_type.as_str() {
                "security" | "threat" => "🚨",
                "block" => "🚫",
                "allow" => "✅",
                "request" => "📤",
                "response" => "📥",
                "error" => "❌",
                _ => "📋",
            };

            // Truncate to readable format (char-boundary safe: the API could
            // return a non-ASCII timestamp string)
            let timestamp = Output::truncate_chars(&log.timestamp, 19);

            print!("{} [{}] {}", icon, timestamp, log.log_type.to_uppercase());

            if let Some(ref decision) = log.decision {
                print!(" - {decision}");
            }

            if let Some(ref threat_type) = log.threat_type {
                print!(" ({threat_type})");
            }

            if let Some(confidence) = log.confidence {
                print!(" [{:.0}%]", confidence * 100.0);
            }

            if let Some(latency) = log.latency_ms {
                print!(" {latency}ms");
            }

            println!();

            if let Some(ref message) = log.message {
                println!("   {message}");
            }
        }

        println!("─────────────────────────────────────────────────────────────");
    }
}
