use crate::error::Result;
use std::fs;
use std::path::Path;

pub struct EnvManager;

impl EnvManager {
    pub fn add_or_update_key(env_path: &Path, key: &str, value: &str) -> Result<()> {
        let content = if env_path.exists() {
            fs::read_to_string(env_path)?
        } else {
            String::new()
        };

        let mut lines: Vec<String> = content
            .lines()
            .map(std::string::ToString::to_string)
            .collect();

        // Find and update existing key
        let key_prefix = format!("{key}=");
        let mut found = false;

        for line in &mut lines {
            if line.starts_with(&key_prefix) || line.starts_with(&format!("export {key_prefix}")) {
                *line = format!("{key}={value}");
                found = true;
                break;
            }
        }

        // Add if not found
        if !found {
            if let Some(last_line) = lines.last() {
                if !last_line.is_empty() {
                    lines.push(String::new()); // Add blank line before
                }
            }
            lines.push(format!("{key}={value}"));
        }

        // Preserve the file's dominant line ending and trailing-newline state
        // instead of forcing LF.
        let new_content = crate::text::join_preserving_style(&lines, &content);
        // .env files hold secrets: write with owner-only permissions.
        crate::config::write_private_file(env_path, &new_content)?;

        Ok(())
    }

    pub fn remove_key(env_path: &Path, key: &str) -> Result<bool> {
        if !env_path.exists() {
            return Ok(false);
        }

        let content = fs::read_to_string(env_path)?;
        let key_prefix = format!("{key}=");

        let new_lines: Vec<String> = content
            .lines()
            .filter(|line| {
                !line.starts_with(&key_prefix) && !line.starts_with(&format!("export {key_prefix}"))
            })
            .map(std::string::ToString::to_string)
            .collect();

        let removed = new_lines.len() < content.lines().count();

        if removed {
            let new_content = crate::text::join_preserving_style(&new_lines, &content);
            fs::write(env_path, new_content)?;
        }

        Ok(removed)
    }

    pub fn has_key(env_path: &Path, key: &str) -> bool {
        if !env_path.exists() {
            return false;
        }

        if let Ok(content) = fs::read_to_string(env_path) {
            let key_prefix = format!("{key}=");
            content.lines().any(|line| {
                line.starts_with(&key_prefix) || line.starts_with(&format!("export {key_prefix}"))
            })
        } else {
            false
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn add_key_preserves_crlf() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join(".env");
        fs::write(&path, "EXISTING=1\r\nOTHER=2\r\n").unwrap();

        EnvManager::add_or_update_key(&path, "PROMPTGUARD_API_KEY", "pg_live_x").unwrap();

        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("PROMPTGUARD_API_KEY=pg_live_x"));
        assert!(
            content.contains("\r\n"),
            "CRLF must be preserved:\n{content:?}"
        );
        assert_eq!(
            content.matches('\n').count(),
            content.matches("\r\n").count(),
            "no bare LF introduced:\n{content:?}"
        );
    }

    #[test]
    fn update_existing_key_preserves_crlf() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join(".env");
        fs::write(&path, "PROMPTGUARD_API_KEY=old\r\nOTHER=2\r\n").unwrap();

        EnvManager::add_or_update_key(&path, "PROMPTGUARD_API_KEY", "new").unwrap();

        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("PROMPTGUARD_API_KEY=new"));
        assert!(!content.contains("=old"));
        assert_eq!(
            content.matches('\n').count(),
            content.matches("\r\n").count(),
            "CRLF preserved on update:\n{content:?}"
        );
    }
}
