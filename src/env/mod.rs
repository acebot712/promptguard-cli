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

        let mut lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();

        // Find and update existing key
        let key_prefix = format!("{}=", key);
        let mut found = false;

        for line in lines.iter_mut() {
            if line.starts_with(&key_prefix) || line.starts_with(&format!("export {}", key_prefix)) {
                *line = format!("{}={}", key, value);
                found = true;
                break;
            }
        }

        // Add if not found
        if !found {
            if !lines.is_empty() && !lines.last().unwrap().is_empty() {
                lines.push(String::new()); // Add blank line before
            }
            lines.push(format!("{}={}", key, value));
        }

        let new_content = lines.join("\n");
        fs::write(env_path, new_content)?;

        Ok(())
    }

    // Alias for add_or_update_key
    pub fn set_key(env_path: &Path, key: &str, value: &str) -> Result<()> {
        Self::add_or_update_key(env_path, key, value)
    }

    pub fn remove_key(env_path: &Path, key: &str) -> Result<bool> {
        if !env_path.exists() {
            return Ok(false);
        }

        let content = fs::read_to_string(env_path)?;
        let key_prefix = format!("{}=", key);

        let new_lines: Vec<String> = content
            .lines()
            .filter(|line| {
                !line.starts_with(&key_prefix) && !line.starts_with(&format!("export {}", key_prefix))
            })
            .map(|l| l.to_string())
            .collect();

        let removed = new_lines.len() < content.lines().count();

        if removed {
            fs::write(env_path, new_lines.join("\n"))?;
        }

        Ok(removed)
    }

    pub fn has_key(env_path: &Path, key: &str) -> bool {
        if !env_path.exists() {
            return false;
        }

        if let Ok(content) = fs::read_to_string(env_path) {
            let key_prefix = format!("{}=", key);
            content
                .lines()
                .any(|line| line.starts_with(&key_prefix) || line.starts_with(&format!("export {}", key_prefix)))
        } else {
            false
        }
    }

    pub fn get_value(env_path: &Path, key: &str) -> Option<String> {
        if !env_path.exists() {
            return None;
        }

        let content = fs::read_to_string(env_path).ok()?;
        let key_prefix = format!("{}=", key);

        for line in content.lines() {
            if line.starts_with(&key_prefix) {
                return Some(line[key_prefix.len()..].to_string());
            }
            let export_prefix = format!("export {}=", key);
            if line.starts_with(&export_prefix) {
                return Some(line[export_prefix.len()..].to_string());
            }
        }

        None
    }
}
