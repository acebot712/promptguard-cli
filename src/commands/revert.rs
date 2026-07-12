use super::disable::{remove_shim_artifacts, restore_recorded_backups, warn_missing_backups};
use crate::config::ConfigManager;
use crate::env::EnvManager;
use crate::error::Result;
use crate::output::Output;

/// Completely remove `PromptGuard` from the project.
///
/// The command's help promises "Reverts all file changes and removes
/// configuration", so it must actually undo everything `PromptGuard` did:
/// restore the recorded backups of transformed files, remove injected shim
/// imports and the generated `.promptguard/` directory, and only then remove
/// the env entry and config. (Previously it deleted the API key and config
/// while leaving transformed files routed at the proxy — a broken app.)
pub struct RevertCommand {
    pub yes: bool,
}

impl RevertCommand {
    pub fn execute(&self) -> Result<()> {
        Output::header("Revert PromptGuard");

        let config_manager = ConfigManager::new(None)?;
        if !config_manager.exists() {
            Output::warning("No PromptGuard configuration found. Nothing to revert.");
            return Ok(());
        }

        let config = config_manager.load()?;
        let root_path = std::env::current_dir()?;
        let git_dir = root_path.join(".git");
        let shim_dir = root_path.join(".promptguard");

        let recorded_backups = config.metadata.backups.len();

        println!("\nThis will:");
        if recorded_backups > 0 {
            println!(
                "  • Restore {recorded_backups} file(s) from PromptGuard-created backups \
                 (and delete those backups)"
            );
        }
        if shim_dir.exists() {
            println!("  • Remove injected shim imports and delete .promptguard/");
        } else {
            println!("  • Remove any shim imports PromptGuard injected into entry points");
        }
        println!(
            "  • Remove {} from {}",
            config.env_var_name, config.env_file
        );
        println!("  • Delete .promptguard.json");

        if git_dir.exists() {
            println!("\nTo review the current changes first:");
            println!("  git diff                    # Review what changed");
        } else {
            println!("\n⚠️  No git repository found.");
            println!("Only PromptGuard-recorded backups can be restored automatically;");
            println!("verify the result manually afterwards.");
        }

        if !self.yes && !Output::confirm("\nContinue with revert?", true)? {
            Output::info("Revert cancelled");
            return Ok(());
        }

        // 1. Restore the files PromptGuard transformed, from the backups it
        //    recorded in metadata (never glob the tree for *.bak — that could
        //    clobber backup files the user created themselves). Since revert
        //    removes PromptGuard entirely, restored backups are deleted.
        if recorded_backups > 0 {
            Output::section("Restoring original files...", "📦");
            let summary = restore_recorded_backups(&config, &root_path, true);

            if summary.restored > 0 {
                Output::step(&format!("Restored {} file(s)", summary.restored));
            }
            warn_missing_backups(&summary.missing, &root_path);
            if summary.restored < recorded_backups {
                Output::warning(
                    "Files without a restorable backup keep their transformations — \
                     revert them with git (e.g. 'git checkout -- <file>').",
                );
            }
        } else if config.metadata.files_managed.is_empty() {
            Output::step("No transformed files recorded — nothing to restore");
        } else {
            Output::warning(
                "No PromptGuard-created backups are recorded — transformed files were \
                 NOT reverted and keep routing through the proxy. Restore them with \
                 git (e.g. 'git checkout -- .').",
            );
        }

        // 2. Remove runtime shim artifacts: injected imports + .promptguard/.
        Output::section("Removing runtime shims...", "🧹");
        let (injections_removed, shim_dir_removed) = remove_shim_artifacts(&root_path)?;
        if injections_removed > 0 {
            Output::step(&format!(
                "Removed shim imports from {injections_removed} file(s)"
            ));
        }
        if shim_dir_removed {
            Output::step("Removed .promptguard/ directory");
        }
        if injections_removed == 0 && !shim_dir_removed {
            Output::step("No shim artifacts found");
        }

        // 3. Remove the API key from the env file — only after the code no
        //    longer routes through the proxy, so we never leave a transformed
        //    app without its key.
        Output::section("Removing configuration...", "🗑️");
        let env_path = root_path.join(&config.env_file);
        if EnvManager::remove_key(&env_path, &config.env_var_name)? {
            Output::step(&format!(
                "Removed {} from {}",
                config.env_var_name, config.env_file
            ));
        }

        // 4. Delete the config file.
        config_manager.delete()?;
        Output::step("Deleted .promptguard.json");

        println!();
        Output::success("PromptGuard removed!");

        if git_dir.exists() {
            println!("\nRun 'git diff' / 'git status' to verify the final state.");
        }

        Ok(())
    }
}
