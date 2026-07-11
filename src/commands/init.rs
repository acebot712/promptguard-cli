use crate::api::PromptGuardClient;
use crate::config::{ConfigManager, PromptGuardConfig};
use crate::detector::ProviderInfo;
use crate::env::EnvManager;
use crate::error::Result;
use crate::output::Output;
use crate::scanner::FileScanner;
use crate::types::Provider;
use std::io::Write;
use std::path::Path;

pub struct InitCommand {
    pub provider: Vec<String>,
    pub api_key: Option<String>,
    pub base_url: String,
    pub env_file: String,
    pub auto: bool,
    pub dry_run: bool,
    pub force: bool,
    pub exclude: Vec<String>,
    pub framework: Option<String>,
}

impl InitCommand {
    pub fn execute(&self) -> Result<()> {
        if !self.dry_run {
            Output::header(&format!(
                "🛡️ PromptGuard CLI v{}",
                env!("CARGO_PKG_VERSION")
            ));
        }

        // --base-url is interpolated into transformed source files and
        // generated shims before the config is saved: validate up front.
        crate::config::validate_proxy_url(&self.base_url)?;

        // Check for git repository (Linus-approved safety)
        let root_path = std::env::current_dir()?;
        if !self.check_version_control(&root_path)? {
            return Ok(());
        }

        // Check if already initialized
        let config_manager = ConfigManager::new(None)?;
        if config_manager.exists() && !self.dry_run {
            Output::warning("PromptGuard is already initialized in this project.");
            if !self.auto && !Output::confirm("Reinitialize?", false)? {
                return Ok(());
            }
        }

        // Get API key
        let api_key = self.get_api_key()?;

        // Scan project
        Output::section("Scanning project...", "📁");

        // User-provided --exclude patterns ADD to the defaults rather than
        // replacing them: replacing meant a single --exclude dropped the
        // node_modules/dist/venv defaults and those trees were scanned and
        // transformed.
        let exclude_patterns: Option<Vec<String>> = if self.exclude.is_empty() {
            None
        } else {
            let mut patterns = crate::config::default_exclude_patterns();
            patterns.extend(self.exclude.iter().cloned());
            Some(patterns)
        };

        let scanner = FileScanner::new(&root_path, exclude_patterns.clone())?;

        if let Some(git_root) = scanner.find_git_root() {
            Output::step(&format!(
                "Found .git directory (root: {})",
                git_root.display()
            ));
        }

        let framework = self
            .framework
            .clone()
            .or_else(|| scanner.detect_framework());
        if let Some(ref fw) = framework {
            Output::step(&format!("Detected framework: {fw}"));
        }

        let files = scanner.scan_files(None)?;
        Output::step(&format!("Scanning {} files...", files.len()));

        // Detect SDK usage
        Output::section("Detected LLM SDKs:", "🔍");

        // --help documents the default as "all detected", so check the full
        // provider registry (previously only 4 of the 7 providers were
        // checked when no --provider was given).
        let providers_to_check: Vec<Provider> =
            if self.provider.is_empty() || self.provider.contains(&"all".to_string()) {
                crate::detector::registry::PROVIDERS
                    .iter()
                    .map(|info| info.provider)
                    .collect()
            } else {
                self.provider
                    .iter()
                    .filter_map(|p| Provider::parse(p))
                    .collect()
            };

        let detection_results = super::detect_providers_in_files(&files, &providers_to_check);

        if detection_results.is_empty() {
            Output::error("No LLM SDKs detected in this project.");
            println!("\nPromptGuard works with:");
            println!("  • OpenAI SDK (openai)");
            println!("  • Anthropic SDK (@anthropic-ai/sdk)");
            println!("  • Cohere SDK (cohere-ai)");
            println!("  • HuggingFace SDK (@huggingface/inference)");
            println!("  • Google Gemini SDK (google-genai)");
            println!("  • Groq SDK (groq)");
            println!("  • AWS Bedrock (boto3 / @aws-sdk/client-bedrock-runtime)");
            println!("\nMake sure you've installed one of these SDKs.");
            return Ok(());
        }

        for (provider, files) in &detection_results {
            let mut unique_files = files.clone();
            unique_files.sort();
            unique_files.dedup();

            println!(
                "  • {} SDK ({} files)",
                provider.display_name(),
                unique_files.len()
            );
            for file in unique_files.iter().take(5) {
                let rel_path = file.strip_prefix(&root_path).unwrap_or(file);
                Output::step(&rel_path.display().to_string());
            }
            if unique_files.len() > 5 {
                Output::step(&format!("... and {} more", unique_files.len() - 5));
            }
        }

        // Show configuration
        Output::section("Configuration:", "📝");
        println!("  • Proxy URL: {}", self.base_url);
        println!("  • Environment: {}", self.env_file);
        println!("  • Version control: Git (backups via git diff/revert)");

        // Confirm changes
        if !self.auto && !self.dry_run {
            println!();
            if !Output::confirm("Apply these changes?", true)? {
                return Ok(());
            }
        }

        if self.dry_run {
            println!();
            Output::info("DRY RUN - no changes will be made");
        }

        // Apply transformations
        Output::section(
            if self.dry_run {
                "Preview:"
            } else {
                "Applying changes..."
            },
            "🔧",
        );

        // Same backup strategy as `apply`: copy each file to <file>.bak
        // before transforming it, so `disable` has something to restore.
        let backup_manager = if self.dry_run {
            None
        } else {
            Some(crate::backup::BackupManager::new(None))
        };

        // Use the same env var name the config will be created with, so the
        // transformed source and the .env entry can never drift apart.
        let env_var_name = crate::config::default_env_var_name();

        let mode = if self.dry_run {
            super::TransformMode::DryRun
        } else {
            super::TransformMode::Apply(backup_manager.as_ref())
        };

        let outcome = super::run_transform_pipeline(
            &detection_results,
            &root_path,
            mode,
            &self.base_url,
            &env_var_name,
            |provider, file_path, result| {
                let rel_path = file_path.strip_prefix(&root_path).unwrap_or(file_path);

                if result.modified {
                    let base_url_param = ProviderInfo::get(provider)
                        .map_or("base_url", |info| info.ts_base_url_param);
                    let verb = if self.dry_run { "would add" } else { "added" };
                    Output::step(&format!(
                        "{} ({} {} for {})",
                        rel_path.display(),
                        verb,
                        base_url_param,
                        provider.display_name()
                    ));
                } else if result.needs_manual_routing > 0 {
                    // Be honest: these call sites pass dynamic arguments
                    // (`**cfg`, an identifier options object, ...) that could
                    // already carry a base_url — injecting one could raise a
                    // TypeError at runtime, so they were left untouched.
                    Output::excluded(&format!(
                        "{} (skipped: {} call site(s) pass dynamic arguments — route through PromptGuard manually)",
                        rel_path.display(),
                        result.needs_manual_routing
                    ));
                } else {
                    Output::excluded(&format!("{} (no changes needed)", rel_path.display()));
                }
            },
        );

        let files_modified = outcome.files_modified;
        let backups_created = outcome.backups_created;

        // Update .env file
        // Security: Validate env_file doesn't escape project directory
        // (covers Unix and Windows absolute/UNC/parent-dir forms).
        crate::config::validate_env_file_path(&self.env_file)?;
        let env_path = root_path.join(&self.env_file);
        if !self.dry_run {
            EnvManager::add_or_update_key(&env_path, &env_var_name, &api_key)?;
            Output::step(&format!("{} (added {env_var_name})", self.env_file));
        } else {
            Output::step(&format!("{} (would add {env_var_name})", self.env_file));
        }

        // Save configuration
        if !self.dry_run {
            let providers_list: Vec<String> = detection_results
                .keys()
                .map(|p| p.as_str().to_string())
                .collect();

            let mut config =
                PromptGuardConfig::new(api_key, self.base_url.clone(), providers_list)?;

            // Persist defaults + user patterns (mirrors the scanner above).
            config.exclude_patterns =
                exclude_patterns.unwrap_or_else(crate::config::default_exclude_patterns);

            config.env_file = self.env_file.clone();
            config.framework = framework;

            config.metadata.files_managed = files_modified
                .iter()
                .map(|f| {
                    f.strip_prefix(&root_path)
                        .unwrap_or(f)
                        .to_string_lossy()
                        .to_string()
                })
                .collect();
            config.metadata.backups = backups_created;
            config.metadata.last_applied = Some(chrono::Utc::now());

            config_manager.save(&config)?;
            Output::step(".promptguard.json (created)");

            // Both files contain the API key in plaintext — keep them out
            // of version control.
            match Self::ensure_gitignored(&root_path, &[".promptguard.json", &self.env_file]) {
                Ok(added) if !added.is_empty() => {
                    Output::step(&format!(".gitignore (added {})", added.join(", ")));
                },
                Ok(_) => {},
                Err(e) => {
                    Output::warning(&format!(
                        "Could not update .gitignore ({e}). Add .promptguard.json and {} \
                         to it manually — both contain your API key.",
                        self.env_file
                    ));
                },
            }
        } else {
            Output::step(".promptguard.json (would be created)");
        }

        // Summary
        println!();
        if !self.dry_run {
            Output::success("PromptGuard is now active!");
            println!("\nNext steps:");
            println!("  • Run your app normally - all LLM requests now go through PromptGuard");
            println!("  • View logs: promptguard logs");
            println!("  • Check dashboard: https://app.promptguard.co/dashboard");
            println!("\nManaging PromptGuard:");
            println!("  • Pause protection:  promptguard disable");
            println!("  • Remove entirely:   promptguard revert");
            println!("  • Undo file edits:   git diff (review) | git checkout -- . (undo)");
        } else {
            let n = files_modified.len();
            println!(
                "✓ {n} {} would be modified",
                super::pluralize(n, "file", "files")
            );
            println!("✓ 1 file would be created (.promptguard.json)");
            println!("\nTo apply: promptguard init");
        }

        println!("\nNeed help? https://docs.promptguard.co/cli");

        Ok(())
    }

    /// Ensure the given entries are listed in the project's .gitignore.
    ///
    /// Creates .gitignore when the project is a git repository and it does
    /// not exist yet. Returns the entries that were newly added. Outside a
    /// git repository this is a no-op (nothing to protect from committing).
    fn ensure_gitignored(root_path: &Path, entries: &[&str]) -> Result<Vec<String>> {
        let gitignore_path = root_path.join(".gitignore");

        if !gitignore_path.exists() && !root_path.join(".git").exists() {
            return Ok(Vec::new());
        }

        let existing = if gitignore_path.exists() {
            std::fs::read_to_string(&gitignore_path)?
        } else {
            String::new()
        };

        let existing_lines: Vec<&str> = existing
            .lines()
            .map(|l| l.trim().trim_start_matches('/'))
            .collect();

        let mut added = Vec::new();
        let mut new_content = existing.clone();

        for entry in entries {
            if existing_lines.contains(&entry.trim_start_matches('/')) {
                continue;
            }
            if !new_content.is_empty() && !new_content.ends_with('\n') {
                new_content.push('\n');
            }
            if added.is_empty() {
                new_content.push_str("\n# PromptGuard (contains API key)\n");
            }
            new_content.push_str(entry);
            new_content.push('\n');
            added.push((*entry).to_string());
        }

        if !added.is_empty() {
            std::fs::write(&gitignore_path, new_content)?;
        }

        Ok(added)
    }

    fn check_version_control(&self, root_path: &Path) -> Result<bool> {
        let git_dir = root_path.join(".git");

        if !git_dir.exists() {
            println!();
            Output::warning("⚠️  NOT A GIT REPOSITORY");
            println!();
            println!("PromptGuard will modify your source files.");
            println!("Without version control, you cannot easily revert these changes.");
            println!();
            println!("Recommended:");
            println!("  git init");
            println!("  git add .");
            println!("  git commit -m 'Initial commit before PromptGuard'");
            println!("  promptguard init");
            println!();

            if !self.force {
                println!("To proceed anyway: promptguard init --force");
                println!();
                return Ok(false);
            }

            // Note: .bak backups ARE still created (same strategy as
            // `apply`); what is missing without git is the ability to review
            // and revert arbitrary changes with git diff/checkout.
            println!("⚠️  Proceeding with --force (.bak file backups will be created,");
            println!("   but without git you cannot review or revert other changes)");
            println!();

            if !self.auto
                && !self.dry_run
                && !Output::confirm(
                    "Are you SURE you want to continue without version control?",
                    false,
                )?
            {
                return Ok(false);
            }
        }

        Ok(true)
    }

    fn get_api_key(&self) -> Result<String> {
        let api_key = if let Some(ref key) = self.api_key {
            // `--api-key -` reads the key from stdin
            super::resolve_api_key_flag(key)?
        } else if let Ok(key) = std::env::var("PROMPTGUARD_API_KEY") {
            key
        } else if !self.auto && !self.dry_run {
            // Interactive mode - offer signup flow
            println!();
            Output::section("API Key Required", "🔑");
            println!();
            println!("You need a PromptGuard API key to continue.");
            println!();
            println!("Options:");
            println!("  1. I have an API key");
            println!("  2. Sign up / Get API key");
            println!("  3. Cancel");
            println!();

            let mut choice = String::new();
            print!("Select option (1-3): ");
            std::io::stdout().flush()?;
            std::io::stdin().read_line(&mut choice)?;
            let choice = choice.trim();

            match choice {
                "1" => {
                    // User has API key - prompt for it
                    println!();
                    Output::input("🔑 Paste your PromptGuard API key")?
                },
                "2" => {
                    // Signup flow
                    println!();
                    Output::info("Opening signup page in your browser...");
                    let signup_url = "https://app.promptguard.co/signup";

                    // Try to open browser, but don't fail if it doesn't work
                    if let Err(e) = open::that(signup_url) {
                        Output::warning(&format!("Could not open browser automatically: {e}"));
                    }

                    println!();
                    println!("Please sign up at: {signup_url}");
                    println!("After signing up, you can get your API key from:");
                    println!("  https://app.promptguard.co/settings/api-keys");
                    println!();

                    if Output::confirm("Have you signed up and got your API key?", false)? {
                        println!();
                        Output::input("🔑 Paste your PromptGuard API key")?
                    } else {
                        return Err(crate::error::PromptGuardError::Custom(
                            "API key is required to continue".to_string(),
                        ));
                    }
                },
                _ => {
                    return Err(crate::error::PromptGuardError::Custom(
                        "Initialization cancelled".to_string(),
                    ));
                },
            }
        } else if self.dry_run {
            return Ok("pg_live_demo123456789012345678901234".to_string());
        } else {
            return Err(crate::error::PromptGuardError::Custom(
                "API key required in non-interactive mode. Use --api-key flag or set PROMPTGUARD_API_KEY".to_string(),
            ));
        };

        if api_key.is_empty() {
            return Err(crate::error::PromptGuardError::Custom(
                "API key is required".to_string(),
            ));
        }

        // Validate API key format
        if !crate::config::is_valid_api_key(&api_key) {
            return Err(crate::error::PromptGuardError::InvalidApiKey);
        }

        // Validate API key against the backend (skip in dry-run mode).
        // Uses the authenticated /projects endpoint: the unauthenticated
        // /health probe "succeeds" for any key and proves nothing.
        if !self.dry_run {
            Output::info("Validating API key...");

            let client = PromptGuardClient::new(api_key.clone(), Some(self.base_url.clone()))?;

            match client.validate_credentials() {
                Ok(()) => {
                    Output::success("API key validated successfully");
                },
                // 401/403: the key is definitively bad — don't offer to continue.
                Err(crate::error::PromptGuardError::Auth(msg)) => {
                    Output::error(&format!("API key rejected: {msg}"));
                    println!();
                    println!("Check your key at https://app.promptguard.co/settings/api-keys");
                    return Err(crate::error::PromptGuardError::Auth(msg));
                },
                // Network or server issue: the key may be fine.
                Err(e) => {
                    Output::warning(&format!("Could not validate API key: {e}"));
                    println!();
                    println!("This could mean:");
                    println!("  • The PromptGuard API is temporarily unavailable");
                    println!("  • Network connectivity issues");
                    println!();

                    if !self.auto && !Output::confirm("Continue anyway?", false)? {
                        return Err(crate::error::PromptGuardError::Custom(
                            "API key validation failed. Please check your API key.".to_string(),
                        ));
                    }
                },
            }
        }

        Ok(api_key)
    }
}
