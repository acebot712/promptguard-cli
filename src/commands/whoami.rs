use crate::api::PromptGuardClient;
use crate::auth::{load_credentials, resolve_api_key, resolve_base_url};
use crate::error::Result;
use crate::output::Output;

pub struct WhoamiCommand {
    pub json: bool,
}

impl WhoamiCommand {
    pub fn execute(&self) -> Result<()> {
        let api_key = if let Ok(key) = resolve_api_key() {
            key
        } else {
            if self.json {
                let result = serde_json::json!({
                    "authenticated": false,
                    "error": "Not logged in"
                });
                println!(
                    "{}",
                    serde_json::to_string_pretty(&result).unwrap_or_default()
                );
            } else {
                Output::error("Not logged in. Run 'promptguard login' to authenticate.");
            }
            return Ok(());
        };

        let base_url = resolve_base_url();
        let masked_key = Output::mask_api_key(&api_key);

        // Determine the source of the key
        let source = if std::env::var("PROMPTGUARD_API_KEY").is_ok() {
            "environment variable (PROMPTGUARD_API_KEY)"
        } else if crate::config::ConfigManager::new(None)
            .ok()
            .and_then(|m| m.load().ok())
            .is_some()
        {
            "project config (.promptguard.json)"
        } else {
            "global credentials (~/.promptguard/credentials.json)"
        };

        let active_project = load_credentials()
            .ok()
            .flatten()
            .and_then(|c| c.active_project);

        // Check API connectivity
        let client = PromptGuardClient::new(api_key, Some(base_url.clone()))?;
        let connected = client.health_check().is_ok();

        if self.json {
            let result = serde_json::json!({
                "authenticated": true,
                "api_key": masked_key,
                "source": source,
                "base_url": base_url,
                "active_project": active_project,
                "api_reachable": connected,
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&result).unwrap_or_default()
            );
        } else {
            Output::header("PromptGuard Identity");
            Output::step(&format!("API Key: {masked_key}"));
            Output::step(&format!("Source: {source}"));
            Output::step(&format!("API: {base_url}"));
            if let Some(ref proj) = active_project {
                Output::step(&format!("Active Project: {proj}"));
            }
            if connected {
                Output::success("API is reachable");
            } else {
                Output::warning("API is unreachable — check your network or key");
            }
        }

        Ok(())
    }
}
