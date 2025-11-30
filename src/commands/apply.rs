use crate::backup::BackupManager;
use crate::config::ConfigManager;
use crate::detector::detect_all_providers;
use crate::error::{PromptGuardError, Result};
use crate::output::Output;
use crate::scanner::FileScanner;
use crate::transformer;
use crate::types::Provider;
use std::collections::HashMap;
use std::path::PathBuf;

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

        let config = config_manager.load()?;

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

        let mut detection_results: HashMap<Provider, Vec<PathBuf>> = HashMap::new();

        for file_path in &files {
            if let Ok(results) = detect_all_providers(file_path) {
                for (provider, result) in results {
                    if providers_to_check.contains(&provider) && !result.instances.is_empty() {
                        detection_results
                            .entry(provider)
                            .or_default()
                            .push(file_path.clone());
                    }
                }
            }
        }

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

        let mut files_modified = 0;

        for (provider, files) in &detection_results {
            let mut unique_files = files.clone();
            unique_files.sort();
            unique_files.dedup();

            for file_path in unique_files {
                // Create backup BEFORE transformation
                if let Some(ref bm) = backup_manager {
                    let _ = bm.create_backup(&file_path);
                }

                match transformer::transform_file(
                    &file_path,
                    *provider,
                    &config.proxy_url,
                    &config.env_var_name,
                ) {
                    Ok(result) => {
                        if result.modified {
                            files_modified += 1;
                            let rel_path = file_path.strip_prefix(&root_path).unwrap_or(&file_path);
                            Output::step(&format!("✓ {}", rel_path.display()));
                        }
                    },
                    Err(e) => {
                        Output::warning(&format!(
                            "Failed to transform {}: {}",
                            file_path.display(),
                            e
                        ));
                    },
                }
            }
        }

        println!();
        Output::success("Configuration applied!");
        println!("\n  • {files_modified} files modified");

        Ok(())
    }
}
