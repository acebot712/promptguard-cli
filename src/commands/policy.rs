//! Policy-as-Code Command
//!
//! Define guardrails in YAML, version in git, apply via CLI.
//! Maps to the existing guardrail config API - a YAML front-end,
//! not a new config system.

use crate::api::PromptGuardClient;
use crate::config::ConfigManager;
use crate::error::{PromptGuardError, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;

/// Wrapper for the guardrails API response.
#[derive(Debug, Deserialize)]
struct GuardrailsResponse {
    guardrails: serde_json::Value,
}

/// Wrapper for the guardrails API update request.
#[derive(Debug, Serialize)]
struct GuardrailsUpdateRequest {
    guardrails: serde_json::Value,
}

const VALID_LEVELS: &[&str] = &["strict", "moderate", "permissive"];
const VALID_PII_MODES: &[&str] = &["redact", "mask", "block"];

pub enum PolicyAction {
    Apply { file: String, dry_run: bool },
    Diff { file: String },
    Export,
}

pub struct PolicyCommand {
    pub action: PolicyAction,
    pub project_id: String,
    pub api_key: Option<String>,
    pub base_url: Option<String>,
}

impl PolicyCommand {
    pub fn execute(self) -> Result<()> {
        let api_key = if let Some(key) = &self.api_key {
            key.clone()
        } else {
            ConfigManager::new(None)
                .ok()
                .and_then(|cm| cm.load().ok())
                .map(|c| c.api_key)
                .ok_or_else(|| {
                    PromptGuardError::Config(
                        "API key required. Run 'promptguard init' or pass --api-key".to_string(),
                    )
                })?
        };

        let client = PromptGuardClient::new(api_key, self.base_url.clone())
            .map_err(|e| PromptGuardError::Config(format!("Failed to create client: {e}")))?;

        match self.action {
            PolicyAction::Apply { ref file, dry_run } => self.apply(&client, file, dry_run),
            PolicyAction::Diff { ref file } => self.diff(&client, file),
            PolicyAction::Export => self.export(&client),
        }
    }

    fn load_yaml(path: &str) -> Result<serde_json::Value> {
        let content = fs::read_to_string(path)
            .map_err(|e| PromptGuardError::Config(format!("Failed to read {path}: {e}")))?;

        let parsed: serde_yaml::Value = serde_yaml::from_str(&content)
            .map_err(|e| PromptGuardError::Config(format!("YAML parse error: {e}")))?;

        let json_value = serde_json::to_value(&parsed).map_err(|e| {
            PromptGuardError::Config(format!("Failed to convert YAML to JSON: {e}"))
        })?;

        let guardrails = json_value
            .get("guardrails")
            .ok_or_else(|| {
                PromptGuardError::Config("YAML must have a top-level 'guardrails' key".to_string())
            })?
            .clone();

        Self::validate(&guardrails)?;
        Ok(guardrails)
    }

    fn validate(guardrails: &serde_json::Value) -> Result<()> {
        let mut errors: Vec<String> = Vec::new();

        let level_fields = [
            "prompt_injection",
            "data_exfiltration",
            "secret_key_detection",
        ];

        for field in &level_fields {
            if let Some(cfg) = guardrails.get(field) {
                if let Some(level) = cfg.get("level").and_then(|v| v.as_str()) {
                    if !VALID_LEVELS.contains(&level) {
                        errors.push(format!(
                            "guardrails.{field}.level: must be one of {VALID_LEVELS:?}"
                        ));
                    }
                }
            }
        }

        if let Some(pii) = guardrails.get("pii_detection") {
            if let Some(level) = pii.get("level").and_then(|v| v.as_str()) {
                if !VALID_LEVELS.contains(&level) {
                    errors.push(format!(
                        "guardrails.pii_detection.level: must be one of {VALID_LEVELS:?}"
                    ));
                }
            }
            if let Some(mode) = pii.get("mode").and_then(|v| v.as_str()) {
                if !VALID_PII_MODES.contains(&mode) {
                    errors.push(format!(
                        "guardrails.pii_detection.mode: must be one of {VALID_PII_MODES:?}"
                    ));
                }
            }
        }

        if let Some(toxicity) = guardrails.get("toxicity") {
            if let Some(t) = toxicity.get("threshold") {
                if let Some(val) = t.as_f64() {
                    if !(0.0..=1.0).contains(&val) {
                        errors.push("guardrails.toxicity.threshold: must be 0.0-1.0".to_string());
                    }
                }
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(PromptGuardError::Config(format!(
                "Policy validation failed:\n  {}",
                errors.join("\n  ")
            )))
        }
    }

    fn fetch_current(&self, client: &PromptGuardClient) -> Result<serde_json::Value> {
        let endpoint = format!("/projects/{}/guardrails", self.project_id);
        let resp: GuardrailsResponse = client.get(&endpoint)?;
        Ok(resp.guardrails)
    }

    fn compute_diff(
        current: &serde_json::Value,
        desired: &serde_json::Value,
    ) -> Vec<(String, String, String)> {
        let mut diffs: Vec<(String, String, String)> = Vec::new();

        let cur_map = current.as_object().cloned().unwrap_or_default();
        let des_map = desired.as_object().cloned().unwrap_or_default();

        let mut all_keys: BTreeSet<String> = BTreeSet::new();
        for k in cur_map.keys().chain(des_map.keys()) {
            all_keys.insert(k.clone());
        }

        for key in &all_keys {
            let old = cur_map.get(key);
            let new = des_map.get(key);

            if old == new {
                continue;
            }

            match (
                old.and_then(|v| v.as_object()),
                new.and_then(|v| v.as_object()),
            ) {
                (Some(old_obj), Some(new_obj)) => {
                    let mut sub_keys: BTreeSet<String> = BTreeSet::new();
                    for k in old_obj.keys().chain(new_obj.keys()) {
                        sub_keys.insert(k.clone());
                    }
                    for sk in &sub_keys {
                        let ov = old_obj.get(sk);
                        let nv = new_obj.get(sk);
                        if ov != nv {
                            diffs.push((
                                format!("{key}.{sk}"),
                                ov.map_or("(none)".to_string(), std::string::ToString::to_string),
                                nv.map_or("(none)".to_string(), std::string::ToString::to_string),
                            ));
                        }
                    }
                },
                _ => {
                    diffs.push((
                        key.clone(),
                        old.map_or("(none)".to_string(), std::string::ToString::to_string),
                        new.map_or("(none)".to_string(), std::string::ToString::to_string),
                    ));
                },
            }
        }

        diffs
    }

    fn apply(&self, client: &PromptGuardClient, file: &str, dry_run: bool) -> Result<()> {
        println!("📋 Loading policy from {file}...\n");

        let desired = Self::load_yaml(file)?;
        println!("✅ Policy validated successfully\n");

        let current = self.fetch_current(client)?;
        let diffs = Self::compute_diff(&current, &desired);

        if diffs.is_empty() {
            println!("No changes - policy already matches.");
            return Ok(());
        }

        println!("Changes to apply:\n");
        for (field, old, new) in &diffs {
            println!("  {field}: {old} -> {new}");
        }
        println!();

        if dry_run {
            println!("(dry-run) No changes applied.");
            return Ok(());
        }

        let endpoint = format!("/projects/{}/guardrails", self.project_id);
        let _: serde_json::Value = client.put(
            &endpoint,
            &GuardrailsUpdateRequest {
                guardrails: desired,
            },
        )?;

        println!("✅ Policy applied successfully.");
        Ok(())
    }

    fn diff(&self, client: &PromptGuardClient, file: &str) -> Result<()> {
        println!("📋 Comparing {file} against live config...\n");

        let desired = Self::load_yaml(file)?;
        let current = self.fetch_current(client)?;
        let diffs = Self::compute_diff(&current, &desired);

        if diffs.is_empty() {
            println!("No differences - policy matches live config.");
        } else {
            println!("Differences:\n");
            for (field, old, new) in &diffs {
                println!("  {field}:");
                println!("    - {old}");
                println!("    + {new}");
            }
        }

        Ok(())
    }

    fn export(&self, client: &PromptGuardClient) -> Result<()> {
        let current = self.fetch_current(client)?;

        let wrapper = serde_json::json!({ "guardrails": current });
        let yaml_value: serde_yaml::Value = serde_json::from_value(wrapper)
            .map_err(|e| PromptGuardError::Config(format!("Failed to convert config: {e}")))?;
        let yaml_str = serde_yaml::to_string(&yaml_value)
            .map_err(|e| PromptGuardError::Config(format!("Failed to serialize YAML: {e}")))?;

        print!("{yaml_str}");
        Ok(())
    }
}
