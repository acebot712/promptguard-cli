use thiserror::Error;

#[derive(Debug)]
#[allow(dead_code)]
pub struct QuotaExceededInfo {
    pub message: String,
    pub code: String,
    pub current_plan: Option<String>,
    pub requests_used: Option<u64>,
    pub requests_limit: Option<u64>,
    pub upgrade_url: Option<String>,
}

// NOTE ON MESSAGE STYLE: the top-level handler in `main` prints the raw
// `Display` of these errors WITHOUT an extra "Error: " prefix (dropping it
// avoids doubled prefixes like "Error: Configuration error: …"). Each
// categorized variant therefore carries its own human-readable category as
// the prefix. `Io` is the deliberate exception: its callers already wrap the
// message with friendly context (e.g. "File not found: <path>"), so it renders
// the message verbatim with no "IO error:" prefix.
#[derive(Error, Debug)]
pub enum PromptGuardError {
    #[error("{0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Parse error: {0}")]
    Parse(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("API error: {0}")]
    Api(String),

    #[error("{}", .0.message)]
    QuotaExceeded(Box<QuotaExceededInfo>),

    #[error("Not initialized. Run 'promptguard init' first")]
    NotInitialized,

    #[error("Invalid API key format. Must start with 'pg_live_'")]
    InvalidApiKey,

    #[error("Authentication failed: {0}")]
    Auth(String),

    #[error("{0}")]
    Custom(String),
}

impl PromptGuardError {
    /// Stable machine-readable error code for `--json` error output.
    ///
    /// Preserves the category distinction (io/config/api/…) that the
    /// human-facing `Display` may fold into free text, so scripts can branch
    /// on `.code` regardless of the message wording.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Io(_) => "io",
            Self::Json(_) => "json",
            Self::Parse(_) => "parse",
            Self::Config(_) => "config",
            Self::Api(_) => "api",
            Self::QuotaExceeded(_) => "quota_exceeded",
            Self::NotInitialized => "not_initialized",
            Self::InvalidApiKey => "invalid_api_key",
            Self::Auth(_) => "auth",
            Self::Custom(_) => "error",
        }
    }
}

pub type Result<T> = std::result::Result<T, PromptGuardError>;
