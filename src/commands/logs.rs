use crate::api::PromptGuardClient;
use crate::config::ConfigManager;
use crate::error::{PromptGuardError, Result};
use crate::output::Output;
use serde::{Deserialize, Serialize};

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
    /// Reserved for future streaming logs functionality
    #[allow(dead_code)]
    pub follow: bool,
}

impl Default for LogsCommand {
    fn default() -> Self {
        Self {
            limit: 20,
            log_type: None,
            json: false,
            follow: false,
        }
    }
}

impl LogsCommand {
    pub fn execute(&self) -> Result<()> {
        let config_manager = ConfigManager::new(None)?;
        if !config_manager.exists() {
            return Err(PromptGuardError::NotInitialized);
        }

        let config = config_manager.load()?;
        let client = PromptGuardClient::new(config.api_key, Some(config.proxy_url))?;

        if !self.json {
            Output::header("Activity Logs");
            Output::info("Fetching logs from PromptGuard API...");
        }

        // Build query parameters
        let mut endpoint = format!("/logs?limit={}", self.limit);
        if let Some(ref log_type) = self.log_type {
            endpoint.push_str(&format!("&type={}", log_type));
        }
        if let Some(ref project_id) = config.project_id {
            endpoint.push_str(&format!("&project_id={}", project_id));
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
                    Output::warning(&format!("Could not fetch logs from API: {}", e));
                    println!();
                    println!("View your complete activity logs at:");
                    println!("  https://app.promptguard.co/dashboard/activity");

                    if let Some(project_id) = config.project_id {
                        println!("\nProject: {project_id}");
                    }

                    println!("\nFor real-time monitoring:");
                    println!("  Visit the dashboard at https://app.promptguard.co/dashboard");
                } else {
                    return Err(PromptGuardError::Api(format!(
                        "Failed to fetch logs: {}",
                        e
                    )));
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

            let timestamp = &log.timestamp[..19.min(log.timestamp.len())]; // Truncate to readable format

            print!("{} [{}] {}", icon, timestamp, log.log_type.to_uppercase());

            if let Some(ref decision) = log.decision {
                print!(" - {}", decision);
            }

            if let Some(ref threat_type) = log.threat_type {
                print!(" ({})", threat_type);
            }

            if let Some(confidence) = log.confidence {
                print!(" [{:.0}%]", confidence * 100.0);
            }

            if let Some(latency) = log.latency_ms {
                print!(" {}ms", latency);
            }

            println!();

            if let Some(ref message) = log.message {
                println!("   {}", message);
            }
        }

        println!("─────────────────────────────────────────────────────────────");
    }
}
