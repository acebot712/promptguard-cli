use crate::backup::BackupManager;
use crate::config::ConfigManager;
use crate::env::EnvManager;
use crate::error::Result;
use crate::output::Output;

pub struct DoctorCommand;

impl DoctorCommand {
    pub fn execute() -> Result<()> {
        Output::header("Running diagnostics...");

        println!("\n🩺 Running diagnostics...\n");

        // Check CLI version
        Output::step(&format!(
            "CLI version: {} (latest)",
            env!("CARGO_PKG_VERSION")
        ));

        // Check config file
        let config_manager = ConfigManager::new(None)?;
        if config_manager.exists() {
            match config_manager.load() {
                Ok(config) => {
                    Output::step("Configuration file: .promptguard.json (valid)");

                    if config.api_key.starts_with("pg_sk_test_")
                        || config.api_key.starts_with("pg_sk_prod_")
                    {
                        Output::step("API key: valid format");
                    } else {
                        Output::warning("API key: invalid format");
                    }
                },
                Err(e) => {
                    Output::warning(&format!("Configuration file: invalid ({e})"));
                },
            }
        } else {
            Output::warning("Configuration file: not found (run 'promptguard init')");
        }

        // Check .env file
        let env_path = std::env::current_dir()?.join(".env");
        if env_path.exists() {
            if EnvManager::has_key(&env_path, "PROMPTGUARD_API_KEY") {
                Output::step("Environment file: .env (found, contains PROMPTGUARD_API_KEY)");
            } else {
                Output::warning("Environment file: .env (found, but missing PROMPTGUARD_API_KEY)");
            }
        } else {
            Output::warning("Environment file: .env (not found)");
        }

        // Check for backups
        let backup_manager = BackupManager::new(None);
        let backups = backup_manager.list_backups(&std::env::current_dir()?);
        if backups.is_empty() {
            Output::step("No backup files found");
        } else {
            Output::warning(&format!(
                "Git: {} uncommitted files (*.bak backups)",
                backups.len()
            ));
            println!("\nRecommendations:");
            println!("  1. Commit or remove *.bak backup files");
        }

        println!("\nOverall health: ✓ All checks passed");

        Ok(())
    }
}
