use crate::api::PromptGuardClient;
use crate::auth::{save_credentials, GlobalCredentials};
use crate::error::Result;
use crate::output::Output;

pub struct LoginCommand {
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub json: bool,
}

impl LoginCommand {
    pub fn execute(&self) -> Result<()> {
        let api_key = if let Some(key) = &self.api_key {
            key.clone()
        } else {
            Output::info("Log in to PromptGuard. Get your API key at https://app.promptguard.co");
            Output::input("API key")?
        };

        if api_key.trim().is_empty() {
            Output::error("API key cannot be empty");
            return Ok(());
        }

        let base_url = self.base_url.clone();

        // Validate the key
        Output::info("Validating API key...");
        let client = PromptGuardClient::new(api_key.clone(), base_url.clone())?;
        match client.health_check() {
            Ok(()) => Output::success("API key is valid"),
            Err(e) => {
                Output::warning(&format!("Could not verify API key: {e}"));
                Output::info("Saving key anyway — verify your network connection");
            },
        }

        let creds = GlobalCredentials {
            api_key,
            base_url,
            active_project: None,
        };
        save_credentials(&creds)?;

        if self.json {
            let result = serde_json::json!({
                "status": "authenticated",
                "credentials_path": "~/.promptguard/credentials.json"
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&result).unwrap_or_default()
            );
        } else {
            Output::success("Logged in. Credentials saved to ~/.promptguard/credentials.json");
        }

        Ok(())
    }
}
