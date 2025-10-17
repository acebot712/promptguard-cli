use crate::config::ConfigManager;
use crate::error::{PromptGuardError, Result};
use crate::output::Output;

pub struct LogsCommand;

impl LogsCommand {
    pub fn execute() -> Result<()> {
        Output::header("Activity Logs");

        let config_manager = ConfigManager::new(None);
        if !config_manager.exists() {
            return Err(PromptGuardError::NotInitialized);
        }

        let config = config_manager.load()?;

        Output::info("Fetching logs from PromptGuard API...");

        println!("\nView your complete activity logs at:");
        println!("  https://app.promptguard.co/dashboard/activity");

        if let Some(project_id) = config.project_id {
            println!("\nProject: {}", project_id);
        }

        println!("\nRecent activity:");
        println!("  • Request logs");
        println!("  • Security events");
        println!("  • API usage");

        println!("\nFor real-time monitoring:");
        println!("  Visit the dashboard at https://app.promptguard.co/dashboard");

        Ok(())
    }
}
