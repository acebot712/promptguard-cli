use thiserror::Error;

#[derive(Error, Debug)]
pub enum PromptGuardError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Parse error: {0}")]
    Parse(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("API error: {0}")]
    Api(String),

    #[error("Transformation error: {0}")]
    Transform(String),

    #[error("Not initialized. Run 'promptguard init' first")]
    NotInitialized,

    #[error("Invalid API key format. Must start with 'pg_sk_test_' or 'pg_sk_prod_'")]
    InvalidApiKey,

    #[error("{0}")]
    Custom(String),
}

pub type Result<T> = std::result::Result<T, PromptGuardError>;
