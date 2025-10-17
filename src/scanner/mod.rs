use crate::error::Result;
use glob::Pattern;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub struct FileScanner {
    root_path: PathBuf,
    exclude_patterns: Vec<Pattern>,
}

impl FileScanner {
    pub fn new<P: AsRef<Path>>(root_path: P, exclude_patterns: Option<Vec<String>>) -> Result<Self> {
        let patterns = exclude_patterns.unwrap_or_else(Self::default_exclude_patterns);
        let exclude_patterns: Result<Vec<Pattern>> = patterns
            .iter()
            .map(|p| Pattern::new(p).map_err(|e| crate::error::PromptGuardError::Custom(e.to_string())))
            .collect();

        Ok(Self {
            root_path: root_path.as_ref().to_path_buf(),
            exclude_patterns: exclude_patterns?,
        })
    }

    pub fn default_exclude_patterns() -> Vec<String> {
        vec![
            "**/*.test.js".to_string(),
            "**/*.test.ts".to_string(),
            "**/*.spec.js".to_string(),
            "**/*.spec.ts".to_string(),
            "**/node_modules/**".to_string(),
            "**/dist/**".to_string(),
            "**/__tests__/**".to_string(),
            "**/.venv/**".to_string(),
            "**/venv/**".to_string(),
            "**/.git/**".to_string(),
            "**/__pycache__/**".to_string(),
            "**/*.pyc".to_string(),
        ]
    }

    pub fn find_git_root(&self) -> Option<PathBuf> {
        let mut current = self.root_path.clone();
        loop {
            if current.join(".git").exists() {
                return Some(current);
            }
            if !current.pop() {
                break;
            }
        }
        None
    }

    pub fn detect_framework(&self) -> Option<String> {
        // Check for Next.js
        if self.root_path.join("next.config.js").exists()
            || self.root_path.join("next.config.mjs").exists()
            || self.root_path.join("next.config.ts").exists()
        {
            return Some("nextjs".to_string());
        }

        // Check package.json
        if let Ok(content) = fs::read_to_string(self.root_path.join("package.json")) {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
                let mut deps = Vec::new();
                if let Some(dependencies) = json.get("dependencies").and_then(|v| v.as_object()) {
                    deps.extend(dependencies.keys());
                }
                if let Some(dev_dependencies) = json.get("devDependencies").and_then(|v| v.as_object()) {
                    deps.extend(dev_dependencies.keys());
                }

                if deps.iter().any(|k| k.as_str() == "next") {
                    return Some("nextjs".to_string());
                }
                if deps.iter().any(|k| k.as_str() == "express") {
                    return Some("express".to_string());
                }
            }
        }

        // Check Python frameworks
        if let Ok(content) = fs::read_to_string(self.root_path.join("requirements.txt")) {
            let lower = content.to_lowercase();
            if lower.contains("django") {
                return Some("django".to_string());
            }
            if lower.contains("fastapi") {
                return Some("fastapi".to_string());
            }
            if lower.contains("flask") {
                return Some("flask".to_string());
            }
        }

        if let Ok(content) = fs::read_to_string(self.root_path.join("pyproject.toml")) {
            let lower = content.to_lowercase();
            if lower.contains("django") {
                return Some("django".to_string());
            }
            if lower.contains("fastapi") {
                return Some("fastapi".to_string());
            }
            if lower.contains("flask") {
                return Some("flask".to_string());
            }
        }

        None
    }

    fn should_exclude(&self, path: &Path) -> bool {
        let rel_path = match path.strip_prefix(&self.root_path) {
            Ok(p) => p,
            Err(_) => return false,
        };

        let path_str = rel_path.to_string_lossy();

        for pattern in &self.exclude_patterns {
            if pattern.matches(&path_str) {
                return true;
            }
            // Also check filename alone
            if let Some(filename) = path.file_name() {
                if pattern.matches(&filename.to_string_lossy()) {
                    return true;
                }
            }
        }

        false
    }

    pub fn scan_files(&self, extensions: Option<Vec<String>>) -> Result<Vec<PathBuf>> {
        let exts = extensions.unwrap_or_else(|| {
            vec![
                "ts".to_string(),
                "tsx".to_string(),
                "js".to_string(),
                "jsx".to_string(),
                "py".to_string(),
            ]
        });

        let mut files = Vec::new();

        for entry in WalkDir::new(&self.root_path)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();

            if !path.is_file() {
                continue;
            }

            if self.should_exclude(path) {
                continue;
            }

            if let Some(ext) = path.extension() {
                if exts.iter().any(|e| e == &ext.to_string_lossy()) {
                    files.push(path.to_path_buf());
                }
            }
        }

        // Sort by modification time (newest first)
        files.sort_by_cached_key(|p| {
            std::cmp::Reverse(
                p.metadata()
                    .and_then(|m| m.modified())
                    .unwrap_or(std::time::SystemTime::UNIX_EPOCH),
            )
        });

        Ok(files)
    }

    pub fn root_path(&self) -> &Path {
        &self.root_path
    }
}
