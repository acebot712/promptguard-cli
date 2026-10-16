use crate::api::PromptGuardClient;
use crate::auth::resolve_session;
use crate::detector::detect_all_providers;
use crate::error::{PromptGuardError, Result};
use crate::local_rules;
use crate::output::Output;
use crate::scanner::FileScanner;
use crate::types::{DetectionInstance, Provider};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Response from the /security/scan endpoint.
///
/// The backend returns camelCase fields (`threatType`, `eventId`,
/// `processingTimeMs`).
#[derive(Debug, Deserialize, Serialize)]
pub struct SecurityScanResponse {
    pub blocked: bool,
    pub decision: String,
    pub confidence: f64,
    pub reason: String,
    #[serde(default, rename = "threatType")]
    pub threat_type: Option<String>,
    #[serde(default, rename = "eventId")]
    pub event_id: Option<String>,
    #[serde(default, rename = "processingTimeMs")]
    pub processing_time_ms: Option<f64>,
}

/// Exit code for a clean scan (content allowed / no SDK findings).
pub const EXIT_CLEAN: i32 = 0;
/// Exit code when content is blocked or SDK usage is found.
pub const EXIT_FINDINGS: i32 = 2;

pub struct ScanCommand {
    pub provider: Option<String>,
    pub json: bool,
    /// Text to scan for security threats via the API
    pub text: Option<String>,
    /// File path to scan for security threats via the API
    pub file: Option<String>,
    /// Scan `text`/`file` with the bundled local rule pack, not the API
    pub offline: bool,
}

impl ScanCommand {
    /// Returns the process exit code: 0 = allow/clean, 2 = block/findings.
    /// Errors map to exit code 1 in main.
    pub fn execute(&self) -> Result<i32> {
        if self.offline {
            return self.execute_offline_scan();
        }

        // If --text or --file is provided, do an API security scan instead of local SDK detection
        if self.text.is_some() || self.file.is_some() {
            return self.execute_api_scan();
        }

        // Otherwise, do local SDK detection
        self.execute_local_scan()
    }

    /// The content named by --text or --file.
    fn content(&self) -> Result<String> {
        if let Some(ref text) = self.text {
            Ok(text.clone())
        } else if let Some(ref file_path) = self.file {
            super::read_file_friendly(file_path)
        } else {
            Err(PromptGuardError::Custom(
                "Either --text or --file must be provided".to_string(),
            ))
        }
    }

    /// Scan text or file content against the bundled local rule pack. Opens
    /// no connection and needs no key; says on every run that it is a local
    /// subset of what the API detects.
    fn execute_offline_scan(&self) -> Result<i32> {
        let content = self.content()?;
        let pack = local_rules::bundled()?;
        let findings = pack.scan(&content);
        let blocked = findings.iter().any(|f| f.blocking);
        let notice = local_rules::notice(pack.rules().len());

        if self.json {
            let findings_json: Vec<serde_json::Value> = findings
                .iter()
                .map(|f| {
                    serde_json::json!({
                        "rule": f.rule_id,
                        "detector": f.detector,
                        "category": f.category,
                        "severity": f.severity.as_str(),
                        "blocking": f.blocking,
                        "line": f.line,
                        "excerpt": f.excerpt,
                    })
                })
                .collect();
            let output = serde_json::json!({
                "mode": "offline",
                "notice": notice,
                "rules": pack.rules().len(),
                "blocked": blocked,
                "decision": if blocked { "block" } else { "allow" },
                "findings": findings_json,
            });
            println!("{}", serde_json::to_string_pretty(&output)?);
        } else {
            Output::header(&format!(
                "🛡️ PromptGuard CLI v{}",
                env!("CARGO_PKG_VERSION")
            ));
            Output::section("Offline Threat Scan (local subset)", "🔍");
            // Printed directly, not through Output, so --quiet cannot hide it.
            println!("{notice}");
            Output::info(&format!("Scanning {} characters...", content.len()));
            println!();
            if blocked {
                println!("Decision: 🚫 BLOCKED by the local rules");
            } else {
                println!("Decision: ✅ ALLOWED by the local rules");
            }
            for f in &findings {
                let effect = if f.blocking { "blocking" } else { "warning" };
                println!(
                    "  • [{}] {} (line {}, {effect}): {:?}",
                    f.severity.as_str(),
                    f.rule_id,
                    f.line,
                    f.excerpt
                );
            }
            if !blocked && !findings.is_empty() {
                println!("Medium-severity matches are warnings and do not fail the scan.");
            }
            println!("For full detection, run the same scan without --offline.");
        }

        Ok(if blocked { EXIT_FINDINGS } else { EXIT_CLEAN })
    }

    /// Scan text or file content for security threats via the backend API
    fn execute_api_scan(&self) -> Result<i32> {
        let content = self.content()?;

        // Resolve credentials from env var, project config, or global login —
        // this command works without a project being initialized.
        let (api_key, base_url) = resolve_session()?;

        let client = PromptGuardClient::new(api_key, Some(base_url))?;

        if !self.json {
            Output::header(&format!(
                "🛡️ PromptGuard CLI v{}",
                env!("CARGO_PKG_VERSION")
            ));
            Output::section("Security Threat Scan", "🔍");
            Output::info(&format!("Scanning {} characters...", content.len()));
        }

        let response: SecurityScanResponse = client.post(
            "/security/scan",
            &serde_json::json!({
                "content": content,
                "type": "prompt",
            }),
        )?;

        if self.json {
            println!(
                "{}",
                serde_json::to_string_pretty(&response).unwrap_or_default()
            );
        } else {
            println!();
            let decision_display = match response.decision.as_str() {
                "block" => "🚫 BLOCKED",
                "allow" => "✅ ALLOWED",
                _ => &response.decision,
            };
            println!("Decision: {decision_display}");
            println!("Confidence: {:.1}%", response.confidence * 100.0);

            if let Some(ref threat_type) = response.threat_type {
                println!("Threat Type: {threat_type}");
            }

            if !response.reason.is_empty() {
                println!("Reason: {}", response.reason);
            }

            if response.decision == "block" {
                println!();
                Output::warning("This content was flagged as potentially malicious.");
            } else {
                println!();
                Output::success("No security threats detected.");
            }
        }

        Ok(if response.blocked || response.decision == "block" {
            EXIT_FINDINGS
        } else {
            EXIT_CLEAN
        })
    }

    /// Local SDK detection scan (original behavior)
    fn execute_local_scan(&self) -> Result<i32> {
        if !self.json {
            Output::header(&format!(
                "🛡️ PromptGuard CLI v{}",
                env!("CARGO_PKG_VERSION")
            ));
            Output::section("LLM SDK Detection Report", "📊");
        }

        let root_path = std::env::current_dir()?;
        let scanner = FileScanner::new(&root_path, None)?;
        let files = scanner.scan_files(None)?;

        // Store full detection instances (with line/column info) for each provider
        let mut detection_results: HashMap<Provider, Vec<DetectionInstance>> = HashMap::new();

        for file_path in &files {
            if let Ok(results) = detect_all_providers(file_path) {
                for (provider, result) in results {
                    if let Some(ref filter) = self.provider {
                        if provider.as_str() != filter {
                            continue;
                        }
                    }

                    if !result.instances.is_empty() {
                        detection_results
                            .entry(provider)
                            .or_default()
                            .extend(result.instances);
                    }
                }
            }
        }

        if self.json {
            self.print_json(&detection_results, &root_path, files.len())?;
        } else {
            self.print_human(&detection_results, &root_path, files.len())?;
        }

        Ok(if detection_results.is_empty() {
            EXIT_CLEAN
        } else {
            EXIT_FINDINGS
        })
    }

    fn print_json(
        &self,
        results: &HashMap<Provider, Vec<DetectionInstance>>,
        root: &PathBuf,
        total_files: usize,
    ) -> Result<()> {
        let mut providers_data = Vec::new();

        for (provider, instances) in results {
            // Get unique files
            let mut unique_files: Vec<PathBuf> =
                instances.iter().map(|i| i.file_path.clone()).collect();
            unique_files.sort();
            unique_files.dedup();

            // Build detailed instances array with location info
            let instances_data: Vec<serde_json::Value> = instances
                .iter()
                .map(|inst| {
                    serde_json::json!({
                        "file": inst.file_path.strip_prefix(root).unwrap_or(&inst.file_path).to_string_lossy(),
                        "line": inst.line,
                        "column": inst.column,
                        "has_base_url": inst.has_base_url,
                        "current_base_url": inst.current_base_url,
                    })
                })
                .collect();

            providers_data.push(serde_json::json!({
                "name": provider.as_str(),
                "file_count": unique_files.len(),
                "instance_count": instances.len(),
                "files": unique_files.iter()
                    .map(|f| f.strip_prefix(root).unwrap_or(f).to_string_lossy())
                    .collect::<Vec<_>>(),
                "instances": instances_data,
            }));
        }

        let unique_file_count = {
            let mut all_files: Vec<&PathBuf> = results
                .values()
                .flat_map(|v| v.iter().map(|i| &i.file_path))
                .collect();
            all_files.sort();
            all_files.dedup();
            all_files.len()
        };

        let output = serde_json::json!({
            "total_files_scanned": total_files,
            "files_with_sdks": unique_file_count,
            "total_instances": results.values().map(std::vec::Vec::len).sum::<usize>(),
            "providers": providers_data,
        });

        println!("{}", serde_json::to_string_pretty(&output)?);

        Ok(())
    }

    fn print_human(
        &self,
        results: &HashMap<Provider, Vec<DetectionInstance>>,
        root: &PathBuf,
        total_files: usize,
    ) -> Result<()> {
        for (provider, instances) in results {
            // Get unique files
            let mut unique_files: Vec<PathBuf> =
                instances.iter().map(|i| i.file_path.clone()).collect();
            unique_files.sort();
            unique_files.dedup();

            println!(
                "\n{} SDK ({} files, {} instances)",
                provider.display_name(),
                unique_files.len(),
                instances.len()
            );

            for file in unique_files.iter().take(10) {
                let rel_path = file.strip_prefix(root).unwrap_or(file);
                // Show instances in this file
                let file_instances: Vec<&DetectionInstance> =
                    instances.iter().filter(|i| &i.file_path == file).collect();

                if file_instances.len() == 1 {
                    let inst = file_instances[0];
                    println!("├── {}:{}:{}", rel_path.display(), inst.line, inst.column);
                } else {
                    println!(
                        "├── {} ({} instances)",
                        rel_path.display(),
                        file_instances.len()
                    );
                    for inst in file_instances.iter().take(3) {
                        println!("│   └── line {}, column {}", inst.line, inst.column);
                    }
                    if file_instances.len() > 3 {
                        println!("│   └── ... and {} more", file_instances.len() - 3);
                    }
                }
            }

            if unique_files.len() > 10 {
                println!("└── ... and {} more files", unique_files.len() - 10);
            }
        }

        println!("\nSummary:");
        println!("  • Total files scanned: {total_files}");

        let total_instances: usize = results.values().map(std::vec::Vec::len).sum();
        println!("  • Total instances: {total_instances}");

        println!("\nProviders detected:");
        if results.is_empty() {
            println!("  (none)");
        } else {
            for provider in results.keys() {
                println!("  ✓ {}", provider.as_str());
            }
        }

        println!("\nNext: promptguard init");

        Ok(())
    }
}
