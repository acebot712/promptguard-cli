use crate::error::{PromptGuardError, Result};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use std::thread;
use std::time::Duration;

/// Maximum number of retry attempts for transient failures
const MAX_RETRIES: u32 = 3;

/// Base delay for exponential backoff (100ms)
const RETRY_BASE_DELAY_MS: u64 = 100;

/// Connection timeout in seconds
const CONNECT_TIMEOUT_SECS: u64 = 10;

/// Request timeout in seconds
const REQUEST_TIMEOUT_SECS: u64 = 30;

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
            .connect_timeout(Duration::from_secs(CONNECT_TIMEOUT_SECS))
            .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
            .build()
            .map_err(|e| PromptGuardError::Api(format!("Failed to build HTTP client: {e}")))?;

        let base_url = base_url.unwrap_or_else(|| "https://api.promptguard.co/api/v1".to_string());

        Ok(Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key,
        })
    }

    /// Check if an error is retryable (transient network issues, server errors)
    fn is_retryable_error(error: &reqwest::Error) -> bool {
        error.is_timeout() || error.is_connect() || error.is_request()
    }

    /// Check if an HTTP status code is retryable
    fn is_retryable_status(status: reqwest::StatusCode) -> bool {
        // Retry on 429 (rate limit), 502, 503, 504 (server issues)
        status == reqwest::StatusCode::TOO_MANY_REQUESTS
            || status == reqwest::StatusCode::BAD_GATEWAY
            || status == reqwest::StatusCode::SERVICE_UNAVAILABLE
            || status == reqwest::StatusCode::GATEWAY_TIMEOUT
    }

    fn request<T: serde::de::DeserializeOwned>(
        &self,
        method: reqwest::Method,
        endpoint: &str,
        body: Option<serde_json::Value>,
    ) -> Result<T> {
        let url = format!("{}{}", self.base_url, endpoint);
        let mut last_error: Option<PromptGuardError> = None;

        for attempt in 0..=MAX_RETRIES {
            if attempt > 0 {
                // Exponential backoff: 100ms, 200ms, 400ms
                let delay_ms = RETRY_BASE_DELAY_MS * (1 << (attempt - 1));
                thread::sleep(Duration::from_millis(delay_ms));
            }

            let mut request = self
                .client
                .request(method.clone(), &url)
                .header("X-API-Key", &self.api_key)
                .header(
                    "User-Agent",
                    format!("promptguard-cli/{}", env!("CARGO_PKG_VERSION")),
                );

            if let Some(ref body) = body {
                request = request.json(body);
            }

            match request.send() {
                Ok(response) => {
                    let status = response.status();

                    if status.is_success() {
                        return response.json().map_err(|e| {
                            PromptGuardError::Api(format!("Failed to parse response: {e}"))
                        });
                    }

                    // Check if we should retry this status code
                    if Self::is_retryable_status(status) && attempt < MAX_RETRIES {
                        last_error = Some(PromptGuardError::Api(format!(
                            "Server returned {status}, retrying..."
                        )));
                        continue;
                    }

                    // Non-retryable error or out of retries
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
                },
                Err(e) => {
                    if Self::is_retryable_error(&e) && attempt < MAX_RETRIES {
                        last_error = Some(PromptGuardError::Api(format!(
                            "Request failed: {e}, retrying..."
                        )));
                        continue;
                    }
                    return Err(PromptGuardError::Api(format!("Request failed: {e}")));
                },
            }
        }

        // All retries exhausted
        Err(last_error.unwrap_or_else(|| {
            PromptGuardError::Api("Request failed after all retries".to_string())
        }))
    }

    // Health Check

    pub fn health_check(&self) -> Result<()> {
        let _: serde_json::Value = self.request(reqwest::Method::GET, "/health", None)?;

        Ok(())
    }

    /// GET request helper (public API for future use)
    #[allow(dead_code)]
    pub fn get<T: serde::de::DeserializeOwned>(&self, endpoint: &str) -> Result<T> {
        self.request(reqwest::Method::GET, endpoint, None)
    }

    /// POST request helper (public API for future use)
    #[allow(dead_code)]
    pub fn post<T: serde::de::DeserializeOwned, B: serde::Serialize>(
        &self,
        endpoint: &str,
        body: &B,
    ) -> Result<T> {
        self.request(
            reqwest::Method::POST,
            endpoint,
            Some(
                serde_json::to_value(body)
                    .map_err(|e| PromptGuardError::Api(format!("Failed to serialize body: {e}")))?,
            ),
        )
    }

    /// DELETE request helper (public API for future use)
    #[allow(dead_code)]
    pub fn delete<T: serde::de::DeserializeOwned>(&self, endpoint: &str) -> Result<T> {
        self.request(reqwest::Method::DELETE, endpoint, None)
    }
}
