use crate::error::{PromptGuardError, Result};
use chrono::{DateTime, Utc};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct APIKey {
    pub id: String,
    pub name: String,
    pub key_prefix: String,
    pub created_at: DateTime<Utc>,
    pub is_active: bool,
    pub last_used_at: Option<DateTime<Utc>>,
    pub project_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityLog {
    pub id: String,
    pub timestamp: DateTime<Utc>,
    pub provider: String,
    pub model: String,
    pub prompt_tokens: i32,
    pub completion_tokens: i32,
    pub total_tokens: i32,
    pub response_time_ms: f64,
    pub status: String,
    pub project_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CreateKeyRequest {
    name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CreateKeyResponse {
    key: String,
    key_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ListKeysResponse {
    keys: Vec<APIKey>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ListLogsResponse {
    logs: Vec<ActivityLog>,
}

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
    pub fn new(api_key: String, base_url: Option<String>) -> Self {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap();

        let base_url = base_url.unwrap_or_else(|| "https://api.promptguard.co".to_string());

        Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key,
        }
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
            .header("User-Agent", format!("promptguard-cli/{}", env!("CARGO_PKG_VERSION")));

        if let Some(body) = body {
            request = request.json(&body);
        }

        let response = request
            .send()
            .map_err(|e| PromptGuardError::Api(format!("Request failed: {}", e)))?;

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
                "API error ({}): {}",
                status, error_text
            )));
        }

        response
            .json()
            .map_err(|e| PromptGuardError::Api(format!("Failed to parse response: {}", e)))
    }

    // API Key Management

    pub fn list_keys(&self) -> Result<Vec<APIKey>> {
        let response: ListKeysResponse = self.request(
            reqwest::Method::GET,
            "/api/v1/api-keys/",
            None,
        )?;

        Ok(response.keys)
    }

    pub fn create_key(&self, name: &str) -> Result<(String, String)> {
        let body = serde_json::json!({ "name": name });
        let response: CreateKeyResponse = self.request(
            reqwest::Method::POST,
            "/api/v1/api-keys/",
            Some(body),
        )?;

        Ok((response.key, response.key_id))
    }

    pub fn revoke_key(&self, key_id: &str) -> Result<()> {
        let _: serde_json::Value = self.request(
            reqwest::Method::DELETE,
            &format!("/api/v1/api-keys/{}", key_id),
            None,
        )?;

        Ok(())
    }

    // Activity Logs

    pub fn get_activity_logs(
        &self,
        limit: Option<usize>,
        provider: Option<&str>,
        status: Option<&str>,
    ) -> Result<Vec<ActivityLog>> {
        let mut params = vec![];

        if let Some(limit) = limit {
            params.push(format!("limit={}", limit));
        }
        if let Some(provider) = provider {
            params.push(format!("provider={}", provider));
        }
        if let Some(status) = status {
            params.push(format!("status={}", status));
        }

        let query = if params.is_empty() {
            String::new()
        } else {
            format!("?{}", params.join("&"))
        };

        let response: ListLogsResponse = self.request(
            reqwest::Method::GET,
            &format!("/dashboard/activity/{}", query),
            None,
        )?;

        Ok(response.logs)
    }

    // Health Check

    pub fn health_check(&self) -> Result<()> {
        let _: serde_json::Value = self.request(
            reqwest::Method::GET,
            "/health",
            None,
        )?;

        Ok(())
    }
}
