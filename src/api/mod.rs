use crate::error::{PromptGuardError, Result};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ErrorResponse {
    error: ErrorDetail,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ErrorDetail {
    code: String,
    message: String,
}

pub struct PromptGuardClient {
    client: Client,
    base_url: String,
    api_key: String,
}

impl PromptGuardClient {
    pub fn new(api_key: String, base_url: Option<String>) -> Result<Self> {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| PromptGuardError::Api(format!("Failed to build HTTP client: {}", e)))?;

        let base_url = base_url.unwrap_or_else(|| "https://api.promptguard.co".to_string());

        Ok(Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key,
        })
    }

    fn request<T: serde::de::DeserializeOwned>(
        &self,
        method: reqwest::Method,
        endpoint: &str,
        body: Option<serde_json::Value>,
    ) -> Result<T> {
        let url = format!("{}{}", self.base_url, endpoint);

        let mut request = self
            .client
            .request(method, &url)
            .header("X-API-Key", &self.api_key)
            .header(
                "User-Agent",
                format!("promptguard-cli/{}", env!("CARGO_PKG_VERSION")),
            );

        if let Some(body) = body {
            request = request.json(&body);
        }

        let response = request
            .send()
            .map_err(|e| PromptGuardError::Api(format!("Request failed: {e}")))?;

        let status = response.status();

        if !status.is_success() {
            let error_text = response
                .text()
                .unwrap_or_else(|_| "Unknown error".to_string());

            if let Ok(error_response) = serde_json::from_str::<ErrorResponse>(&error_text) {
                return Err(PromptGuardError::Api(format!(
                    "API error ({}): {}",
                    status, error_response.error.message
                )));
            }

            return Err(PromptGuardError::Api(format!(
                "API error ({status}): {error_text}"
            )));
        }

        response
            .json()
            .map_err(|e| PromptGuardError::Api(format!("Failed to parse response: {e}")))
    }

    // Health Check

    pub fn health_check(&self) -> Result<()> {
        let _: serde_json::Value = self.request(reqwest::Method::GET, "/health", None)?;

        Ok(())
    }
}
