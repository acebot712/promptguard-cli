use crate::config::ConfigManager;
use crate::error::Result;
use crate::output::Output;

pub struct StatusCommand {
    pub json: bool,
}

impl StatusCommand {
    pub fn execute(&self) -> Result<()> {
        if !self.json {
            Output::header("PromptGuard Status");
        }

        let config_manager = ConfigManager::new(None);

        if !config_manager.exists() {
            if self.json {
                println!("{{\"initialized\": false, \"status\": \"not_initialized\"}}");
            } else {
                println!("\nStatus: ⊘ Not initialized\n");
                println!("To get started: promptguard init");
            }
            return Ok(());
        }

        let config = config_manager.load()?;

        if self.json {
            let output = serde_json::json!({
                "initialized": true,
                "status": if config.metadata.last_applied.is_some() { "active" } else { "disabled" },
                "api_key": Output::mask_api_key(&config.api_key),
                "proxy_url": config.proxy_url,
                "configuration": {
                    "config_file": ".promptguard.json",
                    "last_applied": config.metadata.last_applied,
                    "files_managed": config.metadata.files_managed.len(),
                    "managed_files": config.metadata.files_managed,
                    "providers": config.providers,
                    "backup_enabled": config.backup_enabled,
                    "env_file": config.env_file,
                    "framework": config.framework,
                    "exclude_patterns": config.exclude_patterns,
                    "cli_version": config.metadata.cli_version,
                    "backups": config.metadata.backups,
                }
            });
            println!("{}", serde_json::to_string_pretty(&output)?);
        } else {
            println!("\nStatus: ✓ Active");
            println!("API Key: {} (configured)", Output::mask_api_key(&config.api_key));
            println!("Proxy URL: {}", config.proxy_url);

            println!("\nConfiguration:");
            println!("  • Config file: .promptguard.json");
            if let Some(last_applied) = config.metadata.last_applied {
                println!("  • Last applied: {}", last_applied.format("%Y-%m-%d %H:%M:%S"));
            }
            println!("  • Files managed: {}", config.metadata.files_managed.len());
            println!("  • Providers: {}", config.providers.join(", "));

            println!("\nView full dashboard: https://app.promptguard.co/dashboard");
        }

        Ok(())
    }
}
