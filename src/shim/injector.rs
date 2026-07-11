/// Entry point injection logic
///
/// Detects application entry points and injects shim imports to enable
/// runtime interception of LLM SDK calls.
use crate::error::Result;
use crate::scanner::is_skip_dir;
use crate::types::Language;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

const PYTHON_SHIM_IMPORT: &str = "\n# PromptGuard runtime shim - auto-injected\nimport sys\nimport os\nsys.path.insert(0, os.path.join(os.path.dirname(__file__), '.promptguard'))\nimport promptguard_shim\n# PromptGuard runtime shim - end\n";

const PYTHON_SHIM_IMPORT_MARKER: &str = "# PromptGuard runtime shim - auto-injected";

const PYTHON_SHIM_END_MARKER: &str = "# PromptGuard runtime shim - end";

/// The exact lines injected by older CLI versions that did not emit an
/// end-marker. Used to remove legacy blocks without deleting user code.
const PYTHON_SHIM_LEGACY_LINES: &[&str] = &[
    "import sys",
    "import os",
    "sys.path.insert(0, os.path.join(os.path.dirname(__file__), '.promptguard'))",
    "import promptguard_shim",
];

/// Entry point detector and injector
pub struct ShimInjector {
    project_root: PathBuf,
}

impl ShimInjector {
    /// Create a new shim injector
    pub fn new(project_root: impl AsRef<Path>) -> Self {
        Self {
            project_root: project_root.as_ref().to_path_buf(),
        }
    }

    /// Detect Python entry points
    ///
    /// Looks for common Python entry point patterns:
    /// - Files with `if __name__ == "__main__":`
    /// - main.py, app.py, server.py, run.py
    /// - manage.py (Django)
    /// - wsgi.py, asgi.py (WSGI/ASGI apps)
    pub fn detect_python_entry_points(&self) -> Result<Vec<PathBuf>> {
        let mut entry_points = HashSet::new();

        // Common entry point filenames
        let common_entry_files = [
            "main.py",
            "app.py",
            "server.py",
            "run.py",
            "manage.py",
            "wsgi.py",
            "asgi.py",
            "__main__.py",
        ];

        for entry in WalkDir::new(&self.project_root)
            .max_depth(3) // Don't go too deep
            .follow_links(false)
        {
            let entry = entry.map_err(std::io::Error::other)?;
            let path = entry.path();

            if path
                .components()
                .any(|c| c.as_os_str().to_str().is_some_and(is_skip_dir))
            {
                continue;
            }

            if !path.is_file() {
                continue;
            }

            let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

            // Check if it's a common entry point file
            if common_entry_files.contains(&file_name) {
                entry_points.insert(path.to_path_buf());
                continue;
            }

            // Check if file contains if __name__ == "__main__":
            if path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("py"))
            {
                if let Ok(content) = fs::read_to_string(path) {
                    if content.contains("if __name__ == \"__main__\":")
                        || content.contains("if __name__ == '__main__':")
                    {
                        entry_points.insert(path.to_path_buf());
                    }
                }
            }
        }

        Ok(entry_points.into_iter().collect())
    }

    /// Detect TypeScript/JavaScript entry points
    ///
    /// Looks for:
    /// - package.json "main" field
    /// - index.ts, index.js, main.ts, main.js, app.ts, app.js, server.ts, server.js
    /// - Files in src/ directory matching entry patterns
    pub fn detect_typescript_entry_points(&self) -> Result<Vec<PathBuf>> {
        let mut entry_points = HashSet::new();

        // Check package.json for main entry point
        let package_json_path = self.project_root.join("package.json");
        if package_json_path.exists() {
            if let Ok(content) = fs::read_to_string(&package_json_path) {
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&content) {
                    // Check "main" field
                    if let Some(main) = parsed.get("main").and_then(|v| v.as_str()) {
                        let main_path = self.project_root.join(main);
                        if main_path.exists() {
                            entry_points.insert(main_path);
                        }
                    }

                    // Check "scripts" -> "start" for common entry patterns
                    if let Some(scripts) = parsed.get("scripts").and_then(|v| v.as_object()) {
                        if let Some(start) = scripts.get("start").and_then(|v| v.as_str()) {
                            // Extract file from script like "node dist/index.js"
                            for word in start.split_whitespace() {
                                let script_path = self.project_root.join(word);
                                if script_path.extension().is_some_and(|ext| {
                                    ext.eq_ignore_ascii_case("js") || ext.eq_ignore_ascii_case("ts")
                                }) && script_path.exists()
                                {
                                    entry_points.insert(script_path);
                                }
                            }
                        }
                    }
                }
            }
        }

        // Common entry point filenames
        let common_entry_files = [
            "index.ts",
            "index.js",
            "main.ts",
            "main.js",
            "app.ts",
            "app.js",
            "server.ts",
            "server.js",
        ];

        // Look in common source directories
        let search_dirs = vec!["src", "lib", "."];

        for dir in search_dirs {
            let search_path = self.project_root.join(dir);
            if !search_path.exists() {
                continue;
            }

            for entry in WalkDir::new(&search_path).max_depth(2).follow_links(false) {
                let entry = entry.map_err(std::io::Error::other)?;
                let path = entry.path();

                if path
                    .components()
                    .any(|c| c.as_os_str().to_str().is_some_and(is_skip_dir))
                {
                    continue;
                }

                if !path.is_file() {
                    continue;
                }

                let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

                if common_entry_files.contains(&file_name) {
                    entry_points.insert(path.to_path_buf());
                }
            }
        }

        Ok(entry_points.into_iter().collect())
    }

    /// Inject Python shim import into a file
    pub fn inject_python_shim(&self, file_path: &Path) -> Result<bool> {
        let content = fs::read_to_string(file_path)?;

        // Check if already injected
        if content.contains(PYTHON_SHIM_IMPORT_MARKER) {
            return Ok(false); // Already injected
        }

        // Inject at the top, after shebang and docstring if present
        let lines: Vec<&str> = content.lines().collect();
        let mut inject_pos = 0;

        // Skip shebang
        if !lines.is_empty() && lines[0].starts_with("#!") {
            inject_pos = 1;
        }

        // Skip module docstring
        if inject_pos < lines.len() {
            let remaining = &lines[inject_pos..].join("\n");
            if remaining.trim_start().starts_with("\"\"\"")
                || remaining.trim_start().starts_with("'''")
            {
                // Find end of docstring
                let quote = if remaining.trim_start().starts_with("\"\"\"") {
                    "\"\"\""
                } else {
                    "'''"
                };

                let mut in_docstring = false;
                for (i, line) in lines[inject_pos..].iter().enumerate() {
                    if line.trim().starts_with(quote) {
                        if in_docstring {
                            inject_pos += i + 1;
                            break;
                        }
                        in_docstring = true;
                    }
                }
            }
        }

        // Insert shim import
        let mut new_content = String::new();
        for (i, line) in lines.iter().enumerate() {
            if i == inject_pos {
                new_content.push_str(PYTHON_SHIM_IMPORT);
            }
            new_content.push_str(line);
            new_content.push('\n');
        }

        // Handle case where inject_pos is at the end
        if inject_pos >= lines.len() {
            new_content.push_str(PYTHON_SHIM_IMPORT);
        }

        fs::write(file_path, new_content)?;
        Ok(true)
    }

    /// Remove Python shim import from a file
    pub fn remove_python_shim(&self, file_path: &Path) -> Result<bool> {
        let content = fs::read_to_string(file_path)?;

        if !content.contains(PYTHON_SHIM_IMPORT_MARKER) {
            return Ok(false); // Not injected
        }

        // Remove the shim import block: start-marker to end-marker. For
        // legacy blocks (no end-marker), remove exactly the known injected
        // lines so user code is never deleted.
        let lines: Vec<&str> = content.lines().collect();
        let mut new_lines: Vec<&str> = Vec::new();
        let mut i = 0;

        while i < lines.len() {
            let line = lines[i];

            if line.contains(PYTHON_SHIM_IMPORT_MARKER) {
                // Injection always prepends one blank line before the marker;
                // drop it so inject + remove round-trips exactly.
                if new_lines.last().is_some_and(|l| l.trim().is_empty()) {
                    new_lines.pop();
                }

                i += 1; // skip the start-marker line

                let has_end_marker = lines[i..]
                    .iter()
                    .any(|l| l.contains(PYTHON_SHIM_END_MARKER));

                if has_end_marker {
                    // Skip everything up to and including the end marker.
                    while i < lines.len() && !lines[i].contains(PYTHON_SHIM_END_MARKER) {
                        i += 1;
                    }
                    i += 1; // skip the end-marker line itself
                } else {
                    // Legacy block: remove only the exact injected lines.
                    for expected in PYTHON_SHIM_LEGACY_LINES {
                        if i < lines.len() && lines[i].trim() == *expected {
                            i += 1;
                        }
                    }
                }
                continue;
            }

            new_lines.push(line);
            i += 1;
        }

        let new_content = new_lines.join("\n") + "\n";
        fs::write(file_path, new_content)?;
        Ok(true)
    }

    /// Inject TypeScript/JavaScript shim imports
    /// Inject shims into all detected entry points for a language
    pub fn inject_shims(&self, language: Language) -> Result<Vec<PathBuf>> {
        match language {
            Language::Python => {
                let entry_points = self.detect_python_entry_points()?;
                let mut injected = Vec::new();

                for entry_point in entry_points {
                    if self.inject_python_shim(&entry_point)? {
                        injected.push(entry_point);
                    }
                }

                Ok(injected)
            },
            Language::TypeScript | Language::JavaScript => {
                // For TS/JS, we don't auto-inject, just detect
                let entry_points = self.detect_typescript_entry_points()?;
                Ok(entry_points)
            },
        }
    }

    /// Remove shim injections from all files
    pub fn remove_all_injections(&self) -> Result<usize> {
        let mut removed_count = 0;

        // Find all Python files with injections
        for entry in WalkDir::new(&self.project_root)
            .max_depth(5)
            .follow_links(false)
        {
            let entry = entry.map_err(std::io::Error::other)?;
            let path = entry.path();

            if !path.is_file() {
                continue;
            }

            if path.extension().and_then(|e| e.to_str()) == Some("py")
                && self.remove_python_shim(path)?
            {
                removed_count += 1;
            }
        }

        Ok(removed_count)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_detect_python_entry_points() {
        let temp_dir = TempDir::new().unwrap();

        // Create a main.py
        fs::write(temp_dir.path().join("main.py"), "print('hello')").unwrap();

        // Create a file with __main__
        fs::write(
            temp_dir.path().join("script.py"),
            "if __name__ == \"__main__\":\n    pass",
        )
        .unwrap();

        let injector = ShimInjector::new(temp_dir.path());
        let entry_points = injector.detect_python_entry_points().unwrap();

        assert!(entry_points.len() >= 2);
        assert!(entry_points.iter().any(|p| p.ends_with("main.py")));
        assert!(entry_points.iter().any(|p| p.ends_with("script.py")));
    }

    #[test]
    fn test_inject_python_shim() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("test.py");

        fs::write(&test_file, "#!/usr/bin/env python3\nprint('hello')").unwrap();

        let injector = ShimInjector::new(temp_dir.path());
        let injected = injector.inject_python_shim(&test_file).unwrap();

        assert!(injected);

        let content = fs::read_to_string(&test_file).unwrap();
        assert!(content.contains("import promptguard_shim"));
        assert!(content.contains("sys.path.insert"));
    }

    #[test]
    fn test_remove_python_shim() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("test.py");

        fs::write(&test_file, "print('hello')").unwrap();

        let injector = ShimInjector::new(temp_dir.path());

        // Inject
        injector.inject_python_shim(&test_file).unwrap();
        let after_inject = fs::read_to_string(&test_file).unwrap();
        assert!(after_inject.contains("import promptguard_shim"));

        // Remove
        let removed = injector.remove_python_shim(&test_file).unwrap();
        assert!(removed);

        let after_remove = fs::read_to_string(&test_file).unwrap();
        assert!(!after_remove.contains("import promptguard_shim"));
    }

    /// Regression test: user code following the injected block (with no blank
    /// line separating it) must survive an inject/remove round-trip.
    #[test]
    fn test_remove_python_shim_preserves_user_code() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("test.py");

        let original = "#!/usr/bin/env python3\nimport json\n\ndef main():\n    print('hello')\n\nif __name__ == \"__main__\":\n    main()\n";
        fs::write(&test_file, original).unwrap();

        let injector = ShimInjector::new(temp_dir.path());
        injector.inject_python_shim(&test_file).unwrap();
        injector.remove_python_shim(&test_file).unwrap();

        let after = fs::read_to_string(&test_file).unwrap();
        assert_eq!(after, original, "user code must survive inject/remove");
    }

    /// Round-trip with trailing blank lines in the original file.
    #[test]
    fn test_remove_python_shim_preserves_trailing_blank_lines() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("test.py");

        let original = "import json\nprint('a')\nprint('b')\n\n";
        fs::write(&test_file, original).unwrap();

        let injector = ShimInjector::new(temp_dir.path());
        injector.inject_python_shim(&test_file).unwrap();
        injector.remove_python_shim(&test_file).unwrap();

        let after = fs::read_to_string(&test_file).unwrap();
        // lines() drops trailing blank lines and we re-add a single final
        // newline, so compare line content (user code intact).
        assert_eq!(
            after.trim_end(),
            original.trim_end(),
            "user code must survive inject/remove"
        );
        assert!(!after.contains("promptguard_shim"));
    }

    /// Legacy blocks (injected by older versions without an end-marker) must
    /// be removed without deleting the user code that follows them.
    #[test]
    fn test_remove_legacy_python_shim_block_without_end_marker() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("test.py");

        // Simulate an old-format injection: no end-marker, user code
        // immediately after the injected lines (no blank line).
        let legacy = "\n# PromptGuard runtime shim - auto-injected\nimport sys\nimport os\nsys.path.insert(0, os.path.join(os.path.dirname(__file__), '.promptguard'))\nimport promptguard_shim\nprint('user code 1')\nprint('user code 2')\n";
        fs::write(&test_file, legacy).unwrap();

        let injector = ShimInjector::new(temp_dir.path());
        let removed = injector.remove_python_shim(&test_file).unwrap();
        assert!(removed);

        let after = fs::read_to_string(&test_file).unwrap();
        assert!(!after.contains("promptguard_shim"));
        assert!(after.contains("print('user code 1')"), "user code deleted");
        assert!(after.contains("print('user code 2')"), "user code deleted");
    }
}
