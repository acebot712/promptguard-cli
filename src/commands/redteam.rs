//! Red Team Command - Adversarial security testing
//!
//! Run automated security tests against your AI application.
//! Uses `PromptGuard`'s Red Team API to evaluate security posture.

use crate::api::PromptGuardClient;
use crate::config::ConfigManager;
use crate::error::{PromptGuardError, Result};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// The autonomous agent runs an LLM-powered mutation loop server-side and
/// routinely takes minutes; the default 30s request timeout all but
/// guaranteed a client-side timeout (and, before retries were restricted to
/// connect errors, a timeout-retry storm of duplicate agent runs).
const AUTONOMOUS_TIMEOUT: Duration = Duration::from_mins(5);

#[derive(Debug, Deserialize, Serialize)]
struct RedTeamTestResult {
    test_name: String,
    prompt: String,
    decision: String,
    reason: String,
    threat_type: Option<String>,
    confidence: f64,
    blocked: bool,
    #[serde(default)]
    details: serde_json::Value,
}

#[derive(Debug, Deserialize, Serialize)]
struct RedTeamSummary {
    total_tests: usize,
    blocked: usize,
    allowed: usize,
    block_rate: f64,
    results: Vec<RedTeamTestResult>,
}

#[derive(Debug, Serialize)]
struct TestRequest {
    target_preset: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    custom_prompt: Option<String>,
}

#[derive(Debug, Serialize)]
struct AutonomousRequest {
    budget: u32,
    target_preset: String,
}

#[derive(Debug, Deserialize)]
struct AutonomousReport {
    grade: String,
    bypass_rate: f64,
    total_attempts: usize,
    bypasses_found: usize,
    #[serde(default)]
    bypasses: Vec<serde_json::Value>,
    #[serde(default)]
    recommendations: Vec<String>,
}

pub struct RedTeamCommand {
    pub target_url: Option<String>,
    pub api_key: Option<String>,
    pub output_format: String,
    pub verbose: bool,
    pub test_name: Option<String>,
    pub custom_prompt: Option<String>,
    pub preset: String,
    pub autonomous: bool,
    pub budget: u32,
}

impl Default for RedTeamCommand {
    fn default() -> Self {
        Self {
            target_url: None,
            api_key: None,
            output_format: "human".to_string(),
            verbose: false,
            test_name: None,
            custom_prompt: None,
            preset: "default".to_string(),
            autonomous: false,
            budget: 100,
        }
    }
}

impl RedTeamCommand {
    pub fn execute(self) -> Result<()> {
        println!("🔴 PromptGuard Red Team - Adversarial Security Testing\n");

        // --target-url is used as the API base URL, and every request
        // attaches the PromptGuard API key. Never send that key to an
        // arbitrary host: require HTTPS (or loopback) and refuse hosts other
        // than the configured/default PromptGuard API host. The red team
        // endpoints are authenticated PromptGuard endpoints, so a keyless
        // request to a foreign host could not work anyway.
        if let Some(ref target) = self.target_url {
            Self::validate_target_url(target)?;
        }

        // Resolve credentials through the shared precedence (env > project >
        // global) and the key-exfiltration guard, unless the caller passed an
        // explicit --api-key ('-' reads the key from stdin, like init/login).
        // --target-url always overrides the resolved base URL (and is
        // separately restricted by validate_target_url above).
        let (api_key, base_url) = if let Some(key) = &self.api_key {
            (super::resolve_api_key_flag(key)?, self.target_url.clone())
        } else {
            let (key, resolved_base) = crate::auth::resolve_session()?;
            (key, self.target_url.clone().or(Some(resolved_base)))
        };

        let client = PromptGuardClient::new(api_key, base_url)
            .map_err(|e| PromptGuardError::Config(format!("Failed to create client: {e}")))?;

        if self.autonomous {
            self.run_autonomous(&client)?;
        } else if let Some(prompt) = &self.custom_prompt {
            self.run_custom_test(&client, prompt)?;
        } else if let Some(test_name) = &self.test_name {
            self.run_single_test(&client, test_name)?;
        } else {
            self.run_all_tests(&client)?;
        }

        Ok(())
    }

    /// Refuse `--target-url` values that would leak the `PromptGuard` API
    /// key: non-HTTPS transports (except loopback) and hosts other than the
    /// configured or default `PromptGuard` API host.
    fn validate_target_url(target: &str) -> Result<()> {
        let parsed = url::Url::parse(target).map_err(|e| {
            PromptGuardError::Config(format!("Invalid --target-url '{target}': {e}"))
        })?;

        let host = parsed
            .host_str()
            .ok_or_else(|| {
                PromptGuardError::Config(format!("Invalid --target-url '{target}': missing host"))
            })?
            .to_string();

        let is_loopback = crate::config::is_loopback_host(&parsed);

        if parsed.scheme() != "https" && !is_loopback {
            return Err(PromptGuardError::Config(format!(
                "--target-url must use HTTPS (got '{target}'). The PromptGuard API key \
                 is sent with every request and must not travel over plaintext HTTP."
            )));
        }

        if is_loopback {
            return Ok(());
        }

        // Allowed remote hosts: the default API host and, if configured, the
        // host of this project's proxy_url.
        let mut allowed: Vec<String> = vec!["api.promptguard.co".to_string()];
        if let Ok(cfg) = ConfigManager::new(None).and_then(|cm| cm.load()) {
            if let Ok(proxy) = url::Url::parse(&cfg.proxy_url) {
                if let Some(h) = proxy.host_str() {
                    allowed.push(h.to_string());
                }
            }
        }

        if !allowed.iter().any(|h| h == &host) {
            return Err(PromptGuardError::Config(format!(
                "Refusing to send the PromptGuard API key to '{host}'. \
                 --target-url must point at the PromptGuard API ({}); the red team \
                 endpoints are authenticated PromptGuard endpoints and cannot be \
                 used against arbitrary hosts.",
                allowed.join(" or ")
            )));
        }

        Ok(())
    }

    fn run_all_tests(&self, client: &PromptGuardClient) -> Result<()> {
        println!(
            "Running all red team tests against preset '{}'...\n",
            self.preset
        );

        // Call the API
        let summary: RedTeamSummary = client
            .post(
                "/internal/redteam/test-all",
                &TestRequest {
                    target_preset: self.preset.clone(),
                    custom_prompt: None,
                },
            )
            .map_err(|e| PromptGuardError::Api(format!("Failed to run tests: {e}")))?;

        // Print results
        for result in &summary.results {
            let status = if result.blocked {
                "✅ BLOCKED"
            } else {
                "❌ PASSED THROUGH"
            };
            println!(
                "  {} - {} (confidence: {:.0}%)",
                result.test_name,
                status,
                result.confidence * 100.0
            );

            if self.verbose {
                println!(
                    "    Prompt: {}...",
                    crate::output::Output::truncate_chars(&result.prompt, 60)
                );
                println!("    Reason: {}", result.reason);
                if let Some(threat) = &result.threat_type {
                    println!("    Threat: {threat}");
                }
            }
        }

        self.print_summary(&summary);

        if self.output_format == "json" {
            println!(
                "\n{}",
                serde_json::to_string_pretty(&summary).unwrap_or_default()
            );
        }

        Ok(())
    }

    fn run_single_test(&self, client: &PromptGuardClient, test_name: &str) -> Result<()> {
        println!(
            "Running test '{}' against preset '{}'...\n",
            test_name, self.preset
        );

        let result: RedTeamTestResult = client
            .post(
                &format!("/internal/redteam/test/{test_name}"),
                &TestRequest {
                    target_preset: self.preset.clone(),
                    custom_prompt: None,
                },
            )
            .map_err(|e| PromptGuardError::Api(format!("Failed to run test: {e}")))?;

        let status = if result.blocked {
            "✅ BLOCKED"
        } else {
            "❌ PASSED THROUGH"
        };
        println!("Result: {status}");
        println!("Decision: {}", result.decision);
        println!("Reason: {}", result.reason);
        println!("Confidence: {:.0}%", result.confidence * 100.0);

        if let Some(threat) = &result.threat_type {
            println!("Threat Type: {threat}");
        }

        if self.output_format == "json" {
            println!(
                "\n{}",
                serde_json::to_string_pretty(&result).unwrap_or_default()
            );
        }

        Ok(())
    }

    fn run_custom_test(&self, client: &PromptGuardClient, prompt: &str) -> Result<()> {
        println!(
            "Running custom adversarial test against preset '{}'...\n",
            self.preset
        );
        println!(
            "Prompt: {}...\n",
            crate::output::Output::truncate_chars(prompt, 100)
        );

        let result: RedTeamTestResult = client
            .post(
                "/internal/redteam/test-custom",
                &TestRequest {
                    target_preset: self.preset.clone(),
                    custom_prompt: Some(prompt.to_string()),
                },
            )
            .map_err(|e| PromptGuardError::Api(format!("Failed to run custom test: {e}")))?;

        let status = if result.blocked {
            "✅ BLOCKED"
        } else {
            "❌ PASSED THROUGH"
        };
        println!("Result: {status}");
        println!("Decision: {}", result.decision);
        println!("Reason: {}", result.reason);
        println!("Confidence: {:.0}%", result.confidence * 100.0);

        if self.output_format == "json" {
            println!(
                "\n{}",
                serde_json::to_string_pretty(&result).unwrap_or_default()
            );
        }

        Ok(())
    }

    fn run_autonomous(&self, client: &PromptGuardClient) -> Result<()> {
        println!(
            "Running autonomous red team agent (budget: {}, preset: '{}')...\n",
            self.budget, self.preset
        );
        println!("This may take a while - the agent uses LLM-powered mutation\n");

        let report: AutonomousReport = client
            .post_with_timeout(
                "/internal/redteam/autonomous",
                &AutonomousRequest {
                    budget: self.budget,
                    target_preset: self.preset.clone(),
                },
                AUTONOMOUS_TIMEOUT,
            )
            .map_err(|e| PromptGuardError::Api(format!("Autonomous agent failed: {e}")))?;

        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        println!("🤖 Autonomous Red Team Report");
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

        println!("  Grade:            {}", report.grade);
        println!("  Bypass Rate:      {:.1}%", report.bypass_rate * 100.0);
        println!("  Total Attempts:   {}", report.total_attempts);
        println!("  Bypasses Found:   {}\n", report.bypasses_found);

        if !report.bypasses.is_empty() && self.verbose {
            println!("⚠️  Discovered Bypasses:\n");
            for (i, bypass) in report.bypasses.iter().enumerate() {
                println!("  {}. {}", i + 1, bypass);
            }
            println!();
        }

        if !report.recommendations.is_empty() {
            println!("📋 Recommendations:\n");
            for rec in &report.recommendations {
                println!("  • {rec}");
            }
            println!();
        }

        if self.output_format == "json" {
            let json_out = serde_json::json!({
                "grade": report.grade,
                "bypass_rate": report.bypass_rate,
                "total_attempts": report.total_attempts,
                "bypasses_found": report.bypasses_found,
                "bypasses": report.bypasses,
                "recommendations": report.recommendations,
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&json_out).unwrap_or_default()
            );
        }

        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

        Ok(())
    }

    fn print_summary(&self, summary: &RedTeamSummary) {
        println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        println!("📊 Security Assessment Report");
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

        let score = summary.block_rate * 100.0;

        println!("  Total Attacks:      {}", summary.total_tests);
        println!("  Attacks Blocked:    {} ✅", summary.blocked);
        println!("  Attacks Passed:     {} ❌", summary.allowed);
        println!("  Security Score:     {score:.1}/100\n");

        if summary.allowed > 0 {
            println!("⚠️  Vulnerabilities Found:\n");
            for result in &summary.results {
                if !result.blocked {
                    println!("  • {} - {}", result.test_name, result.reason);
                }
            }
            println!();
            println!("📋 Recommendations:\n");
            println!("  1. Enable PromptGuard ML detection for advanced threats");
            println!("  2. Review and strengthen your policy presets");
            println!("  3. Add custom rules for specific attack patterns");
        } else {
            println!("✨ Your application passed all security tests!");
        }

        println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_url_allows_promptguard_and_loopback() {
        assert!(RedTeamCommand::validate_target_url("https://api.promptguard.co/api/v1").is_ok());
        assert!(RedTeamCommand::validate_target_url("http://localhost:8080/api/v1").is_ok());
        assert!(RedTeamCommand::validate_target_url("http://127.0.0.1:3000").is_ok());
    }

    #[test]
    fn target_url_refuses_plain_http_and_foreign_hosts() {
        assert!(RedTeamCommand::validate_target_url("http://api.promptguard.co/api/v1").is_err());
        assert!(RedTeamCommand::validate_target_url("https://evil.example.com/api/v1").is_err());
        assert!(RedTeamCommand::validate_target_url("not a url").is_err());
    }
}
