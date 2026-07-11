use crate::api::PromptGuardClient;
use crate::auth::{save_credentials, GlobalCredentials};
use crate::error::{PromptGuardError, Result};
use crate::output::Output;

pub struct LoginCommand {
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub json: bool,
}

impl LoginCommand {
    pub fn execute(&self) -> Result<()> {
        let api_key = if let Some(key) = &self.api_key {
            // `--api-key -` reads the key from stdin
            super::resolve_api_key_flag(key)?
        } else {
            Output::info("Log in to PromptGuard. Get your API key at https://app.promptguard.co");
            Output::info("Note: input is not hidden — prefer 'promptguard login --api-key -' piped from a secret manager if others can see your terminal.");
            Output::input("API key")?
        };

        if api_key.trim().is_empty() {
            Output::error("API key cannot be empty");
            return Ok(());
        }

        let base_url = self.base_url.clone();

        // Validate the key against an authenticated endpoint. `/health` is
        // unauthenticated and would "succeed" for any garbage key, so we hit
        // `validate_credentials` (GET /projects) which returns 401/403 on a
        // bad key.
        Output::info("Validating API key...");
        let client = PromptGuardClient::new(api_key.clone(), base_url.clone())?;
        match client.validate_credentials() {
            Ok(()) => Output::success("API key is valid"),
            // Authentication failure (401/403): the key is bad. Reject it —
            // do NOT save and do NOT claim success.
            Err(PromptGuardError::Auth(msg)) => {
                if self.json {
                    let result = serde_json::json!({
                        "status": "auth_failed",
                        "error": msg,
                    });
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&result).unwrap_or_default()
                    );
                } else {
                    Output::error(&format!("Authentication failed: {msg}"));
                    Output::info(
                        "Check your API key at https://app.promptguard.co/settings/api-keys",
                    );
                }
                // The styled error above is the user-facing report; return the
                // sentinel so main() exits non-zero without re-printing it
                // (matches the init key-rejection path).
                return Err(PromptGuardError::AlreadyReported);
            },
            // Network/other failure: the key may be fine but we can't reach the
            // API. Offer to save with an explicit warning; don't claim "Logged in".
            Err(e) => {
                Output::warning(&format!(
                    "Could not reach PromptGuard to verify the key: {e}"
                ));
                let proceed = if self.api_key.is_some() {
                    // Non-interactive (key passed via flag): save with a warning.
                    true
                } else {
                    Output::confirm("Save the key anyway (unverified)?", false)?
                };

                if !proceed {
                    Output::info("Aborted — credentials were not saved.");
                    return Ok(());
                }

                let creds = GlobalCredentials {
                    api_key,
                    base_url,
                    active_project: None,
                };
                save_credentials(&creds)?;

                if self.json {
                    let result = serde_json::json!({
                        "status": "saved_unverified",
                        "credentials_path": "~/.promptguard/credentials.json",
                        "warning": "API key saved without verification (network error)",
                    });
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&result).unwrap_or_default()
                    );
                } else {
                    Output::warning(
                        "Key saved unverified — run 'promptguard whoami' once your network is back.",
                    );
                }
                return Ok(());
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
