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
                "name": "promptguard_scan_text",
                "description": "Scan text for security threats (prompt injection, jailbreaks, PII leakage, toxic content) via the PromptGuard API. Returns a decision (allow/block), confidence score, threat type, and reason.",
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
                "description": "Scan a project directory for unprotected LLM SDK usage (OpenAI, Anthropic, Cohere, Gemini, Bedrock, etc.). Returns detected providers, file locations, and whether PromptGuard is configured.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "directory": {
                            "type": "string",
                            "description": "Path to the project directory to scan (defaults to current directory)"
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
                "description": "Redact PII (emails, phone numbers, SSNs, credit cards, API keys, etc.) from text via the PromptGuard API. Returns sanitized text with entities replaced by placeholders.",
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
                "description": "Show current PromptGuard configuration and status for the project. Returns whether PromptGuard is initialized, active, and which providers are configured.",
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
        let client = PromptGuardClient::new(config.api_key, Some(config.proxy_url))?;

        let response: serde_json::Value =
            client.post("/security/scan", &serde_json::json!({ "text": text }))?;

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
        let client = PromptGuardClient::new(config.api_key, Some(config.proxy_url))?;

        let response: serde_json::Value =
            client.post("/security/redact", &serde_json::json!({ "text": text }))?;

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

// ---------------------------------------------------------------------------
// Request dispatch
// ---------------------------------------------------------------------------

fn handle_request(request: &JsonRpcRequest) -> JsonRpcResponse {
    let id = request.id.clone().unwrap_or(serde_json::Value::Null);

    match request.method.as_str() {
        "initialize" => JsonRpcResponse::success(
            id,
            serde_json::json!({
                "protocolVersion": "2024-11-05",
                "capabilities": { "tools": {} },
                "serverInfo": {
                    "name": "promptguard",
                    "version": env!("CARGO_PKG_VERSION"),
                }
            }),
        ),

        "notifications/initialized" | "notifications/cancelled" => {
            JsonRpcResponse::success(id, serde_json::json!(null))
        },

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
