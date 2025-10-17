use crate::detector::detect_all_providers;
use crate::error::Result;
use crate::output::Output;
use crate::scanner::FileScanner;
use crate::types::Provider;
use std::collections::HashMap;
use std::path::PathBuf;

pub struct ScanCommand {
    pub provider: Option<String>,
    pub json: bool,
}

impl ScanCommand {
    pub fn execute(&self) -> Result<()> {
        if !self.json {
            Output::header(&format!("🛡️  PromptGuard CLI v{}", env!("CARGO_PKG_VERSION")));
            Output::section("LLM SDK Detection Report", "📊");
        }

        let root_path = std::env::current_dir()?;
        let scanner = FileScanner::new(&root_path, None)?;
        let files = scanner.scan_files(None)?;

        let mut detection_results: HashMap<Provider, Vec<PathBuf>> = HashMap::new();

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
                            .or_insert_with(Vec::new)
                            .push(file_path.clone());
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

    fn print_json(&self, results: &HashMap<Provider, Vec<PathBuf>>, root: &PathBuf, total_files: usize) -> Result<()> {
        let mut providers_data = Vec::new();

        for (provider, files) in results {
            let mut unique_files = files.clone();
            unique_files.sort();
            unique_files.dedup();

            providers_data.push(serde_json::json!({
                "name": provider.as_str(),
                "file_count": unique_files.len(),
                "instance_count": files.len(),
                "files": unique_files.iter()
                    .map(|f| f.strip_prefix(root).unwrap_or(f).to_string_lossy())
                    .collect::<Vec<_>>(),
            }));
        }

        let output = serde_json::json!({
            "total_files_scanned": total_files,
            "files_with_sdks": results.values().flat_map(|v| v.iter()).count(),
            "total_instances": results.values().map(|v| v.len()).sum::<usize>(),
            "providers": providers_data,
        });

        println!("{}", serde_json::to_string_pretty(&output)?);

        Ok(())
    }

    fn print_human(&self, results: &HashMap<Provider, Vec<PathBuf>>, root: &PathBuf, total_files: usize) -> Result<()> {
        for (provider, files) in results {
            let mut unique_files = files.clone();
            unique_files.sort();
            unique_files.dedup();

            println!("\n{} SDK ({} files, {} instances)", provider.class_name(), unique_files.len(), files.len());

            for file in unique_files.iter().take(10) {
                let rel_path = file.strip_prefix(root).unwrap_or(file);
                println!("├── {}", rel_path.display());
            }

            if unique_files.len() > 10 {
                println!("└── ... and {} more", unique_files.len() - 10);
            }
        }

        println!("\nSummary:");
        println!("  • Total files scanned: {}", total_files);

        let total_instances: usize = results.values().map(|v| v.len()).sum();
        println!("  • Total instances: {}", total_instances);

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
