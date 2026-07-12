use crate::backup::BackupManager;
use crate::config::ConfigManager;
use crate::error::{PromptGuardError, Result};
use crate::output::Output;
use crate::shim::{ShimGenerator, ShimInjector};
use std::path::{Path, PathBuf};

/// Outcome of restoring one recorded backup: only `Restored` counts toward
/// the "Restored N files" summary.
pub(crate) enum RestoreOutcome {
    /// The backup was copied back over the transformed file.
    Restored {
        /// The backup file that was restored from (so callers that fully
        /// remove `PromptGuard` — `revert` — can delete it afterwards).
        backup_path: PathBuf,
    },
    /// The backup file no longer exists — nothing was restored.
    MissingBackup(PathBuf),
    Failed,
}

/// Restore ONE backup file (given as a path to the backup itself, relative
/// to `root_path` or absolute), printing a per-file step on success.
pub(crate) fn restore_backup_file(
    rel_or_abs: &Path,
    root_path: &Path,
    backup_manager: &BackupManager,
    backup_extension: &str,
) -> RestoreOutcome {
    let backup_path = if rel_or_abs.is_absolute() {
        rel_or_abs.to_path_buf()
    } else {
        root_path.join(rel_or_abs)
    };
    let Some(original_str) = backup_path
        .to_str()
        .and_then(|s| s.strip_suffix(backup_extension))
    else {
        return RestoreOutcome::Failed;
    };
    let original_path = PathBuf::from(original_str);
    match backup_manager.restore_backup(&original_path) {
        Ok(true) => {
            let rel_path = original_path
                .strip_prefix(root_path)
                .unwrap_or(&original_path);
            Output::step(&format!("✓ {}", rel_path.display()));
            RestoreOutcome::Restored { backup_path }
        },
        Ok(false) => RestoreOutcome::MissingBackup(backup_path),
        Err(_) => RestoreOutcome::Failed,
    }
}

/// Summary of restoring the backups recorded in `.promptguard.json`.
pub(crate) struct BackupRestoreSummary {
    pub restored: usize,
    /// Recorded backups that no longer exist on disk: those files were NOT
    /// restored and keep their transformations — callers must report them.
    pub missing: Vec<PathBuf>,
}

/// Restore every backup recorded in `config.metadata.backups`.
///
/// Only backups `PromptGuard` itself created are touched — never glob the
/// tree for `*.bak`, which would clobber backup files the user created for
/// their own reasons. With `delete_backups`, each successfully restored
/// backup file is removed afterwards (used by `revert`, which removes
/// `PromptGuard` entirely; `disable` keeps them for re-enabling).
pub(crate) fn restore_recorded_backups(
    config: &crate::config::PromptGuardConfig,
    root_path: &Path,
    delete_backups: bool,
) -> BackupRestoreSummary {
    let backup_manager = BackupManager::new(Some(config.backup_extension.clone()));
    let mut summary = BackupRestoreSummary {
        restored: 0,
        missing: Vec::new(),
    };

    for rel_backup in &config.metadata.backups {
        match restore_backup_file(
            Path::new(rel_backup),
            root_path,
            &backup_manager,
            &config.backup_extension,
        ) {
            RestoreOutcome::Restored { backup_path } => {
                if delete_backups {
                    let _ = std::fs::remove_file(&backup_path);
                }
                summary.restored += 1;
            },
            RestoreOutcome::MissingBackup(p) => summary.missing.push(p),
            RestoreOutcome::Failed => {},
        }
    }

    summary
}

/// Report recorded backups that no longer exist: those files were NOT
/// restored and keep their transformations.
pub(crate) fn warn_missing_backups(missing: &[PathBuf], root_path: &Path) {
    if missing.is_empty() {
        return;
    }
    Output::warning(&format!(
        "{} recorded backup file(s) are missing and could not be restored:",
        missing.len()
    ));
    for backup_path in missing {
        let rel = backup_path.strip_prefix(root_path).unwrap_or(backup_path);
        Output::step(&format!("missing: {}", rel.display()));
    }
}

/// Remove runtime shim artifacts: shim import injections in entry points and
/// the generated `.promptguard/` directory.
///
/// Returns `(injections_removed, shim_dir_removed)`.
pub(crate) fn remove_shim_artifacts(root_path: &Path) -> Result<(usize, bool)> {
    let injector = ShimInjector::new(root_path);
    let injections_removed = injector.remove_all_injections()?;

    let generator = ShimGenerator::new(
        root_path,
        String::new(), // Unused for cleanup
        vec![],        // Unused for cleanup
    );
    let shim_dir_removed = if generator.shim_dir().exists() {
        generator.clean_shims()?;
        true
    } else {
        false
    };

    Ok((injections_removed, shim_dir_removed))
}

pub struct DisableCommand {
    /// Skip confirmation prompts (each prompt falls through to its default
    /// answer). Required for non-interactive callers — e.g. the VS Code
    /// extension spawns the CLI with piped stdin that never delivers input.
    pub yes: bool,
}

impl DisableCommand {
    pub fn execute(&self) -> Result<()> {
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

        if !self.yes && !Output::confirm("Continue?", true)? {
            return Ok(());
        }

        let root_path = std::env::current_dir()?;

        if config.runtime_mode {
            // Remove runtime shims
            Output::section("Removing shim injections...", "🧹");

            let (removed_count, shim_dir_removed) = remove_shim_artifacts(&root_path)?;

            if removed_count > 0 {
                Output::step(&format!("✓ Removed imports from {removed_count} files"));
            }
            if shim_dir_removed {
                Output::step("✓ Removed .promptguard/ directory");
            }
        } else {
            // Restore backups (static mode). Only restore the backups that
            // PromptGuard itself created (recorded in metadata.backups) —
            // never glob the tree for *.bak, which would clobber backup files
            // the user created for their own reasons and destroy their work.
            let backup_manager = BackupManager::new(Some(config.backup_extension.clone()));
            let mut restored_count = 0;
            let mut missing_backups: Vec<PathBuf> = Vec::new();

            Output::section("Restoring original files...", "📦");

            if config.metadata.backups.is_empty() {
                // Nothing recorded. Do NOT auto-restore every *.bak in the
                // tree; offer an explicit, clearly-labelled opt-in instead.
                Output::warning(
                    "No PromptGuard-created backups are recorded in .promptguard.json — \
                     nothing to restore automatically.",
                );

                // With --yes this opt-in keeps its DEFAULT answer (No):
                // "skip the prompts" must never silently opt in to restoring
                // backups PromptGuard did not create.
                let discovered = backup_manager.list_backups(&root_path);
                if !discovered.is_empty()
                    && !self.yes
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
                        match restore_backup_file(
                            backup_path,
                            &root_path,
                            &backup_manager,
                            &config.backup_extension,
                        ) {
                            RestoreOutcome::Restored { .. } => restored_count += 1,
                            RestoreOutcome::MissingBackup(p) => missing_backups.push(p),
                            RestoreOutcome::Failed => {},
                        }
                    }
                }
            } else {
                let summary = restore_recorded_backups(&config, &root_path, false);
                restored_count = summary.restored;
                missing_backups = summary.missing;
            }

            // Be honest about recorded backups that no longer exist: those
            // files were NOT restored and keep their transformations.
            warn_missing_backups(&missing_backups, &root_path);

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

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::types::Provider;
    use std::fs;
    use tempfile::TempDir;

    fn test_config() -> crate::config::PromptGuardConfig {
        crate::config::PromptGuardConfig::new(
            "pg_live_test_key_1234567890".to_string(),
            "https://api.promptguard.co/api/v1".to_string(),
            vec!["openai".to_string()],
        )
        .unwrap()
    }

    /// Recorded backups are restored; missing ones are reported, not counted.
    #[test]
    fn restore_recorded_backups_restores_and_reports_missing() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("app.py"), "transformed").unwrap();
        fs::write(dir.path().join("app.py.bak"), "original").unwrap();

        let mut config = test_config();
        config.metadata.backups = vec!["app.py.bak".to_string(), "gone.py.bak".to_string()];

        let summary = restore_recorded_backups(&config, dir.path(), false);

        assert_eq!(summary.restored, 1);
        assert_eq!(summary.missing.len(), 1);
        assert_eq!(
            fs::read_to_string(dir.path().join("app.py")).unwrap(),
            "original"
        );
        assert!(
            dir.path().join("app.py.bak").exists(),
            "without delete_backups the backup must be kept (disable re-enables later)"
        );
    }

    /// With `delete_backups` (revert), the restored backup file is removed.
    #[test]
    fn restore_recorded_backups_can_delete_backups_after_restore() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("app.py"), "transformed").unwrap();
        fs::write(dir.path().join("app.py.bak"), "original").unwrap();

        let mut config = test_config();
        config.metadata.backups = vec!["app.py.bak".to_string()];

        let summary = restore_recorded_backups(&config, dir.path(), true);

        assert_eq!(summary.restored, 1);
        assert_eq!(
            fs::read_to_string(dir.path().join("app.py")).unwrap(),
            "original"
        );
        assert!(
            !dir.path().join("app.py.bak").exists(),
            "revert must clean up the backup it restored from"
        );
    }

    /// Shim removal must strip injected imports AND delete .promptguard/.
    #[test]
    fn remove_shim_artifacts_removes_injections_and_dir() {
        let dir = TempDir::new().unwrap();

        let generator = ShimGenerator::new(
            dir.path(),
            "https://api.promptguard.co/api/v1".to_string(),
            vec![Provider::OpenAI],
        );
        generator.generate_python_shim().unwrap();

        let entry = dir.path().join("main.py");
        fs::write(&entry, "print('hi')\n").unwrap();
        let injector = ShimInjector::new(dir.path());
        assert!(injector.inject_python_shim(&entry).unwrap());
        assert!(fs::read_to_string(&entry).unwrap().contains("promptguard"));

        let (injections_removed, shim_dir_removed) = remove_shim_artifacts(dir.path()).unwrap();

        assert_eq!(injections_removed, 1);
        assert!(shim_dir_removed);
        assert!(!dir.path().join(".promptguard").exists());
        assert!(
            !fs::read_to_string(&entry).unwrap().contains("promptguard"),
            "injected import must be gone"
        );
    }

    /// A project without shims is a no-op, not an error.
    #[test]
    fn remove_shim_artifacts_is_noop_without_shims() {
        let dir = TempDir::new().unwrap();
        let (injections_removed, shim_dir_removed) = remove_shim_artifacts(dir.path()).unwrap();
        assert_eq!(injections_removed, 0);
        assert!(!shim_dir_removed);
    }
}
