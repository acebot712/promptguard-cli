/// Environment variable scanner
///
/// Scans .env files and code for environment variable usage related to LLM SDKs.
/// Helps users understand what environment variables need to be configured.
use crate::error::Result;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Debug, Clone)]
pub struct EnvVariable {
    pub name: String,
    pub value: Option<String>,
    pub file: PathBuf,
    pub line: usize,
}

#[derive(Debug, Clone)]
pub struct EnvUsage {
    pub var_name: String,
    pub file: PathBuf,
    pub line: usize,
    pub context: String,
}

/// Environment variable scanner
pub struct EnvScanner {
    project_root: PathBuf,
}

impl EnvScanner {
    /// Create a new environment scanner
    pub fn new(project_root: impl AsRef<Path>) -> Self {
        Self {
            project_root: project_root.as_ref().to_path_buf(),
        }
    }

    /// Scan for .env files in the project
    pub fn find_env_files(&self) -> Result<Vec<PathBuf>> {
        let mut env_files = Vec::new();

        // Common .env file patterns
        let env_patterns = [
            ".env",
            ".env.local",
            ".env.development",
            ".env.production",
            ".env.test",
            ".env.example",
        ];

        for entry in WalkDir::new(&self.project_root)
            .max_depth(3)
            .follow_links(false)
        {
            let entry = entry.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
            let path = entry.path();

            // Skip node_modules and other build directories
            if path.components().any(|c| {
                matches!(
                    c.as_os_str().to_str(),
                    Some("node_modules" | ".git" | "dist" | "build" | "venv" | ".venv")
                )
            }) {
                continue;
            }

            if !path.is_file() {
                continue;
            }

            let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

            if env_patterns.iter().any(|pattern| file_name == *pattern) {
                env_files.push(path.to_path_buf());
            }
        }

        Ok(env_files)
    }

    /// Parse a .env file and extract variables
    pub fn parse_env_file(&self, path: &Path) -> Result<Vec<EnvVariable>> {
        let content = fs::read_to_string(path)?;
        let mut variables = Vec::new();

        for (line_num, line) in content.lines().enumerate() {
            let trimmed = line.trim();

            // Skip comments and empty lines
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            // Parse KEY=VALUE format
            if let Some(equals_pos) = trimmed.find('=') {
                let name = trimmed[..equals_pos].trim().to_string();
                let value_part = trimmed[equals_pos + 1..].trim();

                // Remove quotes if present
                let value = if (value_part.starts_with('"') && value_part.ends_with('"'))
                    || (value_part.starts_with('\'') && value_part.ends_with('\''))
                {
                    value_part[1..value_part.len() - 1].to_string()
                } else {
                    value_part.to_string()
                };

                variables.push(EnvVariable {
                    name,
                    value: Some(value),
                    file: path.to_path_buf(),
                    line: line_num + 1,
                });
            }
        }

        Ok(variables)
    }

    /// Scan all .env files and parse variables
    pub fn scan_env_variables(&self) -> Result<Vec<EnvVariable>> {
        let env_files = self.find_env_files()?;
        let mut all_variables = Vec::new();

        for env_file in env_files {
            let variables = self.parse_env_file(&env_file)?;
            all_variables.extend(variables);
        }

        Ok(all_variables)
    }

    /// Find environment variables that look like API URLs or keys
    pub fn find_api_related_vars(&self) -> Result<Vec<EnvVariable>> {
        let all_vars = self.scan_env_variables()?;

        let api_keywords = [
            "API",
            "KEY",
            "SECRET",
            "TOKEN",
            "URL",
            "ENDPOINT",
            "BASE",
            "OPENAI",
            "ANTHROPIC",
            "COHERE",
            "HUGGINGFACE",
        ];

        let api_vars: Vec<EnvVariable> = all_vars
            .into_iter()
            .filter(|var| {
                api_keywords
                    .iter()
                    .any(|keyword| var.name.to_uppercase().contains(keyword))
            })
            .collect();

        Ok(api_vars)
    }

    /// Scan Python code for environment variable usage
    pub fn scan_python_env_usage(&self) -> Result<Vec<EnvUsage>> {
        let mut usages = Vec::new();

        for entry in WalkDir::new(&self.project_root)
            .max_depth(5)
            .follow_links(false)
        {
            let entry = entry.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
            let path = entry.path();

            // Skip non-Python files and build directories
            if !path.is_file() || path.extension().and_then(|e| e.to_str()) != Some("py") {
                continue;
            }

            if path.components().any(|c| {
                matches!(
                    c.as_os_str().to_str(),
                    Some("venv" | ".venv" | "build" | "dist" | "__pycache__")
                )
            }) {
                continue;
            }

            let content = fs::read_to_string(path)?;

            for (line_num, line) in content.lines().enumerate() {
                // Look for os.environ, os.getenv
                if line.contains("os.environ") || line.contains("os.getenv") {
                    // Try to extract variable name
                    if let Some(var_name) = Self::extract_env_var_from_python(line) {
                        usages.push(EnvUsage {
                            var_name,
                            file: path.to_path_buf(),
                            line: line_num + 1,
                            context: line.trim().to_string(),
                        });
                    }
                }
            }
        }

        Ok(usages)
    }

    /// Scan TypeScript/JavaScript code for environment variable usage
    pub fn scan_typescript_env_usage(&self) -> Result<Vec<EnvUsage>> {
        let mut usages = Vec::new();

        for entry in WalkDir::new(&self.project_root)
            .max_depth(5)
            .follow_links(false)
        {
            let entry = entry.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
            let path = entry.path();

            // Check for JS/TS files
            if !path.is_file() {
                continue;
            }

            let ext = path.extension().and_then(|e| e.to_str());
            if !matches!(ext, Some("ts" | "tsx" | "js" | "jsx")) {
                continue;
            }

            if path.components().any(|c| {
                matches!(
                    c.as_os_str().to_str(),
                    Some("node_modules" | "dist" | "build" | ".next")
                )
            }) {
                continue;
            }

            let content = fs::read_to_string(path)?;

            for (line_num, line) in content.lines().enumerate() {
                // Look for process.env
                if line.contains("process.env") {
                    if let Some(var_name) = Self::extract_env_var_from_typescript(line) {
                        usages.push(EnvUsage {
                            var_name,
                            file: path.to_path_buf(),
                            line: line_num + 1,
                            context: line.trim().to_string(),
                        });
                    }
                }
            }
        }

        Ok(usages)
    }

    /// Extract environment variable name from Python code
    fn extract_env_var_from_python(line: &str) -> Option<String> {
        // Pattern: os.environ["VAR"] or os.environ.get("VAR") or os.getenv("VAR")
        if let Some(start) = line
            .find("os.environ[\"")
            .or_else(|| line.find("os.environ['"))
            .or_else(|| line.find("os.environ.get(\""))
            .or_else(|| line.find("os.environ.get('"))
            .or_else(|| line.find("os.getenv(\""))
            .or_else(|| line.find("os.getenv('"))
        {
            let after = &line[start..];
            let quote = if after.contains('"') { "\"" } else { "'" };

            if let Some(quote_start) = after.find(quote) {
                if let Some(quote_end) = after[quote_start + 1..].find(quote) {
                    let var_name = &after[quote_start + 1..quote_start + 1 + quote_end];
                    return Some(var_name.to_string());
                }
            }
        }

        None
    }

    /// Extract environment variable name from TypeScript/JavaScript code
    fn extract_env_var_from_typescript(line: &str) -> Option<String> {
        // Pattern: process.env.VAR or process.env["VAR"]
        if let Some(start) = line.find("process.env.") {
            let after = &line[start + 12..]; // "process.env.".len() == 12
            let var_name: String = after
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();

            if !var_name.is_empty() {
                return Some(var_name);
            }
        } else if let Some(start) = line
            .find("process.env[\"")
            .or_else(|| line.find("process.env['"))
        {
            let after = &line[start..];
            let quote = if after.contains('"') { "\"" } else { "'" };

            if let Some(quote_start) = after.find(quote) {
                if let Some(quote_end) = after[quote_start + 1..].find(quote) {
                    let var_name = &after[quote_start + 1..quote_start + 1 + quote_end];
                    return Some(var_name.to_string());
                }
            }
        }

        None
    }

    /// Generate a report of environment variable usage
    pub fn generate_report(&self) -> Result<String> {
        let mut report = String::new();

        // Scan .env files
        let env_vars = self.find_api_related_vars()?;
        let python_usage = self.scan_python_env_usage()?;
        let typescript_usage = self.scan_typescript_env_usage()?;

        // Group by variable name
        let mut var_map: HashMap<String, Vec<String>> = HashMap::new();

        for var in &env_vars {
            let entry = var_map.entry(var.name.clone()).or_default();
            let rel_path = var
                .file
                .strip_prefix(&self.project_root)
                .unwrap_or(&var.file);
            entry.push(format!("Defined in {}:{}", rel_path.display(), var.line));
        }

        for usage in python_usage.iter().chain(typescript_usage.iter()) {
            let entry = var_map.entry(usage.var_name.clone()).or_default();
            let rel_path = usage
                .file
                .strip_prefix(&self.project_root)
                .unwrap_or(&usage.file);
            entry.push(format!("Used in {}:{}", rel_path.display(), usage.line));
        }

        if var_map.is_empty() {
            report.push_str("No environment variables detected.\n");
            return Ok(report);
        }

        report.push_str("Environment Variables Detected:\n\n");

        for (var_name, locations) in &var_map {
            report.push_str(&format!("  {var_name}\n"));
            for location in locations {
                report.push_str(&format!("    - {location}\n"));
            }
            report.push('\n');
        }

        Ok(report)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_parse_env_file() {
        let temp_dir = TempDir::new().unwrap();
        let env_file = temp_dir.path().join(".env");

        fs::write(
            &env_file,
            "OPENAI_API_KEY=sk-test123\nOPENAI_BASE_URL=https://api.example.com\n# Comment\nEMPTY=\n",
        )
        .unwrap();

        let scanner = EnvScanner::new(temp_dir.path());
        let vars = scanner.parse_env_file(&env_file).unwrap();

        assert_eq!(vars.len(), 3);
        assert_eq!(vars[0].name, "OPENAI_API_KEY");
        assert_eq!(vars[0].value.as_ref().unwrap(), "sk-test123");
    }

    #[test]
    fn test_extract_env_var_from_python() {
        assert_eq!(
            EnvScanner::extract_env_var_from_python("api_key = os.environ[\"OPENAI_API_KEY\"]"),
            Some("OPENAI_API_KEY".to_string())
        );

        assert_eq!(
            EnvScanner::extract_env_var_from_python("url = os.getenv('BASE_URL')"),
            Some("BASE_URL".to_string())
        );
    }

    #[test]
    fn test_extract_env_var_from_typescript() {
        assert_eq!(
            EnvScanner::extract_env_var_from_typescript("const key = process.env.OPENAI_API_KEY"),
            Some("OPENAI_API_KEY".to_string())
        );

        assert_eq!(
            EnvScanner::extract_env_var_from_typescript("const url = process.env[\"BASE_URL\"]"),
            Some("BASE_URL".to_string())
        );
    }
}
