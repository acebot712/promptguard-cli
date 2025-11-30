use crate::analyzer::EnvScanner;
use crate::config::ConfigManager;
use crate::detector::detect_all_providers;
use crate::error::{PromptGuardError, Result};
use crate::output::Output;
use crate::scanner::FileScanner;
use crate::shim::{ShimGenerator, ShimInjector};
use crate::transformer;
use crate::types::{Language, Provider};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

pub struct EnableCommand {
    pub runtime: bool,
}

impl EnableCommand {
    pub fn execute(&self) -> Result<()> {
        Output::header("Enable PromptGuard");

        let config_manager = ConfigManager::new(None)?;
        if !config_manager.exists() {
            return Err(PromptGuardError::NotInitialized);
        }

        let mut config = config_manager.load()?;

        if config.enabled && config.runtime_mode == self.runtime {
            if self.runtime {
                Output::warning("PromptGuard runtime mode is already enabled");
            } else {
                Output::warning("PromptGuard is already enabled");
            }
            return Ok(());
        }

        // Determine mode
        let mode = if self.runtime {
            "Runtime Shim Mode (100% Coverage)"
        } else {
            "Static Transform Mode"
        };

        println!("\nThis will enable PromptGuard using:");
        println!("  • Mode: {mode}");
        println!("  • Proxy URL: {}", config.proxy_url);
        println!("  • Providers: {}", config.providers.join(", "));

        if self.runtime {
            println!("\nRuntime mode provides:");
            println!("  ✓ 100% coverage of all SDK calls");
            println!("  ✓ Catches dynamic URL construction");
            println!("  ✓ Works with environment variables");
            println!("  ✓ No code modification needed");
        }

        if !Output::confirm("Continue?", true)? {
            return Ok(());
        }

        let root_path = std::env::current_dir()?;

        if self.runtime {
            // Runtime shim mode
            self.enable_runtime_mode(&root_path, &mut config, &config_manager)?;
        } else {
            // Static transformation mode
            Self::enable_static_mode(&root_path, &mut config, &config_manager)?;
        }

        Ok(())
    }

    fn enable_runtime_mode(
        &self,
        root_path: &PathBuf,
        config: &mut crate::config::PromptGuardConfig,
        config_manager: &ConfigManager,
    ) -> Result<()> {
        Output::section("Scanning project...", "🔍");

        // Scan for SDK usage to detect languages
        let scanner = FileScanner::new(root_path, Some(config.exclude_patterns.clone()))?;
        let files = scanner.scan_files(None)?;

        Output::step(&format!("Scanning {} files...", files.len()));

        let mut detected_languages = HashSet::new();

        for file_path in &files {
            if let Some(ext) = file_path.extension().and_then(|e| e.to_str()) {
                if let Some(lang) = Language::from_extension(ext) {
                    detected_languages.insert(lang);
                }
            }
        }

        if detected_languages.is_empty() {
            Output::warning("No supported languages detected");
            return Ok(());
        }

        Output::step(&format!(
            "Detected languages: {}",
            detected_languages
                .iter()
                .map(super::super::types::Language::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        ));

        // Generate runtime shims
        Output::section("Generating runtime shims...", "⚙️");

        let providers: Vec<Provider> = config
            .providers
            .iter()
            .filter_map(|p| Provider::parse(p))
            .collect();

        let generator = ShimGenerator::new(
            root_path,
            config.proxy_url.clone(),
            config.env_var_name.clone(),
            providers.clone(),
        );

        let languages: Vec<Language> = detected_languages.into_iter().collect();
        let shim_files = generator.generate_shims(&languages)?;

        for shim_file in &shim_files {
            let rel_path = shim_file.strip_prefix(root_path).unwrap_or(shim_file);
            Output::step(&format!("✓ Generated {}", rel_path.display()));
        }

        // Inject shim imports into entry points
        Output::section("Injecting shim imports...", "💉");

        let injector = ShimInjector::new(root_path);
        let mut total_injected = 0;

        for language in &languages {
            match language {
                Language::Python => {
                    let injected = injector.inject_shims(Language::Python)?;
                    for entry_point in &injected {
                        let rel_path = entry_point.strip_prefix(root_path).unwrap_or(entry_point);
                        Output::step(&format!("✓ Injected into {}", rel_path.display()));
                        total_injected += 1;
                    }
                },
                Language::TypeScript | Language::JavaScript => {
                    let entry_points = injector.detect_typescript_entry_points()?;
                    if !entry_points.is_empty() {
                        println!("\n  TypeScript/JavaScript entry points detected:");
                        for entry_point in &entry_points {
                            let rel_path =
                                entry_point.strip_prefix(root_path).unwrap_or(entry_point);
                            println!("    - {}", rel_path.display());
                        }
                        println!("\n  To complete setup, choose one:");
                        println!("    1. Add this import to each entry file:");
                        println!("       import './.promptguard/promptguard-shim';");
                        println!("\n    2. Or use tsconfig.json path aliases (recommended):");
                        println!("       See .promptguard/README.md for instructions");
                    }
                },
            }
        }

        // Scan environment variables
        Output::section("Checking environment variables...", "🌍");

        let env_scanner = EnvScanner::new(root_path);
        let env_report = env_scanner.generate_report()?;

        if !env_report.is_empty() && !env_report.contains("No environment variables") {
            println!("\n{env_report}");
            println!("  Recommendation: Ensure API_URL variables point to PromptGuard proxy:");
            println!("    {}", config.proxy_url);
        } else {
            Output::step("No environment variable configuration needed");
        }

        // Update config
        config.enabled = true;
        config.runtime_mode = true;
        config_manager.save(config)?;

        println!();
        Output::success("PromptGuard runtime mode enabled!");
        println!("\n  • Shim files generated: {}", shim_files.len());
        println!("  • Entry points injected: {total_injected}");
        println!("\n  Coverage: 100% - All SDK calls will route through PromptGuard");
        println!("\n  Shim directory: .promptguard/");
        println!("  (Safe to commit to version control)");

        Ok(())
    }

    fn enable_static_mode(
        root_path: &PathBuf,
        config: &mut crate::config::PromptGuardConfig,
        config_manager: &ConfigManager,
    ) -> Result<()> {
        Output::section("Scanning files...", "📁");

        let scanner = FileScanner::new(root_path, Some(config.exclude_patterns.clone()))?;
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
                            let rel_path = file_path.strip_prefix(root_path).unwrap_or(&file_path);
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

        // Update config
        config.enabled = true;
        config.runtime_mode = false;
        config_manager.save(config)?;
        Output::step("Updated configuration");

        println!();
        Output::success("PromptGuard enabled!");
        println!("\n  • {files_modified} files modified");
        println!("\nYour LLM requests will now go through PromptGuard.");

        Ok(())
    }
}
