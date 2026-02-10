use crate::api::PromptGuardClient;
use crate::config::ConfigManager;
use crate::detector::detect_all_providers;
use crate::error::{PromptGuardError, Result};
use crate::output::Output;
use crate::scanner::FileScanner;
use crate::types::{DetectionInstance, Provider};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

/// Response from the /security/scan endpoint
#[derive(Debug, Deserialize, Serialize)]
pub struct SecurityScanResponse {
    pub decision: String,
    pub confidence: f64,
    #[serde(default)]
    pub threat_type: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub details: serde_json::Value,
}

pub struct ScanCommand {
    pub provider: Option<String>,
    pub json: bool,
    /// Text to scan for security threats via the API
    pub text: Option<String>,
    /// File path to scan for security threats via the API
    pub file: Option<String>,
}

impl ScanCommand {
    pub fn execute(&self) -> Result<()> {
        // If --text or --file is provided, do an API security scan instead of local SDK detection
        if self.text.is_some() || self.file.is_some() {
            return self.execute_api_scan();
        }

        // Otherwise, do local SDK detection
        self.execute_local_scan()
    }

    /// Scan text or file content for security threats via the backend API
    fn execute_api_scan(&self) -> Result<()> {
        let content = if let Some(ref text) = self.text {
            text.clone()
        } else if let Some(ref file_path) = self.file {
            fs::read_to_string(file_path).map_err(|e| {
                PromptGuardError::Io(std::io::Error::new(
                    e.kind(),
                    format!("Failed to read file '{}': {}", file_path, e),
                ))
            })?
        } else {
            return Err(PromptGuardError::Custom(
                "Either --text or --file must be provided".to_string(),
            ));
        };

        // Get API key from config
        let config_manager = ConfigManager::new(None)?;
        let config = config_manager.load()?;

        let client = PromptGuardClient::new(config.api_key, Some(config.proxy_url))?;

        if !self.json {
            Output::header(&format!(
                "🛡️  PromptGuard CLI v{}",
                env!("CARGO_PKG_VERSION")
            ));
            Output::section("Security Threat Scan", "🔍");
            Output::info(&format!("Scanning {} characters...", content.len()));
        }

        // Call the security scan endpoint
        let response: SecurityScanResponse = client.post(
            "/security/scan",
            &serde_json::json!({
                "text": content,
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
            println!("Decision: {}", decision_display);
            println!("Confidence: {:.1}%", response.confidence * 100.0);

            if let Some(ref threat_type) = response.threat_type {
                println!("Threat Type: {}", threat_type);
            }

            if let Some(ref reason) = response.reason {
                println!("Reason: {}", reason);
            }

            if response.decision == "block" {
                println!();
                Output::warning("This content was flagged as potentially malicious.");
            } else {
                println!();
                Output::success("No security threats detected.");
            }
        }

        Ok(())
    }

    /// Local SDK detection scan (original behavior)
    fn execute_local_scan(&self) -> Result<()> {
        if !self.json {
            Output::header(&format!(
                "🛡️  PromptGuard CLI v{}",
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

        Ok(())
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

        let output = serde_json::json!({
            "total_files_scanned": total_files,
            "files_with_sdks": results.values().flat_map(|v| v.iter()).count(),
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
                provider.class_name(),
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
