use crate::backup::BackupManager;
use crate::config::ConfigManager;
use crate::env::EnvManager;
use crate::error::Result;
use crate::output::Output;
use std::fs;
use std::path::Path;

pub struct DoctorCommand;

impl DoctorCommand {
    pub fn execute() -> Result<()> {
        Output::header("Running diagnostics...");

        println!("\n🩺 Running diagnostics...\n");

        let mut warnings_count = 0;
        let mut errors_count = 0;

        let root_path = std::env::current_dir()?;

        // Check CLI version
        Output::step(&format!(
            "CLI version: {} (latest)",
            env!("CARGO_PKG_VERSION")
        ));

        // Check config file
        let config_manager = ConfigManager::new(None)?;
        if config_manager.exists() {
            match config_manager.load() {
                Ok(config) => {
                    Output::step("Configuration file: .promptguard.json (valid)");

                    if config.api_key.starts_with("pg_sk_test_")
                        || config.api_key.starts_with("pg_sk_prod_")
                    {
                        Output::step("API key: valid format");
                    } else {
                        Output::warning("API key: invalid format");
                        errors_count += 1;
                    }

                    // Security check: warn if config contains API key and is not gitignored
                    if Self::check_config_in_gitignore(&root_path) {
                        Output::step("Security: .promptguard.json is in .gitignore");
                    } else {
                        Output::warning(
                            "Security: .promptguard.json contains API key but is NOT in .gitignore",
                        );
                        println!(
                            "  ⚠️  Your API key may be exposed if committed to version control!"
                        );
                        println!(
                            "  Recommendation: Add '.promptguard.json' to your .gitignore file"
                        );
                        println!("  Or use environment variables only (PROMPTGUARD_API_KEY)");
                        warnings_count += 1;
                    }
                },
                Err(e) => {
                    Output::warning(&format!("Configuration file: invalid ({e})"));
                    errors_count += 1;
                },
            }
        } else {
            Output::warning("Configuration file: not found (run 'promptguard init')");
            warnings_count += 1;
        }

        // Check .env file
        let env_path = root_path.join(".env");
        if env_path.exists() {
            if EnvManager::has_key(&env_path, "PROMPTGUARD_API_KEY") {
                Output::step("Environment file: .env (found, contains PROMPTGUARD_API_KEY)");

                // Check if .env is gitignored
                if Self::check_env_in_gitignore(&root_path) {
                    Output::step("Security: .env is in .gitignore");
                } else {
                    Output::warning("Security: .env is NOT in .gitignore");
                    println!("  ⚠️  Your secrets may be exposed if committed!");
                    println!("  Recommendation: Add '.env' to your .gitignore file");
                    warnings_count += 1;
                }
            } else {
                Output::warning("Environment file: .env (found, but missing PROMPTGUARD_API_KEY)");
                warnings_count += 1;
            }
        } else {
            Output::warning("Environment file: .env (not found)");
            warnings_count += 1;
        }

        // Check for backups
        let backup_manager = BackupManager::new(None);
        let backups = backup_manager.list_backups(&root_path);
        if backups.is_empty() {
            Output::step("No backup files found");
        } else {
            Output::warning(&format!(
                "Backup files: {} *.bak files found",
                backups.len()
            ));
            println!("\n  Recommendations:");
            println!("    1. Review and commit or remove *.bak backup files");
            println!("    2. Or add '*.bak' to .gitignore");
            warnings_count += 1;
        }

        // Report overall health based on actual findings
        println!();
        if errors_count > 0 {
            Output::error(&format!(
                "Overall health: ✗ {errors_count} error(s), {warnings_count} warning(s)"
            ));
        } else if warnings_count > 0 {
            Output::warning(&format!(
                "Overall health: ⚠ {warnings_count} warning(s) (see above)"
            ));
        } else {
            Output::success("Overall health: ✓ All checks passed");
        }

        Ok(())
    }

    /// Check if .promptguard.json is listed in .gitignore
    fn check_config_in_gitignore(root_path: &Path) -> bool {
        Self::is_pattern_in_gitignore(root_path, ".promptguard.json")
    }

    /// Check if .env is listed in .gitignore
    fn check_env_in_gitignore(root_path: &Path) -> bool {
        Self::is_pattern_in_gitignore(root_path, ".env")
    }

    /// Check if a pattern exists in .gitignore
    fn is_pattern_in_gitignore(root_path: &Path, pattern: &str) -> bool {
        let gitignore_path = root_path.join(".gitignore");
        if let Ok(content) = fs::read_to_string(gitignore_path) {
            content.lines().any(|line| {
                let trimmed = line.trim();
                // Match exact pattern or with leading slash
                trimmed == pattern || trimmed == format!("/{pattern}")
            })
        } else {
            false
        }
    }
}
