use crate::config::ConfigManager;
use crate::detector::detect_all_providers;
use crate::error::{PromptGuardError, Result};
use crate::output::Output;
use crate::scanner::FileScanner;
use crate::transformer;
use crate::types::Provider;
use std::collections::HashMap;
use std::path::PathBuf;

pub struct EnableCommand;

impl EnableCommand {
    pub fn execute() -> Result<()> {
        Output::header("Enable PromptGuard");

        let config_manager = ConfigManager::new(None);
        if !config_manager.exists() {
            return Err(PromptGuardError::NotInitialized);
        }

        let mut config = config_manager.load()?;

        if config.enabled {
            Output::warning("PromptGuard is already enabled");
            return Ok(());
        }

        println!("\nThis will re-enable PromptGuard by:");
        println!("  • Re-applying transformations");
        println!("  • Proxy URL: {}", config.proxy_url);
        println!("  • Providers: {}", config.providers.join(", "));

        if !Output::confirm("Continue?", true) {
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
            .filter_map(|p| Provider::from_str(p))
            .collect();

        let mut detection_results: HashMap<Provider, Vec<PathBuf>> = HashMap::new();

        for file_path in &files {
            if let Ok(results) = detect_all_providers(file_path) {
                for (provider, result) in results {
                    if providers_to_check.contains(&provider) && !result.instances.is_empty() {
                        detection_results
                            .entry(provider)
                            .or_insert_with(Vec::new)
                            .push(file_path.clone());
                    }
                }
            }
        }

        if detection_results.is_empty() {
            Output::warning("No SDK instances found to transform.");
            return Ok(());
        }

        Output::section("Re-applying transformations...", "🔧");

        let mut files_modified = 0;

        for (provider, files) in &detection_results {
            let mut unique_files = files.clone();
            unique_files.sort();
            unique_files.dedup();

            for file_path in unique_files {
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
                    }
                    Err(e) => {
                        Output::warning(&format!("Failed to transform {}: {}", file_path.display(), e));
                    }
                }
            }
        }

        // Update config to mark as enabled
        config.enabled = true;
        config_manager.save(&config)?;
        Output::step("Updated configuration");

        println!();
        Output::success("PromptGuard is now enabled!");
        println!("\n  • {} files modified", files_modified);
        println!("\nYour LLM requests will now go through PromptGuard.");

        Ok(())
    }
}
