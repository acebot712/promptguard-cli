use crate::error::{PromptGuardError, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

/// Default `PromptGuard` API base URL.
pub const DEFAULT_BASE_URL: &str = "https://api.promptguard.co/api/v1";

/// Host of [`DEFAULT_BASE_URL`].
const DEFAULT_HOST: &str = "api.promptguard.co";

/// Set by the global `--allow-custom-proxy` CLI flag.
static ALLOW_CUSTOM_PROXY: AtomicBool = AtomicBool::new(false);

/// Opt in to sending an env/global API key to a repo-configured custom proxy
/// host (set from the `--allow-custom-proxy` CLI flag).
pub fn set_allow_custom_proxy(allow: bool) {
    ALLOW_CUSTOM_PROXY.store(allow, Ordering::Relaxed);
}

/// Where a resolved credential or base URL came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialSource {
    /// `PROMPTGUARD_API_KEY` / `PROMPTGUARD_BASE_URL` environment variables
    Env,
    /// Repo-local `.promptguard.json`
    ProjectConfig,
    /// `~/.promptguard/credentials.json`
    Global,
    /// Built-in default (base URL only)
    Default,
}

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
    resolve_api_key_with_source().map(|(key, _)| key)
}

/// Resolve API key and report where it came from.
pub fn resolve_api_key_with_source() -> Result<(String, CredentialSource)> {
    // 1. Environment variable (highest priority)
    if let Ok(key) = std::env::var("PROMPTGUARD_API_KEY") {
        if !key.is_empty() {
            return Ok((key, CredentialSource::Env));
        }
    }

    // 2. Project-local config (.promptguard.json)
    let local_config = crate::config::ConfigManager::new(None);
    if let Ok(mgr) = local_config {
        if let Ok(cfg) = mgr.load() {
            if !cfg.api_key.is_empty() {
                return Ok((cfg.api_key, CredentialSource::ProjectConfig));
            }
        }
    }

    // 3. Global credentials (~/.promptguard/credentials.json)
    if let Ok(Some(creds)) = load_credentials() {
        return Ok((creds.api_key, CredentialSource::Global));
    }

    Err(PromptGuardError::Config(
        "No API key found. Run 'promptguard login' or set PROMPTGUARD_API_KEY".to_string(),
    ))
}

/// Resolve base URL with precedence: env var > project-local > global > default
pub fn resolve_base_url() -> String {
    resolve_base_url_with_source().0
}

/// Resolve base URL and report where it came from.
pub fn resolve_base_url_with_source() -> (String, CredentialSource) {
    if let Ok(url) = std::env::var("PROMPTGUARD_BASE_URL") {
        if !url.is_empty() {
            return (url, CredentialSource::Env);
        }
    }

    if let Ok(mgr) = crate::config::ConfigManager::new(None) {
        if let Ok(cfg) = mgr.load() {
            return (cfg.proxy_url, CredentialSource::ProjectConfig);
        }
    }

    if let Ok(Some(creds)) = load_credentials() {
        if let Some(url) = creds.base_url {
            return (url, CredentialSource::Global);
        }
    }

    (DEFAULT_BASE_URL.to_string(), CredentialSource::Default)
}

/// True when `url_str` points at the default `PromptGuard` host, or at
/// loopback (a proxy on the user's own machine cannot exfiltrate the key).
fn is_trusted_host(url_str: &str) -> bool {
    match url::Url::parse(url_str) {
        Ok(parsed) => match parsed.host_str() {
            Some(host) => {
                host == DEFAULT_HOST || host == "localhost" || host == "127.0.0.1" || host == "::1"
            },
            None => false,
        },
        Err(_) => false,
    }
}

/// Whether resolving credentials must be refused: the API key came from the
/// environment or global credentials, while the base URL was supplied by a
/// repo-local `.promptguard.json` pointing at a non-default host. A cloned
/// malicious repository could otherwise exfiltrate the user's key by simply
/// containing a config file with an attacker-controlled `proxy_url`.
fn is_credential_host_split(
    key_source: CredentialSource,
    url_source: CredentialSource,
    base_url: &str,
    allow_custom_proxy: bool,
) -> bool {
    url_source == CredentialSource::ProjectConfig
        && key_source != CredentialSource::ProjectConfig
        && !is_trusted_host(base_url)
        && !allow_custom_proxy
}

/// Resolve the API key and base URL together, refusing dangerous
/// combinations of credential sources (see [`is_credential_host_split`]).
///
/// Callers that send the API key to the resolved base URL must use this
/// instead of calling [`resolve_api_key`] and [`resolve_base_url`]
/// independently.
pub fn resolve_session() -> Result<(String, String)> {
    let (key, key_source) = resolve_api_key_with_source()?;
    let (base_url, url_source) = resolve_base_url_with_source();

    if is_credential_host_split(
        key_source,
        url_source,
        &base_url,
        ALLOW_CUSTOM_PROXY.load(Ordering::Relaxed),
    ) {
        let key_origin = match key_source {
            CredentialSource::Env => "the PROMPTGUARD_API_KEY environment variable",
            _ => "your global credentials (~/.promptguard/credentials.json)",
        };
        return Err(PromptGuardError::Config(format!(
            "Refusing to send the API key from {key_origin} to '{base_url}', \
             a non-default host configured by this repository's .promptguard.json. \
             A malicious repository could use this to steal your key.\n\
             If you trust this proxy, either set PROMPTGUARD_BASE_URL yourself \
             or re-run with --allow-custom-proxy."
        )));
    }

    Ok((key, base_url))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trusted_hosts() {
        assert!(is_trusted_host(DEFAULT_BASE_URL));
        assert!(is_trusted_host("https://api.promptguard.co/api/v1"));
        assert!(is_trusted_host("http://localhost:8080/api/v1"));
        assert!(is_trusted_host("http://127.0.0.1:3000"));
        assert!(!is_trusted_host("https://evil.example.com/api/v1"));
        assert!(!is_trusted_host("https://api.promptguard.co.evil.com/v1"));
        assert!(!is_trusted_host("not a url"));
    }

    #[test]
    fn split_refused_for_env_key_with_repo_local_custom_host() {
        assert!(is_credential_host_split(
            CredentialSource::Env,
            CredentialSource::ProjectConfig,
            "https://evil.example.com/api/v1",
            false,
        ));
        assert!(is_credential_host_split(
            CredentialSource::Global,
            CredentialSource::ProjectConfig,
            "https://evil.example.com/api/v1",
            false,
        ));
    }

    #[test]
    fn split_allowed_when_key_and_url_share_a_source() {
        assert!(!is_credential_host_split(
            CredentialSource::ProjectConfig,
            CredentialSource::ProjectConfig,
            "https://custom.example.com/api/v1",
            false,
        ));
    }

    #[test]
    fn split_allowed_for_default_host_or_loopback_or_optin() {
        assert!(!is_credential_host_split(
            CredentialSource::Env,
            CredentialSource::ProjectConfig,
            DEFAULT_BASE_URL,
            false,
        ));
        assert!(!is_credential_host_split(
            CredentialSource::Env,
            CredentialSource::ProjectConfig,
            "http://localhost:8080",
            false,
        ));
        assert!(!is_credential_host_split(
            CredentialSource::Env,
            CredentialSource::ProjectConfig,
            "https://custom.example.com/api/v1",
            true,
        ));
        // URL not from repo-local config: user chose it explicitly
        assert!(!is_credential_host_split(
            CredentialSource::Env,
            CredentialSource::Env,
            "https://custom.example.com/api/v1",
            false,
        ));
    }
}
