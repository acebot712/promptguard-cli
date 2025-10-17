use crate::backup::BackupManager;
use crate::config::{ConfigManager, PromptGuardConfig};
use crate::detector::detect_all_providers;
use crate::env::EnvManager;
use crate::error::Result;
use crate::output::Output;
use crate::scanner::FileScanner;
use crate::transformer;
use crate::types::Provider;
use std::collections::HashMap;
use std::path::PathBuf;

pub struct InitCommand {
    pub provider: Vec<String>,
    pub api_key: Option<String>,
    pub base_url: String,
    pub env_file: String,
    pub auto: bool,
    pub dry_run: bool,
    pub no_backup: bool,
    pub exclude: Vec<String>,
    pub framework: Option<String>,
}

impl InitCommand {
    pub fn execute(&self) -> Result<()> {
        if !self.dry_run {
            Output::header(&format!("🛡️  PromptGuard CLI v{}", env!("CARGO_PKG_VERSION")));
        }

        // Check if already initialized
        let config_manager = ConfigManager::new(None);
        if config_manager.exists() && !self.dry_run {
            Output::warning("PromptGuard is already initialized in this project.");
            if !self.auto && !Output::confirm("Reinitialize?", false) {
                return Ok(());
            }
        }

        // Get API key
        let api_key = self.get_api_key()?;

        // Scan project
        Output::section("Scanning project...", "📁");

        let root_path = std::env::current_dir()?;
        let scanner = FileScanner::new(
            &root_path,
            if self.exclude.is_empty() {
                None
            } else {
                Some(self.exclude.clone())
            },
        )?;

        if let Some(git_root) = scanner.find_git_root() {
            Output::step(&format!("Found .git directory (root: {})", git_root.display()));
        }

        let framework = self.framework.clone().or_else(|| scanner.detect_framework());
        if let Some(ref fw) = framework {
            Output::step(&format!("Detected framework: {}", fw));
        }

        let files = scanner.scan_files(None)?;
        Output::step(&format!("Scanning {} files...", files.len()));

        // Detect SDK usage
        Output::section("Detected LLM SDKs:", "🔍");

        let providers_to_check: Vec<Provider> = if self.provider.is_empty() || self.provider.contains(&"all".to_string()) {
            vec![Provider::OpenAI, Provider::Anthropic, Provider::Cohere, Provider::HuggingFace]
        } else {
            self.provider
                .iter()
                .filter_map(|p| Provider::from_str(p))
                .collect()
        };

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
            Output::error("No LLM SDKs detected in this project.");
            println!("\nPromptGuard works with:");
            println!("  • OpenAI SDK (openai)");
            println!("  • Anthropic SDK (@anthropic-ai/sdk)");
            println!("  • Cohere SDK (cohere-ai)");
            println!("  • HuggingFace SDK (@huggingface/inference)");
            println!("\nMake sure you've installed one of these SDKs.");
            return Ok(());
        }

        for (provider, files) in &detection_results {
            let mut unique_files = files.clone();
            unique_files.sort();
            unique_files.dedup();

            println!("   • {} SDK ({} files)", provider.class_name(), unique_files.len());
            for (i, file) in unique_files.iter().take(5).enumerate() {
                let rel_path = file.strip_prefix(&root_path).unwrap_or(file);
                Output::step(&format!("{}", rel_path.display()));
            }
            if unique_files.len() > 5 {
                Output::step(&format!("... and {} more", unique_files.len() - 5));
            }
        }

        // Show configuration
        println!();
        Output::section("Configuration:", "📝");
        println!("   • Proxy URL: {}", self.base_url);
        println!("   • Backup files: {}", if self.no_backup { "No" } else { "Yes (*.bak)" });
        println!("   • Environment: {}", self.env_file);

        // Confirm changes
        if !self.auto && !self.dry_run {
            println!();
            if !Output::confirm("Apply these changes?", true) {
                return Ok(());
            }
        }

        if self.dry_run {
            println!();
            Output::info("DRY RUN - no changes will be made");
        }

        // Apply transformations
        println!();
        Output::section(
            if self.dry_run { "Preview:" } else { "Applying changes..." },
            "🔧",
        );

        let backup_manager = if self.no_backup {
            None
        } else {
            Some(BackupManager::new(None))
        };

        let mut files_modified = Vec::new();

        for (provider, files) in &detection_results {
            let mut unique_files = files.clone();
            unique_files.sort();
            unique_files.dedup();

            for file_path in unique_files {
                // Create backup BEFORE transformation
                if !self.dry_run {
                    if let Some(ref bm) = backup_manager {
                        let _ = bm.create_backup(&file_path);
                    }
                }

                match transformer::transform_file(
                    &file_path,
                    *provider,
                    &self.base_url,
                    "PROMPTGUARD_API_KEY",
                ) {
                    Ok(result) => {
                        if result.modified && !self.dry_run {
                            files_modified.push(file_path.clone());
                        }

                        let rel_path = file_path.strip_prefix(&root_path).unwrap_or(&file_path);

                        if result.modified {
                            Output::step(&format!(
                                "{} (added {} for {})",
                                rel_path.display(),
                                provider.base_url_param(),
                                provider.as_str()
                            ));
                        } else {
                            Output::excluded(&format!("{} (no changes needed)", rel_path.display()));
                        }
                    }
                    Err(e) => {
                        Output::warning(&format!("Failed to transform {}: {}", file_path.display(), e));
                    }
                }
            }
        }

        // Update .env file
        let env_path = root_path.join(&self.env_file);
        if !self.dry_run {
            EnvManager::add_or_update_key(&env_path, "PROMPTGUARD_API_KEY", &api_key)?;
            Output::step(&format!("{} (added PROMPTGUARD_API_KEY)", self.env_file));
        } else {
            Output::step(&format!("{} (would add PROMPTGUARD_API_KEY)", self.env_file));
        }

        // Save configuration
        if !self.dry_run {
            let providers_list: Vec<String> = detection_results.keys().map(|p| p.as_str().to_string()).collect();

            let mut config = PromptGuardConfig::new(api_key, self.base_url.clone(), providers_list)?;

            config.exclude_patterns = if self.exclude.is_empty() {
                crate::config::default_exclude_patterns()
            } else {
                self.exclude.clone()
            };

            config.backup_enabled = !self.no_backup;
            config.env_file = self.env_file.clone();
            config.framework = framework;

            config.metadata.files_managed = files_modified
                .iter()
                .map(|f| f.strip_prefix(&root_path).unwrap_or(f).to_string_lossy().to_string())
                .collect();

            if let Some(ref bm) = backup_manager {
                config.metadata.backups = files_modified
                    .iter()
                    .map(|f| {
                        let backup_path = bm.backup_path(f);
                        backup_path.strip_prefix(&root_path)
                            .unwrap_or(&backup_path)
                            .to_string_lossy()
                            .to_string()
                    })
                    .collect();
            }

            config_manager.save(&config)?;
            Output::step(".promptguard.json (created)");
        } else {
            Output::step(".promptguard.json (would be created)");
        }

        // Summary
        println!();
        if !self.dry_run {
            Output::success("PromptGuard is now active!");
            println!("\nNext steps:");
            println!("  • Run your app normally - all LLM requests now go through PromptGuard");
            println!("  • View logs: promptguard logs --follow");
            println!("  • Check dashboard: https://app.promptguard.co/dashboard");
        } else {
            println!("✓ {} files would be modified", files_modified.len());
            println!("✓ 1 file would be created (.promptguard.json)");
            if !self.no_backup {
                println!("✓ {} backups would be created", files_modified.len());
            }
            println!("\nTo apply: promptguard init");
        }

        println!("\nNeed help? https://docs.promptguard.co/cli");

        Ok(())
    }

    fn get_api_key(&self) -> Result<String> {
        let api_key = if let Some(ref key) = self.api_key {
            key.clone()
        } else if let Ok(key) = std::env::var("PROMPTGUARD_API_KEY") {
            key
        } else if !self.auto && !self.dry_run {
            println!();
            Output::input("🔑 Paste your PromptGuard API key")
        } else if self.dry_run {
            return Ok("pg_sk_test_demo123456789012345678901234".to_string());
        } else {
            return Err(crate::error::PromptGuardError::Custom(
                "API key required in non-interactive mode. Use --api-key flag or set PROMPTGUARD_API_KEY".to_string(),
            ));
        };

        if api_key.is_empty() {
            return Err(crate::error::PromptGuardError::Custom("API key is required".to_string()));
        }

        // Validate API key format
        if !api_key.starts_with("pg_sk_test_") && !api_key.starts_with("pg_sk_prod_") {
            return Err(crate::error::PromptGuardError::InvalidApiKey);
        }

        Ok(api_key)
    }
}
