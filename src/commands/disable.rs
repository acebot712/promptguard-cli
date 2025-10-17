use crate::backup::BackupManager;
use crate::config::ConfigManager;
use crate::error::{PromptGuardError, Result};
use crate::output::Output;

pub struct DisableCommand;

impl DisableCommand {
    pub fn execute() -> Result<()> {
        Output::header("Disable PromptGuard");

        let config_manager = ConfigManager::new(None);
        if !config_manager.exists() {
            return Err(PromptGuardError::NotInitialized);
        }

        let mut config = config_manager.load()?;

        if !config.enabled {
            Output::warning("PromptGuard is already disabled");
            return Ok(());
        }

        println!("\nThis will temporarily disable PromptGuard by:");
        println!("  • Restoring all backup files");
        println!("  • Keeping configuration and backups");
        println!("\nYou can re-enable with: promptguard enable");

        if !Output::confirm("Continue?", true) {
            return Ok(());
        }

        let root_path = std::env::current_dir()?;
        let backup_manager = BackupManager::new(Some(config.backup_extension.clone()));

        // Restore backups
        let backups = backup_manager.list_backups(&root_path);
        let mut restored_count = 0;

        Output::section("Restoring original files...", "📦");

        for backup_path in &backups {
            if let Some(original_path_str) = backup_path.to_str() {
                if let Some(original_str) = original_path_str.strip_suffix(&config.backup_extension) {
                    let original_path = std::path::PathBuf::from(original_str);
                    if backup_manager.restore_backup(&original_path).is_ok() {
                        let rel_path = original_path.strip_prefix(&root_path).unwrap_or(&original_path);
                        Output::step(&format!("✓ {}", rel_path.display()));
                        restored_count += 1;
                    }
                }
            }
        }

        // Update config to mark as disabled
        config.enabled = false;
        config_manager.save(&config)?;
        Output::step("Updated configuration");

        println!();
        Output::success("PromptGuard is now disabled");
        println!("\n  • {} files restored", restored_count);
        println!("  • Configuration and backups preserved");
        println!("\nTo re-enable: promptguard enable");

        Ok(())
    }
}
