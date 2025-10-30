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
        let root_path = std::env::current_dir()?;
        let git_dir = root_path.join(".git");

        println!("\nThis will:");
        println!("  • Remove PROMPTGUARD_API_KEY from .env");
        println!("  • Delete .promptguard.json");

        if git_dir.exists() {
            println!("\nTo revert your code changes:");
            println!("  git diff                    # Review what changed");
            println!("  git checkout -- .           # Revert all changes");
            println!("  git checkout -- <file>      # Revert specific file");
        } else {
            println!("\n⚠️  No git repository found.");
            println!("Without version control, you cannot automatically revert code changes.");
            println!("You'll need to manually undo the transformations.");
        }

        if !self.yes && !Output::confirm("\nContinue with cleanup?", true) {
            Output::info("Revert cancelled");
            return Ok(());
        }

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
        Output::success("PromptGuard configuration removed!");

        if git_dir.exists() {
            println!("\nNext: Use git to revert your code changes (see commands above)");
        }

        Ok(())
    }
}
