use crate::backup::BackupManager;
use crate::config::ConfigManager;
use crate::error::{PromptGuardError, Result};
use crate::output::Output;
use crate::scanner::FileScanner;
use crate::types::Provider;

pub struct ApplyCommand {
    pub yes: bool,
}

impl ApplyCommand {
    pub fn execute(&self) -> Result<()> {
        Output::header("Apply Configuration");

        let config_manager = ConfigManager::new(None)?;
        if !config_manager.exists() {
            return Err(PromptGuardError::NotInitialized);
        }

        let mut config = config_manager.load()?;

        println!("\nThis will re-apply PromptGuard transformations to:");
        println!("  • Proxy URL: {}", config.proxy_url);
        println!("  • Providers: {}", config.providers.join(", "));

        if !self.yes && !Output::confirm("Proceed?", true)? {
            return Ok(());
        }

        Output::section("Scanning files...", "📁");

        let root_path = std::env::current_dir()?;
        let scanner = FileScanner::new(&root_path, Some(config.exclude_patterns.clone()))?;
        let files = scanner.scan_files(None)?;

        Output::step(&format!("Scanning {} files...", files.len()));

        // Detect SDK usage
        let providers_to_check: Vec<Provider> = config
            .providers
            .iter()
            .filter_map(|p| Provider::parse(p))
            .collect();

        let detection_results = super::detect_providers_in_files(&files, &providers_to_check);

        if detection_results.is_empty() {
            Output::warning("No SDK instances found to transform.");
            return Ok(());
        }

        Output::section("Applying transformations...", "🔧");

        let backup_manager = if config.backup_enabled {
            Some(BackupManager::new(Some(config.backup_extension.clone())))
        } else {
            None
        };

        let outcome = super::run_transform_pipeline(
            &detection_results,
            &root_path,
            super::TransformMode::Apply(backup_manager.as_ref()),
            &config.proxy_url,
            &config.env_var_name,
            |_provider, file_path, modified| {
                if modified {
                    let rel_path = file_path.strip_prefix(&root_path).unwrap_or(file_path);
                    Output::step(&format!("✓ {}", rel_path.display()));
                }
            },
        );

        // Record when transformations were last applied (surfaced by
        // `promptguard status`) and which backups exist (consulted by
        // `disable` to restore only PromptGuard-created files).
        for backup in outcome.backups_created {
            if !config.metadata.backups.contains(&backup) {
                config.metadata.backups.push(backup);
            }
        }
        config.metadata.last_applied = Some(chrono::Utc::now());
        config_manager.save(&config)?;

        println!();
        Output::success("Configuration applied!");
        println!("\n  • {} files modified", outcome.files_modified.len());

        Ok(())
    }
}
