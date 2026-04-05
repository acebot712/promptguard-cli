use crate::config::ConfigManager;
use crate::error::{PromptGuardError, Result};
use crate::output::Output;

pub struct ConfigCommand {
    pub json: bool,
}

impl ConfigCommand {
    pub fn execute(&self) -> Result<()> {
        let config_manager = ConfigManager::new(None)?;
        if !config_manager.exists() {
            return Err(PromptGuardError::NotInitialized);
        }

        let config = config_manager.load()?;

        if self.json {
            let result = serde_json::json!({
                "version": config.version,
                "enabled": config.enabled,
                "proxy_url": config.proxy_url,
                "providers": config.providers,
                "env_file": config.env_file,
                "env_var_name": config.env_var_name,
                "backup_enabled": config.backup_enabled,
                "backup_extension": config.backup_extension,
                "framework": config.framework,
                "project_id": config.project_id,
                "runtime_mode": config.runtime_mode,
                "exclude_patterns": config.exclude_patterns,
                "config_path": config_manager.config_path().display().to_string(),
                "metadata": {
                    "cli_version": config.metadata.cli_version,
                    "files_managed": config.metadata.files_managed.len(),
                },
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&result).unwrap_or_default()
            );
            return Ok(());
        }

        Output::header("PromptGuard Configuration");

        println!("\nConfiguration:");
        println!("  Version: {}", config.version);
        println!(
            "  Status: {}",
            if config.enabled {
                "Enabled ✓"
            } else {
                "Disabled"
            }
        );
        println!("  Proxy URL: {}", config.proxy_url);
        println!("  Providers: {}", config.providers.join(", "));
        println!("  Environment file: {}", config.env_file);
        println!("  API key variable: {}", config.env_var_name);
        println!(
            "  Backups: {}",
            if config.backup_enabled {
                "Enabled"
            } else {
                "Disabled"
            }
        );
        if config.backup_enabled {
            println!("  Backup extension: {}", config.backup_extension);
        }

        if let Some(ref framework) = config.framework {
            println!("  Framework: {framework}");
        }

        if let Some(ref project_id) = config.project_id {
            println!("  Project ID: {project_id}");
        }

        println!("\nExclude patterns:");
        for pattern in &config.exclude_patterns {
            println!("  • {pattern}");
        }

        println!("\nMetadata:");
        println!("  CLI version: {}", config.metadata.cli_version);
        if let Some(last_applied) = config.metadata.last_applied {
            println!(
                "  Last applied: {}",
                last_applied.format("%Y-%m-%d %H:%M:%S UTC")
            );
        }
        if !config.metadata.files_managed.is_empty() {
            println!("  Files managed: {}", config.metadata.files_managed.len());
        }

        println!(
            "\nConfiguration file: {}",
            config_manager.config_path().display()
        );

        println!("\nCommands:");
        println!("  promptguard disable  - Temporarily disable PromptGuard");
        println!("  promptguard enable   - Re-enable PromptGuard");
        println!("  promptguard revert   - Completely remove PromptGuard");

        Ok(())
    }
}
