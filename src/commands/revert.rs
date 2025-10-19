use crate::backup::BackupManager;
use crate::config::ConfigManager;
use crate::env::EnvManager;
use crate::error::Result;
use crate::output::Output;

pub struct RevertCommand {
    pub yes: bool,
}

impl RevertCommand {
    pub fn execute(&self) -> Result<()> {
        Output::header("Revert PromptGuard");

        let config_manager = ConfigManager::new(None);
        if !config_manager.exists() {
            Output::warning("No PromptGuard configuration found. Nothing to revert.");
            return Ok(());
        }

        let config = config_manager.load()?;

        println!("\nThis will:");
        println!("  • Restore all backup files (*.bak)");
        println!("  • Remove PROMPTGUARD_API_KEY from .env");
        println!("  • Delete .promptguard.json");
        println!("\nYour code will be reverted to its original state.");

        if !self.yes && !Output::confirm("Continue with revert?", false) {
            Output::info("Revert cancelled");
            return Ok(());
        }

        let root_path = std::env::current_dir()?;
        let backup_manager = BackupManager::new(Some(config.backup_extension.clone()));

        // Restore backups
        let backups = backup_manager.list_backups(&root_path);
        let mut restored_count = 0;

        Output::section("Restoring backups...", "📦");

        for backup_path in &backups {
            if let Some(original_path_str) = backup_path.to_str() {
                if let Some(original_str) = original_path_str.strip_suffix(&config.backup_extension)
                {
                    let original_path = std::path::PathBuf::from(original_str);
                    if backup_manager.restore_backup(&original_path).is_ok() {
                        Output::step(&format!("Restored {}", original_path.display()));
                        restored_count += 1;
                    }
                }
            }
        }

        // Delete backup files
        let deleted = backup_manager.delete_all_backups(&root_path)?;
        Output::step(&format!("Deleted {} backup files", deleted.len()));

        // Remove API key from .env
        let env_path = root_path.join(&config.env_file);
        if EnvManager::remove_key(&env_path, &config.env_var_name)? {
            Output::step(&format!(
                "Removed {} from {}",
                config.env_var_name, config.env_file
            ));
        }

        // Delete config file
        config_manager.delete()?;
        Output::step("Deleted .promptguard.json");

        println!();
        Output::success("PromptGuard has been completely removed!");
        println!("\n  • {restored_count} files restored");
        println!("  • {} backups deleted", deleted.len());
        println!("  • Configuration removed");
        println!("\nYour project is back to its original state.");

        Ok(())
    }
}
