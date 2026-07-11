use crate::api::PromptGuardClient;
use crate::config::ConfigManager;
use crate::error::{PromptGuardError, Result};
use crate::output::Output;

pub struct TestCommand;

impl TestCommand {
    pub fn execute() -> Result<()> {
        Output::header("Test PromptGuard Configuration");

        let config_manager = ConfigManager::new(None)?;
        if !config_manager.exists() {
            return Err(PromptGuardError::NotInitialized);
        }

        let config = config_manager.load()?;

        println!("\nTesting configuration...");
        Output::section("API Key Validation", "🔑");

        let client =
            PromptGuardClient::new(config.api_key.clone(), Some(config.proxy_url.clone()))?;

        // Reachability first (unauthenticated /health probe)
        match client.health_check() {
            Ok(()) => {
                Output::success("✓ Proxy endpoint is reachable");
            },
            Err(e) => {
                Output::warning(&format!("✗ Connection failed: {e}"));
                println!("\nPossible issues:");
                println!("  • Network connectivity");
                println!("  • Proxy endpoint unavailable");
                return Ok(());
            },
        }

        // Then validate the key against an authenticated endpoint. The
        // /health probe succeeds for any key, so it says nothing about
        // whether the key itself is valid.
        match client.validate_credentials() {
            Ok(()) => {
                Output::success("✓ API key is valid");
            },
            Err(crate::error::PromptGuardError::Auth(msg)) => {
                Output::error(&format!("✗ API key rejected: {msg}"));
                println!("\nCheck your key at https://app.promptguard.co/settings/api-keys");
                return Ok(());
            },
            Err(e) => {
                Output::warning(&format!("✗ Could not validate API key: {e}"));
                return Ok(());
            },
        }

        println!();
        Output::section("Configuration Check", "⚙️");

        if config.enabled {
            Output::success("✓ PromptGuard is enabled");
        } else {
            Output::warning("✗ PromptGuard is disabled");
        }

        println!("  Providers: {}", config.providers.join(", "));
        println!("  Proxy: {}", config.proxy_url);

        println!();
        Output::success("Configuration test complete!");

        println!("\nNext steps:");
        println!("  • Run your application");
        println!("  • Monitor requests: https://app.promptguard.co/dashboard");
        println!("  • View logs: promptguard logs");

        Ok(())
    }
}
