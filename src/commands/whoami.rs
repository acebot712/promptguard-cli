use crate::api::PromptGuardClient;
use crate::auth::{load_credentials, resolve_api_key, resolve_session};
use crate::error::Result;
use crate::output::Output;

pub struct WhoamiCommand {
    pub json: bool,
}

impl WhoamiCommand {
    pub fn execute(&self) -> Result<()> {
        // Not logged in at all: report gracefully. Any other resolve_session
        // error (e.g. refusing a repo-configured custom proxy) propagates.
        if resolve_api_key().is_err() {
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
        }

        let (api_key, base_url) = resolve_session()?;
        let masked_key = Output::mask_api_key(&api_key);

        // Determine the source of the key, mirroring resolve_api_key's
        // precedence: env var > project config (only if its key is non-empty)
        // > global credentials.
        let env_key_set = std::env::var("PROMPTGUARD_API_KEY").is_ok_and(|v| !v.is_empty());
        let project_key_set = crate::config::ConfigManager::new(None)
            .ok()
            .and_then(|m| m.load().ok())
            .is_some_and(|cfg| !cfg.api_key.is_empty());

        let source = if env_key_set {
            "environment variable (PROMPTGUARD_API_KEY)"
        } else if project_key_set {
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
