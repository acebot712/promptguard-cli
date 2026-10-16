//! The local rule pack behind `promptguard scan --offline`.
//!
//! A vendored set of regex rules for prompt injection, jailbreaks, indirect
//! injection and leaked secrets, matched on this machine with no network and
//! no key. It is a local subset: the API also runs ML classifiers and applies
//! the project's policy, and nothing here stands in for that.
//!
//! `pack.json` is generated, never hand-edited. It is the earlier rule set's Go detector
//! tables (Go `regexp`, RE2 syntax, which the `regex` crate accepts as is),
//! dumped by `scripts/local-rules/regenerate.sh`; its `provenance` block names
//! the source files and their hashes, how matching differs from the earlier rule set, and
//! which of the earlier rule set's detectors were left out and why.
//! `tests/local_rules_tests.rs` pins the file and checks the port against what
//! the earlier rule set itself matched on its corpus.

use crate::error::{PromptGuardError, Result};
use regex::{Regex, RegexBuilder};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::OnceLock;

/// The pack as compiled into the binary.
pub const PACK_JSON: &str = include_str!("pack.json");

/// How many characters of a match a finding quotes.
const EXCERPT_CHARS: usize = 100;
/// How many characters of a secret a finding keeps; the rest is never shown.
const SECRET_PREFIX_CHARS: usize = 4;

/// What every offline result says, in human and JSON output alike.
pub fn notice(rule_count: usize) -> String {
    format!(
        "Offline scan: this is a local subset, {rule_count} regex rules run on this machine, \
         not the full detection the PromptGuard API runs. It has no ML classifiers and does \
         not apply your project's policy, so a pass here does not mean the API would allow \
         this content."
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Critical,
    High,
    Medium,
}

impl Severity {
    /// Critical and high rules fail the scan; medium rules only warn. the earlier rule set
    /// draws the same line (block vs warn).
    pub fn blocks(self) -> bool {
        matches!(self, Self::Critical | Self::High)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Critical => "critical",
            Self::High => "high",
            Self::Medium => "medium",
        }
    }
}

#[derive(Deserialize)]
struct PackFile {
    patterns: Vec<RawRule>,
}

#[derive(Deserialize)]
struct RawRule {
    id: String,
    detector: String,
    category: String,
    severity: Severity,
    #[serde(default)]
    min_entropy: f64,
    regex: String,
}

#[derive(Debug)]
pub struct Rule {
    pub id: String,
    pub detector: String,
    pub category: String,
    pub severity: Severity,
    /// A match counts only at or above this Shannon entropy; 0 means always.
    min_entropy: f64,
    regex: Regex,
}

#[derive(Debug, Clone)]
pub struct LocalFinding {
    pub rule_id: String,
    pub detector: String,
    pub category: String,
    pub severity: Severity,
    pub blocking: bool,
    /// 1-based line of the match in the scanned text.
    pub line: usize,
    /// The matched text, truncated; for a secret, only its first characters.
    pub excerpt: String,
}

#[derive(Debug)]
pub struct RulePack {
    rules: Vec<Rule>,
}

/// The bundled pack, compiled once.
pub fn bundled() -> Result<&'static RulePack> {
    static PACK: OnceLock<std::result::Result<RulePack, String>> = OnceLock::new();
    PACK.get_or_init(|| RulePack::from_json(PACK_JSON).map_err(|e| e.to_string()))
        .as_ref()
        .map_err(|e| PromptGuardError::Parse(format!("bundled local rule pack: {e}")))
}

impl RulePack {
    pub fn from_json(json: &str) -> Result<Self> {
        let file: PackFile = serde_json::from_str(json)?;
        let rules = file
            .patterns
            .into_iter()
            .map(compile)
            .collect::<Result<Vec<_>>>()?;
        Ok(Self { rules })
    }

    pub fn rules(&self) -> &[Rule] {
        &self.rules
    }

    /// Every rule that matches `text`, at most one finding per rule (its
    /// first counted match), in pack order.
    pub fn scan(&self, text: &str) -> Vec<LocalFinding> {
        self.rules
            .iter()
            .filter_map(|rule| {
                let found = rule.regex.find_iter(text).find(|m| {
                    rule.min_entropy <= 0.0 || shannon_entropy(m.as_str()) >= rule.min_entropy
                })?;
                Some(LocalFinding {
                    rule_id: rule.id.clone(),
                    detector: rule.detector.clone(),
                    category: rule.category.clone(),
                    severity: rule.severity,
                    blocking: rule.severity.blocks(),
                    line: text[..found.start()].matches('\n').count() + 1,
                    excerpt: excerpt(&rule.detector, found.as_str()),
                })
            })
            .collect()
    }
}

fn compile(raw: RawRule) -> Result<Rule> {
    let mut builder = RegexBuilder::new(&raw.regex);
    match raw.detector.as_str() {
        // the earlier rule set lowercases the input before its injection rules run. Matching
        // case-insensitively is the same thing, except that it also lets the
        // two uppercase `[INST]` rules match, which lowercased input never could.
        "injection" => {
            builder.case_insensitive(true);
        },
        "jailbreak" | "indirect_injection" | "secrets" => {},
        other => {
            return Err(PromptGuardError::Parse(format!(
                "rule {} names unknown detector {other:?}",
                raw.id
            )))
        },
    }
    let regex = builder
        .build()
        .map_err(|e| PromptGuardError::Parse(format!("rule {}: {e}", raw.id)))?;
    Ok(Rule {
        id: raw.id,
        detector: raw.detector,
        category: raw.category,
        severity: raw.severity,
        min_entropy: raw.min_entropy,
        regex,
    })
}

fn excerpt(detector: &str, matched: &str) -> String {
    let (keep, always_elide) = if detector == "secrets" {
        (SECRET_PREFIX_CHARS, true)
    } else {
        (EXCERPT_CHARS, false)
    };
    let mut chars = matched.chars();
    let head: String = chars.by_ref().take(keep).collect();
    if always_elide || chars.next().is_some() {
        format!("{head}…")
    } else {
        head
    }
}

/// Bits per character, computed as the earlier rule set does: character counts over the
/// byte length, which is the same thing for the ASCII values its rules match.
#[allow(clippy::cast_precision_loss)] // counts are far below 2^52
fn shannon_entropy(s: &str) -> f64 {
    if s.is_empty() {
        return 0.0;
    }
    let mut counts: HashMap<char, usize> = HashMap::new();
    for c in s.chars() {
        *counts.entry(c).or_default() += 1;
    }
    let n = s.len() as f64;
    counts
        .values()
        .map(|&c| {
            let p = c as f64 / n;
            -p * p.log2()
        })
        .sum()
}
