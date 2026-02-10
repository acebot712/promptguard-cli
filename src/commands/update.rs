use crate::error::{PromptGuardError, Result};
use crate::output::Output;
use reqwest::blocking::Client;
use serde::Deserialize;
use std::time::Duration;

const GITHUB_API_URL: &str =
    "https://api.github.com/repos/acebot712/promptguard-cli/releases/latest";

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    body: Option<String>,
}

pub struct UpdateCommand {
    /// Reserved for future auto-update functionality
    #[allow(dead_code)]
    pub check_only: bool,
}

impl Default for UpdateCommand {
    fn default() -> Self {
        Self { check_only: true }
    }
}

impl UpdateCommand {
    pub fn execute(&self) -> Result<()> {
        Output::header("Update PromptGuard CLI");

        let current_version = env!("CARGO_PKG_VERSION");
        println!("\nCurrent version: v{current_version}");

        Output::info("Checking for updates...");

        // Check GitHub releases for the latest version
        match self.check_latest_version() {
            Ok(release) => {
                let latest_version = release.tag_name.trim_start_matches('v');

                if self.is_newer_version(current_version, latest_version) {
                    println!();
                    Output::success(&format!("New version available: v{}", latest_version));
                    println!();

                    if let Some(ref body) = release.body {
                        println!("What's new:");
                        // Print first few lines of release notes
                        for line in body.lines().take(5) {
                            println!("  {}", line);
                        }
                        println!();
                    }

                    println!("To update, run one of the following:");
                    println!();
                    self.print_update_instructions();

                    println!("Release notes: {}", release.html_url);
                } else {
                    println!();
                    Output::success("You are running the latest version!");
                }
            },
            Err(e) => {
                Output::warning(&format!("Could not check for updates: {}", e));
                println!();
                println!("You can manually check for updates at:");
                println!("  https://github.com/acebot712/promptguard-cli/releases");
                println!();
                self.print_update_instructions();
            },
        }

        println!("\nDocumentation:");
        println!("  https://docs.promptguard.co/cli");

        Ok(())
    }

    fn check_latest_version(&self) -> Result<GitHubRelease> {
        let client = Client::builder()
            .timeout(Duration::from_secs(10))
            .user_agent(format!("promptguard-cli/{}", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| PromptGuardError::Api(format!("Failed to create HTTP client: {}", e)))?;

        let response = client
            .get(GITHUB_API_URL)
            .header("Accept", "application/vnd.github.v3+json")
            .send()
            .map_err(|e| PromptGuardError::Api(format!("Failed to check for updates: {}", e)))?;

        if !response.status().is_success() {
            return Err(PromptGuardError::Api(format!(
                "GitHub API returned status {}",
                response.status()
            )));
        }

        response
            .json()
            .map_err(|e| PromptGuardError::Api(format!("Failed to parse GitHub response: {}", e)))
    }

    fn is_newer_version(&self, current: &str, latest: &str) -> bool {
        let parse_version =
            |v: &str| -> Vec<u32> { v.split('.').filter_map(|part| part.parse().ok()).collect() };

        let current_parts = parse_version(current);
        let latest_parts = parse_version(latest);

        for i in 0..3 {
            let current_num = current_parts.get(i).copied().unwrap_or(0);
            let latest_num = latest_parts.get(i).copied().unwrap_or(0);

            if latest_num > current_num {
                return true;
            } else if latest_num < current_num {
                return false;
            }
        }

        false
    }

    fn print_update_instructions(&self) {
        println!("  • Using curl (recommended):");
        println!("      curl -fsSL https://raw.githubusercontent.com/acebot712/promptguard-cli/main/install.sh | sh");
        println!();
        println!("  • Using Homebrew:");
        println!("      brew upgrade promptguard");
        println!();
        println!("  • Using cargo:");
        println!("      cargo install --force promptguard-cli");
        println!();
    }
}
