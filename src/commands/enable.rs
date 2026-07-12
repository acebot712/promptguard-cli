use crate::analyzer::EnvScanner;
use crate::backup::BackupManager;
use crate::config::ConfigManager;
use crate::error::{PromptGuardError, Result};
use crate::output::Output;
use crate::scanner::FileScanner;
use crate::shim::{ShimGenerator, ShimInjector};
use crate::types::{Language, Provider};
use std::collections::HashSet;
use std::path::PathBuf;

pub struct EnableCommand {
    pub runtime: bool,
    /// Skip the confirmation prompt. Required for non-interactive callers —
    /// e.g. the VS Code extension spawns the CLI with piped stdin that never
    /// delivers input.
    pub yes: bool,
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
            "Runtime Shim Mode"
        } else {
            "Static Transform Mode"
        };

        println!("\nThis will enable PromptGuard using:");
        println!("  • Mode: {mode}");
        println!("  • Proxy URL: {}", config.proxy_url);
        println!("  • Providers: {}", config.providers.join(", "));

        if self.runtime {
            println!("\nRuntime mode provides:");
            println!("  ✓ Intercepts the patched SDK client classes (sync and async)");
            println!("  ✓ Catches dynamic URL construction");
            println!("  ✓ Works with environment variables");
            println!("  ✓ No code modification needed");
        }

        if !self.yes && !Output::confirm("Continue?", true)? {
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

        // Warn about providers that have no runtime shim yet (they still work
        // via proxy mode, so this is informational, not a hard failure).
        for provider in providers.iter().filter(|p| !p.has_runtime_shim()) {
            Output::warning(&format!(
                "Runtime shim not yet available for {} — use proxy mode instead (run 'promptguard enable' without --runtime)",
                provider.display_name()
            ));
        }

        let generator = ShimGenerator::new(root_path, config.proxy_url.clone(), providers.clone());

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
        config.metadata.last_applied = Some(chrono::Utc::now());
        config_manager.save(config)?;

        println!();
        Output::success("PromptGuard runtime mode enabled!");
        println!("\n  • Shim files generated: {}", shim_files.len());
        println!("  • Entry points injected: {total_injected}");

        // Be honest about coverage: list exactly which client classes the
        // generated shim patches instead of claiming "100% - All SDK calls".
        let mut any_patched = false;
        println!("\n  Coverage — calls constructed via these patched classes route");
        println!("  through PromptGuard:");
        for provider in &providers {
            let classes = crate::shim::templates::get_python_patched_classes(*provider);
            if !classes.is_empty() {
                println!("    • {}: {}", provider.display_name(), classes.join(", "));
                any_patched = true;
            }
        }
        if !any_patched {
            println!("    (none — no configured provider has a runtime shim yet)");
        }
        println!("  Other classes/SDKs are not intercepted by the runtime shim.");

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

        let detection_results = super::detect_providers_in_files(&files, &providers_to_check);

        if detection_results.is_empty() {
            Output::warning("No SDK instances found to transform.");
            return Ok(());
        }

        Output::section("Applying transformations...", "🔧");

        // Mirror `apply`/`init`: back up each file BEFORE transforming it and
        // record the backup in metadata.backups, so `disable` can restore
        // exactly what PromptGuard created (create_backup never overwrites an
        // existing .bak, so the original state is preserved).
        let backup_manager = if config.backup_enabled {
            Some(BackupManager::new(Some(config.backup_extension.clone())))
        } else {
            None
        };

        let outcome = super::run_transform_pipeline(
            &detection_results,
            root_path,
            super::TransformMode::Apply(backup_manager.as_ref()),
            &config.proxy_url,
            &config.env_var_name,
            |_provider, file_path, result| {
                let rel_path = file_path.strip_prefix(root_path).unwrap_or(file_path);
                if result.modified {
                    Output::step(&format!("✓ {}", rel_path.display()));
                } else if result.needs_manual_routing > 0 {
                    Output::excluded(&format!(
                        "{} (skipped: {} call site(s) pass dynamic arguments — route through PromptGuard manually)",
                        rel_path.display(),
                        result.needs_manual_routing
                    ));
                }
            },
        );

        // Update config
        config.enabled = true;
        config.runtime_mode = false;
        for backup in outcome.backups_created {
            if !config.metadata.backups.contains(&backup) {
                config.metadata.backups.push(backup);
            }
        }
        config.metadata.last_applied = Some(chrono::Utc::now());
        config_manager.save(config)?;
        Output::step("Updated configuration");

        println!();
        Output::success("PromptGuard enabled!");
        println!("\n  • {} files modified", outcome.files_modified.len());
        println!("\nYour LLM requests will now go through PromptGuard.");

        Ok(())
    }
}
