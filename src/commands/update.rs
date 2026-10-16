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

/// Strip release-tag prefixes. CLI releases are tagged `cli-vX.Y.Z` (the
/// repository hosts multiple components), with plain `vX.Y.Z` accepted for
/// robustness. Only stripping `v` left "cli-v1.6.0" parsing as `[6, 0]`
/// ("cli-" and "1" being non-numeric/mangled), so every check reported a
/// bogus new version.
fn strip_tag_prefix(tag: &str) -> &str {
    tag.strip_prefix("cli-v")
        .or_else(|| tag.strip_prefix('v'))
        .unwrap_or(tag)
}

/// Parse `X.Y.Z` into numeric parts.
///
/// Returns `None` when any dot-separated segment is not a plain number.
/// Non-numeric segments used to be silently FILTERED, shifting the
/// remaining segments into the wrong positions.
fn parse_version(v: &str) -> Option<Vec<u32>> {
    v.split('.').map(|part| part.parse::<u32>().ok()).collect()
}

/// Whether `latest` (a release tag or bare version) is newer than the
/// currently running `current` version. Unparseable versions never report
/// an update.
fn is_newer_version(current: &str, latest: &str) -> bool {
    let (Some(current_parts), Some(latest_parts)) = (
        parse_version(strip_tag_prefix(current)),
        parse_version(strip_tag_prefix(latest)),
    ) else {
        return false;
    };

    for i in 0..3 {
        let current_num = current_parts.get(i).copied().unwrap_or(0);
        let latest_num = latest_parts.get(i).copied().unwrap_or(0);

        match latest_num.cmp(&current_num) {
            std::cmp::Ordering::Greater => return true,
            std::cmp::Ordering::Less => return false,
            std::cmp::Ordering::Equal => {},
        }
    }

    false
}

pub struct UpdateCommand;

impl Default for UpdateCommand {
    fn default() -> Self {
        Self
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
                let latest_version = strip_tag_prefix(&release.tag_name);

                if is_newer_version(current_version, latest_version) {
                    println!();
                    Output::success(&format!("New version available: v{latest_version}"));
                    println!();

                    if let Some(ref body) = release.body {
                        println!("What's new:");
                        // Print first few lines of release notes
                        for line in body.lines().take(5) {
                            println!("  {line}");
                        }
                        println!();
                    }

                    println!("To update, run one of the following:");
                    println!();
                    self.print_update_instructions(Some(&release.tag_name));

                    println!("Release notes: {}", release.html_url);
                } else {
                    println!();
                    Output::success("You are running the latest version!");
                }
            },
            Err(e) => {
                Output::warning(&format!("Could not check for updates: {e}"));
                println!();
                println!("You can manually check for updates at:");
                println!("  https://github.com/acebot712/promptguard-cli/releases");
                println!();
                self.print_update_instructions(None);
            },
        }

        println!("\nDocumentation:");
        println!("  https://docs.promptguard.co");

        Ok(())
    }

    fn check_latest_version(&self) -> Result<GitHubRelease> {
        let client = Client::builder()
            .timeout(Duration::from_secs(10))
            .user_agent(format!("promptguard-cli/{}", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| PromptGuardError::Api(format!("Failed to create HTTP client: {e}")))?;

        let response = client
            .get(GITHUB_API_URL)
            .header("Accept", "application/vnd.github.v3+json")
            .send()
            .map_err(|e| PromptGuardError::Api(format!("Failed to check for updates: {e}")))?;

        if !response.status().is_success() {
            return Err(PromptGuardError::Api(format!(
                "GitHub API returned status {}",
                response.status()
            )));
        }

        response
            .json()
            .map_err(|e| PromptGuardError::Api(format!("Failed to parse GitHub response: {e}")))
    }

    fn print_update_instructions(&self, latest_tag: Option<&str>) {
        print!("{}", update_instructions(latest_tag));
    }
}

/// How to update, for each install method the README offers. There is no
/// Homebrew tap and no crates.io crate, so neither appears here; the Cargo
/// route builds a release tag from the repository, named when it is known.
fn update_instructions(latest_tag: Option<&str>) -> String {
    let tag = latest_tag.unwrap_or("cli-v<version>");
    format!(
        "  • Using curl (recommended):\n      \
         curl -fsSL https://raw.githubusercontent.com/acebot712/promptguard-cli/main/install.sh | sh\n\n  \
         • Using cargo (build from source):\n      \
         cargo install --locked --force --git https://github.com/acebot712/promptguard-cli --tag {tag}\n\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The real release tags are `cli-vX.Y.Z`; only stripping `v` made
    /// "cli-v1.6.0" parse as [6, 0], so v1.6.0 reported 6.0.0 available.
    #[test]
    fn cli_v_tags_compare_correctly() {
        assert!(!is_newer_version("1.6.0", "cli-v1.6.0"));
        assert!(!is_newer_version("2.0.0", "cli-v1.6.0"));
        assert!(is_newer_version("1.5.9", "cli-v1.6.0"));
        assert!(is_newer_version("1.6.0", "cli-v1.6.1"));
        assert!(is_newer_version("1.6.0", "cli-v2.0.0"));
    }

    #[test]
    fn plain_and_v_prefixed_tags_compare_correctly() {
        assert!(is_newer_version("1.0.0", "v1.1.0"));
        assert!(is_newer_version("1.0.0", "1.0.1"));
        assert!(is_newer_version("1.0.0", "2.0.0"));
        assert!(!is_newer_version("1.1.0", "1.0.0"));
        assert!(!is_newer_version("1.0.0", "1.0.0"));
        assert!(!is_newer_version("1.0.0", "v1.0.0"));
    }

    /// Non-numeric segments are a parse FAILURE, not something to filter:
    /// filtering shifted segments into the wrong comparison positions.
    #[test]
    fn unparseable_versions_never_report_an_update() {
        assert!(!is_newer_version("1.0.0", "cli-v2.0.0-rc1"));
        assert!(!is_newer_version("1.0.0", "not-a-version"));
        assert!(!is_newer_version("1.0.0", ""));
        assert!(!is_newer_version("garbage", "2.0.0"));
    }

    /// No Homebrew tap and no crates.io crate exist, so `update` must not
    /// send anyone to either (acebot712/promptguard#962).
    #[test]
    fn update_instructions_offer_only_published_methods() {
        let known = update_instructions(Some("cli-v2.2.0"));
        assert!(!known.to_lowercase().contains("brew"));
        assert!(known.contains(
            "cargo install --locked --force --git https://github.com/acebot712/promptguard-cli --tag cli-v2.2.0"
        ));
        assert!(!known.contains("--force promptguard"));
        assert!(update_instructions(None).contains("--tag cli-v<version>"));
    }

    #[test]
    fn tag_prefix_stripping() {
        assert_eq!(strip_tag_prefix("cli-v1.6.0"), "1.6.0");
        assert_eq!(strip_tag_prefix("v1.6.0"), "1.6.0");
        assert_eq!(strip_tag_prefix("1.6.0"), "1.6.0");
    }
}
