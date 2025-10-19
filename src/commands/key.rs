use crate::config::ConfigManager;
use crate::env::EnvManager;
use crate::error::{PromptGuardError, Result};
use crate::output::Output;
use std::io::{self, Write};

pub struct KeyCommand;

impl KeyCommand {
    pub fn execute() -> Result<()> {
        Output::header("API Key Management");

        let config_manager = ConfigManager::new(None);
        if !config_manager.exists() {
            return Err(PromptGuardError::NotInitialized);
        }

        let mut config = config_manager.load()?;
        let root_path = std::env::current_dir()?;
        let env_path = root_path.join(&config.env_file);

        // Show current key (masked)
        println!("\nCurrent API key:");
        println!(
            "  {} = {}...{}",
            config.env_var_name,
            &config.api_key[..12], // pg_sk_test_
            if config.api_key.len() > 16 {
                &config.api_key[config.api_key.len() - 4..]
            } else {
                ""
            }
        );

        println!("\nOptions:");
        println!("  1. Update API key");
        println!("  2. Show full key (masked)");
        println!("  3. Rotate key");
        println!("  4. Cancel");

        print!("\nSelect option (1-4): ");
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;

        match input.trim() {
            "1" => {
                // Update API key
                print!("Enter new API key: ");
                io::stdout().flush()?;

                let mut new_key = String::new();
                io::stdin().read_line(&mut new_key)?;
                let new_key = new_key.trim().to_string();

                // Validate key format
                if !new_key.starts_with("pg_sk_test_") && !new_key.starts_with("pg_sk_prod_") {
                    return Err(PromptGuardError::InvalidApiKey);
                }

                // Update config
                config.api_key = new_key.clone();
                config_manager.save(&config)?;

                // Update .env
                EnvManager::set_key(&env_path, &config.env_var_name, &new_key)?;

                Output::success("API key updated successfully!");
                println!("\nThe new key has been saved to:");
                println!("  • .promptguard.json");
                println!("  • {}", config.env_file);
            },
            "2" => {
                // Show full key
                println!("\nFull API key:");
                println!("  {}", config.api_key);
                println!("\n⚠️  Keep this key secure. Don't share it publicly.");
            },
            "3" => {
                // Rotate key - requires API call
                Output::info("Key rotation requires API access.");
                println!("\nTo rotate your API key:");
                println!("  1. Visit: https://app.promptguard.co/settings/api-keys");
                println!("  2. Generate a new key");
                println!("  3. Run: promptguard key");
                println!("  4. Select option 1 to update");
            },
            _ => {
                Output::info("Cancelled");
            },
        }

        Ok(())
    }
}
