use crate::error::{PromptGuardError, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// Write a file containing secrets with owner-only permissions (0600).
///
/// On Unix the file is created with mode 0600 (and re-chmodded if it already
/// existed with looser permissions, so there is no window where the secret is
/// world-readable). On other platforms this falls back to a plain write.
pub fn write_private_file(path: &Path, content: &str) -> Result<()> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        use std::os::unix::fs::PermissionsExt;

        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        // mode() only applies at creation; tighten pre-existing files too.
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
        file.write_all(content.as_bytes())?;
    }

    #[cfg(not(unix))]
    {
        fs::write(path, content)?;
    }

    Ok(())
}

/// Validate a `PromptGuard` API key's format.
///
/// Production keys use the canonical `pg_live_` prefix. The check is kept
/// permissive (any `pg_` prefix) to remain forward-compatible with future
/// key classes the backend may introduce.
pub fn is_valid_api_key(key: &str) -> bool {
    // Permissive on prefix (forward-compatible), but reject obviously-malformed
    // input: bare `pg_`, embedded whitespace, or a too-short truncated paste.
    key.starts_with("pg_") && key.len() >= 16 && !key.chars().any(char::is_whitespace)
}

/// True when `url`'s host is a loopback address: IPv4 `127.0.0.0/8`, IPv6
/// `::1`, or the literal `localhost`.
///
/// A proxy on the user's own machine cannot exfiltrate the API key, so
/// loopback hosts are trusted. This must go through [`url::Url::host`]: the
/// `url` crate renders an IPv6 host as the bracketed `"[::1]"` in
/// [`url::Url::host_str`], so a naive `host == "::1"` string compare never
/// matches.
pub fn is_loopback_host(url: &url::Url) -> bool {
    match url.host() {
        Some(url::Host::Ipv4(addr)) => addr.is_loopback(),
        Some(url::Host::Ipv6(addr)) => addr.is_loopback(),
        Some(url::Host::Domain(domain)) => domain.eq_ignore_ascii_case("localhost"),
        None => false,
    }
}

/// Validate a proxy URL for use in config and generated source code.
///
/// The value is interpolated into generated Python/TypeScript shims and
/// transformed source files, so beyond requiring a well-formed HTTPS URL
/// (or localhost HTTP for development), it must not contain quotes,
/// whitespace, control characters, or backslashes that could escape the
/// string literal it is embedded in.
pub fn validate_proxy_url(url_str: &str) -> Result<()> {
    if url_str.chars().any(|c| {
        c.is_whitespace() || c.is_control() || matches!(c, '"' | '\'' | '`' | '\\' | '{' | '}')
    }) {
        return Err(PromptGuardError::Config(
            "Invalid proxy_url: must not contain whitespace, quotes, braces, backslashes, \
             or control characters"
                .to_string(),
        ));
    }

    let parsed = url::Url::parse(url_str)
        .map_err(|e| PromptGuardError::Config(format!("Invalid proxy_url '{url_str}': {e}")))?;

    let is_loopback = is_loopback_host(&parsed);

    match parsed.scheme() {
        "https" => Ok(()),
        "http" if is_loopback => Ok(()),
        _ => Err(PromptGuardError::Config(
            "Invalid proxy_url: must use HTTPS (or http://localhost for development)".to_string(),
        )),
    }
}

/// Validate a backup extension (e.g. `.bak`).
///
/// Must be non-empty, start with a dot, and contain no path separators,
/// whitespace, or further dots. An empty extension used to panic in
/// `BackupManager::backup_path` (`self.backup_extension[1..]`), and a
/// non-dot value silently mangled backup filenames.
pub fn validate_backup_extension(ext: &str) -> Result<()> {
    let rest = ext.strip_prefix('.').ok_or_else(|| {
        PromptGuardError::Config(format!(
            "Invalid backup_extension '{ext}': must start with '.' (e.g. \".bak\")"
        ))
    })?;

    if rest.is_empty() || !rest.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(PromptGuardError::Config(format!(
            "Invalid backup_extension '{ext}': must be '.' followed by alphanumerics \
             (e.g. \".bak\")"
        )));
    }

    Ok(())
}

/// Validate an environment variable name for use in generated source code.
///
/// Strict allowlist (`^[A-Z_][A-Z0-9_]*$`): the value is interpolated into
/// generated Python/TypeScript code, so anything else is rejected.
pub fn validate_env_var_name(name: &str) -> Result<()> {
    let mut chars = name.chars();
    let valid = match chars.next() {
        Some(first) => {
            (first.is_ascii_uppercase() || first == '_')
                && chars.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
        },
        None => false,
    };

    if valid {
        Ok(())
    } else {
        Err(PromptGuardError::Config(format!(
            "Invalid env_var_name '{name}': must match [A-Z_][A-Z0-9_]* \
             (uppercase letters, digits, underscores)"
        )))
    }
}

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
        if !is_valid_api_key(&api_key) {
            return Err(PromptGuardError::InvalidApiKey);
        }

        // proxy_url ends up in generated source code: strict validation
        validate_proxy_url(&proxy_url)?;

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

    pub fn new(config_path: Option<PathBuf>) -> Result<Self> {
        let path = match config_path {
            Some(p) => p,
            None => std::env::current_dir().map_or_else(
                |_| PathBuf::from(Self::DEFAULT_CONFIG_FILE),
                |dir| dir.join(Self::DEFAULT_CONFIG_FILE),
            ),
        };

        Ok(Self { config_path: path })
    }

    /// Supported config versions (for migration compatibility)
    const SUPPORTED_VERSIONS: &'static [&'static str] = &["1.0"];

    pub fn load(&self) -> Result<PromptGuardConfig> {
        if !self.config_path.exists() {
            return Err(PromptGuardError::NotInitialized);
        }

        let content = fs::read_to_string(&self.config_path)?;
        let config: PromptGuardConfig = serde_json::from_str(&content)
            .map_err(|e| PromptGuardError::Config(format!("Failed to parse config: {e}")))?;

        // Validate config version
        if !Self::SUPPORTED_VERSIONS.contains(&config.version.as_str()) {
            return Err(PromptGuardError::Config(format!(
                "Unsupported config version '{}'. Supported versions: {}. \
                 Please run 'promptguard init' to create a new configuration.",
                config.version,
                Self::SUPPORTED_VERSIONS.join(", ")
            )));
        }

        // Security: Validate paths don't escape project directory
        if config.env_file.contains("..") || config.env_file.starts_with('/') {
            return Err(PromptGuardError::Config(
                "Invalid env_file in config: must be relative path within project".to_string(),
            ));
        }

        // Security: proxy_url and env_var_name are interpolated into
        // generated source code — validate with strict allowlists.
        validate_proxy_url(&config.proxy_url)?;
        validate_env_var_name(&config.env_var_name)?;
        validate_backup_extension(&config.backup_extension)?;

        Ok(config)
    }

    pub fn save(&self, config: &PromptGuardConfig) -> Result<()> {
        let content = serde_json::to_string_pretty(&config)
            .map_err(|e| PromptGuardError::Config(format!("Failed to serialize config: {e}")))?;

        // The config contains the API key: owner-only permissions.
        write_private_file(&self.config_path, &content)?;

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

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn proxy_url_accepts_https_and_localhost() {
        assert!(validate_proxy_url("https://api.promptguard.co/api/v1").is_ok());
        assert!(validate_proxy_url("https://proxy.example.com:8443/v1").is_ok());
        assert!(validate_proxy_url("http://localhost:8080/api/v1").is_ok());
        assert!(validate_proxy_url("http://127.0.0.1:3000").is_ok());
        // IPv6 loopback: the url crate renders this host as "[::1]", so a
        // string compare against "::1" would miss it.
        assert!(validate_proxy_url("http://[::1]:8080/api/v1").is_ok());
    }

    #[test]
    fn loopback_host_matches_all_loopback_forms() {
        let loopback = [
            "http://localhost:8080",
            "http://127.0.0.1:3000",
            "http://127.0.0.5",
            "http://[::1]:9000",
            "http://LOCALHOST/x",
        ];
        for url in loopback {
            let parsed = url::Url::parse(url).unwrap();
            assert!(is_loopback_host(&parsed), "{url} should be loopback");
        }

        let remote = ["https://api.promptguard.co", "https://evil.example.com"];
        for url in remote {
            let parsed = url::Url::parse(url).unwrap();
            assert!(!is_loopback_host(&parsed), "{url} should not be loopback");
        }
    }

    #[test]
    fn proxy_url_rejects_injection_and_plain_http() {
        assert!(validate_proxy_url("http://example.com/api").is_err());
        assert!(validate_proxy_url("https://x.com/\"; import os; #").is_err());
        assert!(validate_proxy_url("https://x.com/a b").is_err());
        assert!(validate_proxy_url("https://x.com/`rm -rf`").is_err());
        assert!(validate_proxy_url("https://x.com/\\n").is_err());
        assert!(validate_proxy_url("ftp://x.com").is_err());
        assert!(validate_proxy_url("not a url").is_err());
        assert!(validate_proxy_url("").is_err());
    }

    #[test]
    fn env_var_name_allowlist() {
        assert!(validate_env_var_name("PROMPTGUARD_API_KEY").is_ok());
        assert!(validate_env_var_name("_KEY2").is_ok());
        assert!(validate_env_var_name("").is_err());
        assert!(validate_env_var_name("lowercase").is_err());
        assert!(validate_env_var_name("2STARTS_WITH_DIGIT").is_err());
        assert!(validate_env_var_name("HAS SPACE").is_err());
        assert!(validate_env_var_name("HAS\"QUOTE").is_err());
        assert!(validate_env_var_name("HAS-DASH").is_err());
    }
}
