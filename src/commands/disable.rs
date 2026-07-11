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
            // Restore backups (static mode). Only restore the backups that
            // PromptGuard itself created (recorded in metadata.backups) —
            // never glob the tree for *.bak, which would clobber backup files
            // the user created for their own reasons and destroy their work.
            let backup_manager = BackupManager::new(Some(config.backup_extension.clone()));
            let mut restored_count = 0;

            Output::section("Restoring original files...", "📦");

            let restore_one = |rel_or_abs: &std::path::Path| -> bool {
                let backup_path = if rel_or_abs.is_absolute() {
                    rel_or_abs.to_path_buf()
                } else {
                    root_path.join(rel_or_abs)
                };
                let Some(original_str) = backup_path
                    .to_str()
                    .and_then(|s| s.strip_suffix(&config.backup_extension))
                else {
                    return false;
                };
                let original_path = std::path::PathBuf::from(original_str);
                if backup_manager.restore_backup(&original_path).is_ok() {
                    let rel_path = original_path
                        .strip_prefix(&root_path)
                        .unwrap_or(&original_path);
                    Output::step(&format!("✓ {}", rel_path.display()));
                    true
                } else {
                    false
                }
            };

            if config.metadata.backups.is_empty() {
                // Nothing recorded. Do NOT auto-restore every *.bak in the
                // tree; offer an explicit, clearly-labelled opt-in instead.
                Output::warning(
                    "No PromptGuard-created backups are recorded in .promptguard.json — \
                     nothing to restore automatically.",
                );

                let discovered = backup_manager.list_backups(&root_path);
                if !discovered.is_empty()
                    && Output::confirm(
                        &format!(
                            "Found {} '{}' file(s) in the tree that PromptGuard did not track. \
                             Restore them anyway? This may overwrite files PromptGuard never created.",
                            discovered.len(),
                            config.backup_extension
                        ),
                        false,
                    )?
                {
                    for backup_path in &discovered {
                        if restore_one(backup_path) {
                            restored_count += 1;
                        }
                    }
                }
            } else {
                for rel_backup in &config.metadata.backups {
                    if restore_one(std::path::Path::new(rel_backup)) {
                        restored_count += 1;
                    }
                }
            }

            if restored_count > 0 {
                Output::step(&format!("Restored {restored_count} files"));
            } else {
                // Be honest: nothing was actually restored. Projects
                // initialized before backups existed (or with backups
                // deleted) still have the transformations in place.
                Output::warning(
                    "No files were restored. \
                     Transformed files keep routing through PromptGuard until you \
                     revert them (e.g. 'git checkout -- .' or 'promptguard revert').",
                );
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
