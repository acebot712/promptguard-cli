use crate::api::PromptGuardClient;
use crate::config::ConfigManager;
use crate::detector::detect_all_providers;
use crate::error::{PromptGuardError, Result};
use crate::scanner::FileScanner;
use crate::types::{DetectionInstance, Provider};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// JSON-RPC 2.0 types (MCP transport layer)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    #[allow(dead_code)]
    jsonrpc: String,
    id: Option<serde_json::Value>,
    method: String,
    #[serde(default)]
    params: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    id: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
struct JsonRpcError {
    code: i64,
    message: String,
}

impl JsonRpcResponse {
    fn success(id: serde_json::Value, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    fn error(id: serde_json::Value, code: i64, message: String) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(JsonRpcError { code, message }),
        }
    }
}

// ---------------------------------------------------------------------------
// MCP tool definitions
// ---------------------------------------------------------------------------

fn tool_definitions() -> serde_json::Value {
    serde_json::json!({
        "tools": [
            {
                "name": "promptguard_auth",
                "description": "Authenticate with PromptGuard. When called without an api_key, opens the PromptGuard dashboard in the browser so the user can copy their API key -- then ask the user to provide it. When called with an api_key, validates and saves it.\nWhen to use: When any other promptguard tool reports that the user is not authenticated or not initialized, or when the user asks to log in to PromptGuard.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "api_key": {
                            "type": "string",
                            "description": "PromptGuard API key (starts with pg_sk_test_ or pg_sk_prod_). If omitted, opens the dashboard in the browser for the user to copy their key."
                        }
                    }
                }
            },
            {
                "name": "promptguard_logout",
                "description": "Log out of PromptGuard by removing the locally stored API key and configuration.\nWhen to use: When the user wants to switch PromptGuard accounts, clear credentials, or ensure a clean state by removing existing authentication.",
                "inputSchema": {
                    "type": "object",
                    "properties": {}
                }
            },
            {
                "name": "promptguard_scan_text",
                "description": "Scan text for security threats (prompt injection, jailbreaks, PII leakage, toxic content) via the PromptGuard API. Returns a decision (allow/block), confidence score, threat type, and reason.\nWhen to use: When reviewing user-facing prompts, testing LLM inputs for safety, or verifying that content is free of injection attacks before sending to an LLM.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "text": {
                            "type": "string",
                            "description": "The text content to scan for security threats"
                        }
                    },
                    "required": ["text"]
                }
            },
            {
                "name": "promptguard_scan_project",
                "description": "Performs static analysis on a project directory to detect unprotected LLM SDK usage (OpenAI, Anthropic, Cohere, Gemini, Bedrock, etc.). Returns detected providers, file locations (line/column), and instance count.\nWhen to use: During local development to find LLM calls that are not routed through PromptGuard, after generating new code that uses LLM SDKs, or when auditing an existing project for security coverage.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "directory": {
                            "type": "string",
                            "description": "Absolute path to the project directory to scan (defaults to current directory)"
                        },
                        "provider": {
                            "type": "string",
                            "description": "Filter results to a specific provider (e.g. 'openai', 'anthropic')"
                        }
                    }
                }
            },
            {
                "name": "promptguard_redact",
                "description": "Redact PII (emails, phone numbers, SSNs, credit cards, API keys, etc.) from text via the PromptGuard API. Returns sanitized text with entities replaced by placeholders.\nWhen to use: Before including user data in LLM prompts, when processing customer messages, or when building RAG pipelines that ingest documents containing personal information.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "text": {
                            "type": "string",
                            "description": "The text content to redact PII from"
                        }
                    },
                    "required": ["text"]
                }
            },
            {
                "name": "promptguard_status",
                "description": "Show current PromptGuard configuration and status for the project. Returns whether PromptGuard is initialized, API key type, proxy URL, configured providers, and CLI version.\nWhen to use: To verify PromptGuard is properly configured, check which providers are active, or confirm the installed CLI version.",
                "inputSchema": {
                    "type": "object",
                    "properties": {}
                }
            }
        ]
    })
}

// ---------------------------------------------------------------------------
// Tool handlers
// ---------------------------------------------------------------------------

fn resolve_project_id(config: &crate::config::PromptGuardConfig) -> Option<String> {
    if let Some(ref pid) = config.project_id {
        if !pid.is_empty() {
            return Some(pid.clone());
        }
    }
    if let Ok(Some(creds)) = crate::auth::load_credentials() {
        return creds.active_project;
    }
    None
}

fn handle_scan_text(params: &serde_json::Value) -> serde_json::Value {
    let text = match params.get("text").and_then(serde_json::Value::as_str) {
        Some(t) => t.to_string(),
        None => {
            return serde_json::json!({
                "content": [{"type": "text", "text": "Error: 'text' parameter is required"}],
                "isError": true
            });
        },
    };

    let result = (|| -> Result<serde_json::Value> {
        let config_manager = ConfigManager::new(None)?;
        let config = config_manager.load()?;
        let client =
            PromptGuardClient::new(config.api_key.clone(), Some(config.proxy_url.clone()))?;

        let mut body = serde_json::json!({ "content": text, "type": "prompt" });
        if let Some(pid) = resolve_project_id(&config) {
            body["project_id"] = serde_json::Value::String(pid);
        }

        let response: serde_json::Value = client.post("/security/scan", &body)?;

        Ok(response)
    })();

    match result {
        Ok(response) => serde_json::json!({
            "content": [{"type": "text", "text": serde_json::to_string_pretty(&response).unwrap_or_default()}]
        }),
        Err(e) => serde_json::json!({
            "content": [{"type": "text", "text": format!("Error: {e}")}],
            "isError": true
        }),
    }
}

fn handle_scan_project(params: &serde_json::Value) -> serde_json::Value {
    let dir = params
        .get("directory")
        .and_then(serde_json::Value::as_str)
        .unwrap_or(".");
    let provider_filter = params
        .get("provider")
        .and_then(serde_json::Value::as_str)
        .map(std::string::ToString::to_string);

    let root_path = PathBuf::from(dir);
    let scanner = match FileScanner::new(&root_path, None) {
        Ok(s) => s,
        Err(e) => {
            return serde_json::json!({
                "content": [{"type": "text", "text": format!("Error creating scanner: {e}")}],
                "isError": true
            });
        },
    };
    let files = match scanner.scan_files(None) {
        Ok(f) => f,
        Err(e) => {
            return serde_json::json!({
                "content": [{"type": "text", "text": format!("Error scanning files: {e}")}],
                "isError": true
            });
        },
    };

    let mut all_detections: HashMap<Provider, Vec<DetectionInstance>> = HashMap::new();

    for file_path in &files {
        if let Ok(results) = detect_all_providers(file_path) {
            for (provider, result) in results {
                if let Some(ref filter) = provider_filter {
                    if provider.as_str() != filter {
                        continue;
                    }
                }
                if !result.instances.is_empty() {
                    all_detections
                        .entry(provider)
                        .or_default()
                        .extend(result.instances);
                }
            }
        }
    }

    let mut results = Vec::new();
    for (provider, instances) in &all_detections {
        let instance_data: Vec<serde_json::Value> = instances
            .iter()
            .map(|inst| {
                serde_json::json!({
                    "file": inst.file_path.strip_prefix(&root_path)
                        .unwrap_or(&inst.file_path)
                        .to_string_lossy(),
                    "line": inst.line,
                    "column": inst.column,
                    "has_base_url": inst.has_base_url,
                })
            })
            .collect();

        results.push(serde_json::json!({
            "provider": provider.as_str(),
            "count": instances.len(),
            "instances": instance_data,
        }));
    }

    let summary = if results.is_empty() {
        "No LLM SDK usage detected in this project.".to_string()
    } else {
        let total: usize = all_detections.values().map(Vec::len).sum();
        format!(
            "Found {} LLM SDK usage(s) across {} provider(s) in {} files scanned.",
            total,
            all_detections.len(),
            files.len()
        )
    };

    serde_json::json!({
        "content": [{
            "type": "text",
            "text": serde_json::to_string_pretty(&serde_json::json!({
                "summary": summary,
                "files_scanned": files.len(),
                "providers": results,
            })).unwrap_or_default()
        }]
    })
}

fn handle_redact(params: &serde_json::Value) -> serde_json::Value {
    let text = match params.get("text").and_then(serde_json::Value::as_str) {
        Some(t) => t.to_string(),
        None => {
            return serde_json::json!({
                "content": [{"type": "text", "text": "Error: 'text' parameter is required"}],
                "isError": true
            });
        },
    };

    let result = (|| -> Result<serde_json::Value> {
        let config_manager = ConfigManager::new(None)?;
        let config = config_manager.load()?;
        let client =
            PromptGuardClient::new(config.api_key.clone(), Some(config.proxy_url.clone()))?;

        let mut body = serde_json::json!({ "content": text });
        if let Some(pid) = resolve_project_id(&config) {
            body["project_id"] = serde_json::Value::String(pid);
        }

        let response: serde_json::Value = client.post("/security/redact", &body)?;

        Ok(response)
    })();

    match result {
        Ok(response) => serde_json::json!({
            "content": [{"type": "text", "text": serde_json::to_string_pretty(&response).unwrap_or_default()}]
        }),
        Err(e) => serde_json::json!({
            "content": [{"type": "text", "text": format!("Error: {e}")}],
            "isError": true
        }),
    }
}

fn handle_status(_params: &serde_json::Value) -> serde_json::Value {
    let result = (|| -> Result<serde_json::Value> {
        let config_manager = ConfigManager::new(None)?;
        let config = config_manager.load()?;

        let key_type = if config.api_key.starts_with("pg_sk_test_") {
            "test"
        } else if config.api_key.starts_with("pg_sk_prod_") {
            "production"
        } else {
            "unknown"
        };

        Ok(serde_json::json!({
            "initialized": true,
            "api_key_type": key_type,
            "proxy_url": config.proxy_url,
            "providers": config.providers,
            "version": env!("CARGO_PKG_VERSION"),
        }))
    })();

    match result {
        Ok(info) => serde_json::json!({
            "content": [{"type": "text", "text": serde_json::to_string_pretty(&info).unwrap_or_default()}]
        }),
        Err(_) => serde_json::json!({
            "content": [{"type": "text", "text": serde_json::to_string_pretty(&serde_json::json!({
                "initialized": false,
                "message": "PromptGuard is not configured. Run 'promptguard init' first.",
                "version": env!("CARGO_PKG_VERSION"),
            })).unwrap_or_default()}]
        }),
    }
}

fn handle_auth(params: &serde_json::Value) -> serde_json::Value {
    let api_key = params.get("api_key").and_then(serde_json::Value::as_str);

    match api_key {
        None => {
            // No key provided -- open dashboard so user can grab one
            let url = "https://app.promptguard.co/settings/api-keys";
            let _ = open::that(url);

            serde_json::json!({
                "content": [{
                    "type": "text",
                    "text": "Opened the PromptGuard dashboard in your browser. Please copy your API key (starts with pg_sk_test_ or pg_sk_prod_) and provide it here so I can save it."
                }]
            })
        },
        Some(key) => {
            if !key.starts_with("pg_sk_test_") && !key.starts_with("pg_sk_prod_") {
                return serde_json::json!({
                    "content": [{"type": "text", "text": "Invalid API key format. Keys must start with 'pg_sk_test_' or 'pg_sk_prod_'."}],
                    "isError": true
                });
            }

            let result = (|| -> Result<()> {
                let config_manager = ConfigManager::new(None)?;

                if config_manager.exists() {
                    let mut config = config_manager.load()?;
                    config.api_key = key.to_string();
                    config_manager.save(&config)?;
                } else {
                    let config = crate::config::PromptGuardConfig::new(
                        key.to_string(),
                        "https://api.promptguard.co/api/v1".to_string(),
                        Vec::new(),
                    )?;
                    config_manager.save(&config)?;
                }
                Ok(())
            })();

            match result {
                Ok(()) => {
                    let key_type = if key.starts_with("pg_sk_test_") {
                        "test"
                    } else {
                        "production"
                    };
                    serde_json::json!({
                        "content": [{"type": "text", "text": format!("Authenticated successfully with a {key_type} API key. PromptGuard is ready to use.\n\nTo associate requests with a specific project, set \"project_id\" in .promptguard.json or run 'promptguard projects select <id>'.")}]
                    })
                },
                Err(e) => serde_json::json!({
                    "content": [{"type": "text", "text": format!("Failed to save API key: {e}")}],
                    "isError": true
                }),
            }
        },
    }
}

fn handle_logout(_params: &serde_json::Value) -> serde_json::Value {
    let result = (|| -> Result<()> {
        let config_manager = ConfigManager::new(None)?;
        config_manager.delete()?;
        Ok(())
    })();

    match result {
        Ok(()) => serde_json::json!({
            "content": [{"type": "text", "text": "Logged out. Local PromptGuard configuration and API key have been removed."}]
        }),
        Err(e) => serde_json::json!({
            "content": [{"type": "text", "text": format!("Logout failed: {e}")}],
            "isError": true
        }),
    }
}

// ---------------------------------------------------------------------------
// Request dispatch
// ---------------------------------------------------------------------------

fn handle_request(request: &JsonRpcRequest) -> JsonRpcResponse {
    let id = request.id.clone().unwrap_or(serde_json::Value::Null);

    match request.method.as_str() {
        "initialize" => JsonRpcResponse::success(
            id,
            serde_json::json!({
                "protocolVersion": "2025-03-26",
                "capabilities": {
                    "tools": { "listChanged": false },
                    "logging": {}
                },
                "serverInfo": {
                    "name": "promptguard",
                    "version": env!("CARGO_PKG_VERSION"),
                }
            }),
        ),

        "notifications/initialized"
        | "notifications/cancelled"
        | "notifications/roots/list_changed" => {
            if request.id.is_none() {
                return JsonRpcResponse::success(serde_json::Value::Null, serde_json::json!(null));
            }
            JsonRpcResponse::success(id, serde_json::json!(null))
        },

        "ping" => JsonRpcResponse::success(id, serde_json::json!({})),

        "tools/list" => JsonRpcResponse::success(id, tool_definitions()),

        "tools/call" => {
            let tool_name = request
                .params
                .get("name")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            let arguments = request
                .params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| serde_json::json!({}));

            let result = match tool_name {
                "promptguard_scan_text" => handle_scan_text(&arguments),
                "promptguard_scan_project" => handle_scan_project(&arguments),
                "promptguard_redact" => handle_redact(&arguments),
                "promptguard_status" => handle_status(&arguments),
                "promptguard_auth" => handle_auth(&arguments),
                "promptguard_logout" => handle_logout(&arguments),
                _ => serde_json::json!({
                    "content": [{"type": "text", "text": format!("Unknown tool: {tool_name}")}],
                    "isError": true
                }),
            };

            JsonRpcResponse::success(id, result)
        },

        _ => JsonRpcResponse::error(id, -32601, format!("Method not found: {}", request.method)),
    }
}

// ---------------------------------------------------------------------------
// MCP command entry point
// ---------------------------------------------------------------------------

pub struct McpCommand {
    pub transport: String,
}

impl McpCommand {
    pub fn execute(&self) -> Result<()> {
        if self.transport != "stdio" {
            return Err(PromptGuardError::Custom(format!(
                "Unsupported transport '{}'. Only 'stdio' is supported.",
                self.transport
            )));
        }

        let stdin = io::stdin();
        let mut stdout = io::stdout();

        for line in stdin.lock().lines() {
            let line = line.map_err(PromptGuardError::Io)?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            let request: JsonRpcRequest = match serde_json::from_str(trimmed) {
                Ok(r) => r,
                Err(e) => {
                    let err_response = JsonRpcResponse::error(
                        serde_json::Value::Null,
                        -32700,
                        format!("Parse error: {e}"),
                    );
                    let out = serde_json::to_string(&err_response).unwrap_or_default();
                    writeln!(stdout, "{out}").map_err(|e| {
                        PromptGuardError::Io(io::Error::new(io::ErrorKind::BrokenPipe, e))
                    })?;
                    stdout.flush().map_err(PromptGuardError::Io)?;
                    continue;
                },
            };

            // Notifications have no id and expect no response
            if request.id.is_none() {
                continue;
            }

            let response = handle_request(&request);
            let out = serde_json::to_string(&response).unwrap_or_default();
            writeln!(stdout, "{out}")
                .map_err(|e| PromptGuardError::Io(io::Error::new(io::ErrorKind::BrokenPipe, e)))?;
            stdout.flush().map_err(PromptGuardError::Io)?;
        }

        Ok(())
    }
}
