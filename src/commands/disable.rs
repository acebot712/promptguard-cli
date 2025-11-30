use crate::backup::BackupManager;
use crate::config::ConfigManager;
use crate::error::{PromptGuardError, Result};
use crate::output::Output;
use crate::shim::{ShimGenerator, ShimInjector};

pub struct DisableCommand;

impl DisableCommand {
    pub fn execute() -> Result<()> {
        Output::header("Disable PromptGuard");

        let config_manager = ConfigManager::new(None)?;
        if !config_manager.exists() {
            return Err(PromptGuardError::NotInitialized);
        }

        let mut config = config_manager.load()?;

        if !config.enabled {
            Output::warning("PromptGuard is already disabled");
            return Ok(());
        }

        let mode_description = if config.runtime_mode {
            "runtime shim mode"
        } else {
            "static transform mode"
        };

        println!("\nThis will temporarily disable PromptGuard ({mode_description}) by:");

        if config.runtime_mode {
            println!("  • Removing shim imports from entry points");
            println!("  • Cleaning up generated shim files");
        } else {
            println!("  • Restoring all backup files");
        }

        println!("  • Keeping configuration");
        println!(
            "\nYou can re-enable with: promptguard enable{}",
            if config.runtime_mode {
                " --runtime"
            } else {
                ""
            }
        );

        if !Output::confirm("Continue?", true)? {
            return Ok(());
        }

        let root_path = std::env::current_dir()?;

        if config.runtime_mode {
            // Remove runtime shims
            Output::section("Removing shim injections...", "🧹");

            let injector = ShimInjector::new(&root_path);
            let removed_count = injector.remove_all_injections()?;

            if removed_count > 0 {
                Output::step(&format!("✓ Removed imports from {removed_count} files"));
            }

            // Clean up shim files
            Output::section("Cleaning up shim files...", "🗑️");

            let generator = ShimGenerator::new(
                &root_path,
                String::new(), // Unused for cleanup
                String::new(), // Unused for cleanup
                vec![],        // Unused for cleanup
            );

            if generator.shims_installed() {
                generator.clean_shims()?;
                Output::step("✓ Removed .promptguard/ directory");
            }
        } else {
            // Restore backups (static mode)
            let backup_manager = BackupManager::new(Some(config.backup_extension.clone()));
            let backups = backup_manager.list_backups(&root_path);
            let mut restored_count = 0;

            Output::section("Restoring original files...", "📦");

            for backup_path in &backups {
                if let Some(original_path_str) = backup_path.to_str() {
                    if let Some(original_str) =
                        original_path_str.strip_suffix(&config.backup_extension)
                    {
                        let original_path = std::path::PathBuf::from(original_str);
                        if backup_manager.restore_backup(&original_path).is_ok() {
                            let rel_path = original_path
                                .strip_prefix(&root_path)
                                .unwrap_or(&original_path);
                            Output::step(&format!("✓ {}", rel_path.display()));
                            restored_count += 1;
                        }
                    }
                }
            }

            if restored_count > 0 {
                Output::step(&format!("Restored {restored_count} files"));
            }
        }

        // Update config to mark as disabled
        config.enabled = false;
        config_manager.save(&config)?;
        Output::step("Updated configuration");

        println!();
        Output::success("PromptGuard is now disabled");
        println!("\n  • Configuration preserved");
        println!(
            "  • To re-enable: promptguard enable{}",
            if config.runtime_mode {
                " --runtime"
            } else {
                ""
            }
        );

        Ok(())
    }
}
