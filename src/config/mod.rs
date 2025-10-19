use crate::error::{PromptGuardError, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_applied: Option<DateTime<Utc>>,
    pub cli_version: String,
    #[serde(default)]
    pub files_managed: Vec<String>,
    #[serde(default)]
    pub backups: Vec<String>,
}

impl Default for ConfigMetadata {
    fn default() -> Self {
        Self {
            last_applied: None,
            cli_version: env!("CARGO_PKG_VERSION").to_string(),
            files_managed: Vec::new(),
            backups: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptGuardConfig {
    pub version: String,
    pub api_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    pub proxy_url: String,
    pub providers: Vec<String>,
    #[serde(default = "default_exclude_patterns")]
    pub exclude_patterns: Vec<String>,
    #[serde(default = "default_true")]
    pub backup_enabled: bool,
    #[serde(default = "default_backup_extension")]
    pub backup_extension: String,
    #[serde(default = "default_env_file")]
    pub env_file: String,
    #[serde(default = "default_env_var_name")]
    pub env_var_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub framework: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub runtime_mode: bool,
    #[serde(default)]
    pub metadata: ConfigMetadata,
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
    ]
}

fn default_true() -> bool {
    true
}

fn default_backup_extension() -> String {
    ".bak".to_string()
}

fn default_env_file() -> String {
    ".env".to_string()
}

fn default_env_var_name() -> String {
    "PROMPTGUARD_API_KEY".to_string()
}

impl PromptGuardConfig {
    pub fn new(api_key: String, proxy_url: String, providers: Vec<String>) -> Result<Self> {
        // Validate API key format
        if !api_key.starts_with("pg_sk_test_") && !api_key.starts_with("pg_sk_prod_") {
            return Err(PromptGuardError::InvalidApiKey);
        }

        Ok(Self {
            version: "1.0".to_string(),
            api_key,
            project_id: None,
            proxy_url,
            providers,
            exclude_patterns: default_exclude_patterns(),
            backup_enabled: true,
            backup_extension: ".bak".to_string(),
            env_file: ".env".to_string(),
            env_var_name: "PROMPTGUARD_API_KEY".to_string(),
            framework: None,
            enabled: true,
            runtime_mode: false,
            metadata: ConfigMetadata::default(),
        })
    }
}

pub struct ConfigManager {
    config_path: PathBuf,
}

impl ConfigManager {
    const DEFAULT_CONFIG_FILE: &'static str = ".promptguard.json";

    pub fn new(config_path: Option<PathBuf>) -> Self {
        let path = config_path.unwrap_or_else(|| {
            std::env::current_dir()
                .expect("Failed to get current directory")
                .join(Self::DEFAULT_CONFIG_FILE)
        });

        Self { config_path: path }
    }

    pub fn load(&self) -> Result<PromptGuardConfig> {
        if !self.config_path.exists() {
            return Err(PromptGuardError::NotInitialized);
        }

        let content = fs::read_to_string(&self.config_path)?;
        let config: PromptGuardConfig = serde_json::from_str(&content)
            .map_err(|e| PromptGuardError::Config(format!("Failed to parse config: {e}")))?;

        // Security: Validate paths don't escape project directory
        if config.env_file.contains("..") || config.env_file.starts_with('/') {
            return Err(PromptGuardError::Config(
                "Invalid env_file in config: must be relative path within project".to_string(),
            ));
        }

        Ok(config)
    }

    pub fn save(&self, config: &PromptGuardConfig) -> Result<()> {
        let content = serde_json::to_string_pretty(&config)
            .map_err(|e| PromptGuardError::Config(format!("Failed to serialize config: {e}")))?;

        fs::write(&self.config_path, content)?;

        Ok(())
    }

    pub fn exists(&self) -> bool {
        self.config_path.exists()
    }

    pub fn delete(&self) -> Result<()> {
        if self.config_path.exists() {
            fs::remove_file(&self.config_path)?;
        }
        Ok(())
    }

    pub fn config_path(&self) -> &Path {
        &self.config_path
    }
}
