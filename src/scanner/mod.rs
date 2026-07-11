use crate::config::default_exclude_patterns;
use crate::error::Result;
use glob::Pattern;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Directories always skipped during filesystem traversal.
/// Canonical list - used by scanner, envscanner, and injector.
pub const SKIP_DIRS: &[&str] = &[
    "node_modules",
    ".git",
    "dist",
    "build",
    "__pycache__",
    "__tests__",
    "venv",
    ".venv",
    ".next",
    ".mypy_cache",
];

pub fn is_skip_dir(name: &str) -> bool {
    SKIP_DIRS.contains(&name)
}

pub struct FileScanner {
    root_path: PathBuf,
    exclude_patterns: Vec<Pattern>,
}

impl FileScanner {
    pub fn new<P: AsRef<Path>>(
        root_path: P,
        exclude_patterns: Option<Vec<String>>,
    ) -> Result<Self> {
        let patterns = exclude_patterns.unwrap_or_else(|| {
            let mut p = default_exclude_patterns();
            p.extend([
                "**/.git/**".to_string(),
                "**/__pycache__/**".to_string(),
                "**/*.pyc".to_string(),
            ]);
            p
        });
        let exclude_patterns: Result<Vec<Pattern>> = patterns
            .iter()
            .map(|p| {
                Pattern::new(p).map_err(|e| crate::error::PromptGuardError::Custom(e.to_string()))
            })
            .collect();

        Ok(Self {
            root_path: root_path.as_ref().to_path_buf(),
            exclude_patterns: exclude_patterns?,
        })
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
                if let Some(dev_dependencies) =
                    json.get("devDependencies").and_then(|v| v.as_object())
                {
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
        for filename in ["requirements.txt", "pyproject.toml"] {
            if let Some(fw) = self.detect_python_framework(filename) {
                return Some(fw);
            }
        }

        None
    }

    fn detect_python_framework(&self, filename: &str) -> Option<String> {
        let content = fs::read_to_string(self.root_path.join(filename)).ok()?;
        let lower = content.to_lowercase();
        for fw in ["django", "fastapi", "flask"] {
            if lower.contains(fw) {
                return Some(fw.to_string());
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

        // Prune skip directories at the walker level: without filter_entry,
        // node_modules/.git/.venv were fully enumerated (tens of thousands
        // of entries) only to be discarded one path at a time by the
        // exclude patterns. Exclude patterns still apply below for
        // file-level filtering.
        for entry in WalkDir::new(&self.root_path)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| {
                e.depth() == 0
                    || !(e.file_type().is_dir() && e.file_name().to_str().is_some_and(is_skip_dir))
            })
            .filter_map(std::result::Result::ok)
        {
            let path = entry.path();

            // follow_links(false) stops walkdir from traversing INTO
            // symlinked directories, but symlinked files are still yielded
            // (and path.is_file() follows the link). Skip them: transforming
            // through a symlink writes outside the project tree.
            if entry.file_type().is_symlink() {
                continue;
            }

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
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Skip directories must be pruned by the walker itself, independent of
    /// the exclude patterns (here: an explicitly empty pattern list).
    #[test]
    fn skip_dirs_pruned_even_without_exclude_patterns() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();

        for dir in ["node_modules/pkg", ".git/objects", ".venv/lib", "src"] {
            fs::create_dir_all(root.join(dir)).unwrap();
        }
        fs::write(root.join("node_modules/pkg/index.js"), "x").unwrap();
        fs::write(root.join(".git/objects/blob.py"), "x").unwrap();
        fs::write(root.join(".venv/lib/site.py"), "x").unwrap();
        fs::write(root.join("src/app.py"), "x").unwrap();

        let scanner = FileScanner::new(root, Some(Vec::new())).unwrap();
        let files = scanner.scan_files(None).unwrap();

        assert_eq!(files.len(), 1, "only src/app.py should survive: {files:?}");
        assert!(files[0].ends_with("src/app.py"));
    }

    /// A project root that itself has a skip-dir name must still be scanned.
    #[test]
    fn root_named_like_skip_dir_is_still_scanned() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path().join("build");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("app.py"), "x").unwrap();

        let scanner = FileScanner::new(&root, Some(Vec::new())).unwrap();
        let files = scanner.scan_files(None).unwrap();
        assert_eq!(files.len(), 1);
    }
}
