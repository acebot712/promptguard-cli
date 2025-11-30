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

        // Test API key by calling health endpoint
        let client =
            PromptGuardClient::new(config.api_key.clone(), Some(config.proxy_url.clone()))?;

        match client.health_check() {
            Ok(()) => {
                Output::success("✓ API key is valid");
                Output::success("✓ Proxy endpoint is reachable");
            },
            Err(e) => {
                Output::warning(&format!("✗ Connection failed: {e}"));
                println!("\nPossible issues:");
                println!("  • Invalid API key");
                println!("  • Network connectivity");
                println!("  • Proxy endpoint unavailable");
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
