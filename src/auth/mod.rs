use crate::error::{PromptGuardError, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalCredentials {
    pub api_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_project: Option<String>,
}

fn credentials_dir() -> Result<PathBuf> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map_err(|_| PromptGuardError::Config("Cannot determine home directory".to_string()))?;
    Ok(PathBuf::from(home).join(".promptguard"))
}

fn credentials_path() -> Result<PathBuf> {
    Ok(credentials_dir()?.join("credentials.json"))
}

pub fn load_credentials() -> Result<Option<GlobalCredentials>> {
    let path = credentials_path()?;
    if !path.exists() {
        return Ok(None);
    }

    let content = fs::read_to_string(&path)?;
    let creds: GlobalCredentials = serde_json::from_str(&content)
        .map_err(|e| PromptGuardError::Config(format!("Failed to parse credentials: {e}")))?;
    Ok(Some(creds))
}

pub fn save_credentials(creds: &GlobalCredentials) -> Result<()> {
    let dir = credentials_dir()?;
    fs::create_dir_all(&dir)?;

    let path = dir.join("credentials.json");
    let content = serde_json::to_string_pretty(creds)
        .map_err(|e| PromptGuardError::Config(format!("Failed to serialize credentials: {e}")))?;

    fs::write(&path, &content)?;

    // Restrict permissions on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = fs::Permissions::from_mode(0o600);
        fs::set_permissions(&path, perms)?;
    }

    Ok(())
}

pub fn delete_credentials() -> Result<()> {
    let path = credentials_path()?;
    if path.exists() {
        fs::remove_file(&path)?;
    }
    Ok(())
}

/// Resolve API key with precedence: env var > project-local > global credentials
pub fn resolve_api_key() -> Result<String> {
    // 1. Environment variable (highest priority)
    if let Ok(key) = std::env::var("PROMPTGUARD_API_KEY") {
        if !key.is_empty() {
            return Ok(key);
        }
    }

    // 2. Project-local config (.promptguard.json)
    let local_config = crate::config::ConfigManager::new(None);
    if let Ok(mgr) = local_config {
        if let Ok(cfg) = mgr.load() {
            if !cfg.api_key.is_empty() {
                return Ok(cfg.api_key);
            }
        }
    }

    // 3. Global credentials (~/.promptguard/credentials.json)
    if let Ok(Some(creds)) = load_credentials() {
        return Ok(creds.api_key);
    }

    Err(PromptGuardError::Config(
        "No API key found. Run 'promptguard login' or set PROMPTGUARD_API_KEY".to_string(),
    ))
}

/// Resolve base URL with precedence: env var > project-local > global > default
pub fn resolve_base_url() -> String {
    if let Ok(url) = std::env::var("PROMPTGUARD_BASE_URL") {
        if !url.is_empty() {
            return url;
        }
    }

    if let Ok(mgr) = crate::config::ConfigManager::new(None) {
        if let Ok(cfg) = mgr.load() {
            return cfg.proxy_url;
        }
    }

    if let Ok(Some(creds)) = load_credentials() {
        if let Some(url) = creds.base_url {
            return url;
        }
    }

    "https://api.promptguard.co/api/v1".to_string()
}
