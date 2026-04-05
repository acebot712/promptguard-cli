use std::fmt::Write as _;

use crate::api::PromptGuardClient;
use crate::auth::{resolve_api_key, resolve_base_url};
use crate::error::Result;
use crate::output::Output;

pub struct EventsCommand {
    pub limit: usize,
    pub event_type: Option<String>,
    pub json: bool,
}

impl EventsCommand {
    pub fn execute(&self) -> Result<()> {
        let api_key = resolve_api_key()?;
        let base_url = resolve_base_url();
        let client = PromptGuardClient::new(api_key, Some(base_url))?;

        let mut endpoint = format!("/events?limit={}", self.limit);
        if let Some(ref t) = self.event_type {
            let _ = write!(endpoint, "&type={t}");
        }

        let events: serde_json::Value = client.get(&endpoint)?;

        if self.json {
            println!(
                "{}",
                serde_json::to_string_pretty(&events).unwrap_or_default()
            );
            return Ok(());
        }

        Output::header("Security Events");

        if let Some(arr) = events.as_array() {
            if arr.is_empty() {
                Output::info("No events found");
                return Ok(());
            }

            for event in arr {
                let event_type = event
                    .get("type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let decision = event
                    .get("decision")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?");
                let timestamp = event
                    .get("created_at")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?");
                let threat = event
                    .get("threat_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("-");

                let decision_marker = match decision {
                    "block" => "🛡️",
                    "allow" => "✅",
                    "redact" => "🔒",
                    _ => "•",
                };

                Output::step(&format!(
                    "{decision_marker} [{timestamp}] {event_type}: {decision} (threat: {threat})"
                ));
            }
        } else {
            Output::info("No events returned");
        }

        Ok(())
    }
}
