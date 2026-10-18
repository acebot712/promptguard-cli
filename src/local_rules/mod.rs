//! The local rule pack behind `promptguard scan --offline`.
//!
//! A vendored set of regex rules for prompt injection, jailbreaks, indirect
//! injection and leaked secrets, matched on this machine with no network and
//! no key. It is a local subset: the API also runs ML classifiers and applies
//! the project's policy, and nothing here stands in for that.
//!
//! `pack.json` is generated, never hand-edited. It holds the Go detector
//! tables of the copyright holder's own earlier private rule set ("the
//! source"; Go `regexp`, RE2 syntax, which the `regex` crate accepts as is) and
//! the scoring pipeline they run under there, both dumped by
//! `scripts/local-rules/regenerate.sh`. Its `provenance` block names the source
//! files and their hashes, how matching differs from the source, which detectors
//! and rules were left out or demoted, and why.
//!
//! Scoring follows the source's `DefaultInputSteps`: a finding's weighted score is
//! its severity's score times its confidence (after the detector's combination
//! bonus), and each pipeline step applies its strategy when the score reaches
//! the step's threshold. Every number comes from `pack.json` except the
//! combination bonuses, which are detector code in the source rather than data.

use crate::error::{PromptGuardError, Result};
use regex::{Regex, RegexBuilder};
use serde::Deserialize;
use std::collections::{BTreeSet, HashMap};
use std::sync::OnceLock;

/// The pack as compiled into the binary.
pub const PACK_JSON: &str = include_str!("pack.json");

/// The largest input an offline scan accepts. Larger files are refused rather
/// than scanned slowly or partly: prompts and documents fit well under this.
pub const MAX_INPUT_BYTES: u64 = 10 * 1024 * 1024;

/// How many characters of a match a finding quotes.
const EXCERPT_CHARS: usize = 100;
/// How many characters of a secret a finding keeps; the rest is never shown.
const SECRET_PREFIX_CHARS: usize = 4;

/// The detector whose rules run only on untrusted content.
const UNTRUSTED_ONLY_DETECTOR: &str = "indirect_injection";

/// The source's confidence cap for the combination bonus (`min(c+bonus, 0.99)`
/// in `injection.go` and `jailbreak.go`).
const BONUS_CONFIDENCE_CAP: f64 = 0.99;

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
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Critical => "critical",
            Self::High => "high",
            Self::Medium => "medium",
        }
    }
}

/// What a finding does, weakest first. Only `Block` fails a scan. `Mask` is
/// what the source's pipeline does with a secret: it would be redacted in flight,
/// so it is reported but, as there, it does not block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Effect {
    /// Matched, but below every threshold for its detector.
    None,
    Warn,
    Mask,
    Block,
}

impl Effect {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Warn => "warn",
            Self::Mask => "mask",
            Self::Block => "block",
        }
    }

    fn from_strategy(strategy: &str) -> Result<Self> {
        match strategy {
            "warn" => Ok(Self::Warn),
            "mask" => Ok(Self::Mask),
            "block" => Ok(Self::Block),
            other => Err(PromptGuardError::Parse(format!(
                "pipeline strategy {other:?} has no meaning offline"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ScanOptions {
    /// The content comes from outside the operator: tool output, retrieved
    /// documents, memory. Turns on the indirect-injection rules, which the source
    /// runs only for those roles.
    pub untrusted: bool,
}

#[derive(Deserialize)]
struct PackFile {
    provenance: RawProvenance,
    pipeline: RawPipeline,
    patterns: Vec<RawRule>,
}

#[derive(Deserialize)]
struct RawProvenance {
    #[serde(default)]
    demoted: HashMap<String, String>,
}

#[derive(Deserialize)]
struct RawPipeline {
    severity_scores: HashMap<String, f64>,
    input_steps: Vec<RawStep>,
}

#[derive(Deserialize)]
struct RawStep {
    detector: String,
    strategy: String,
    threshold: f64,
}

#[derive(Deserialize)]
struct RawRule {
    id: String,
    detector: String,
    category: String,
    severity: Severity,
    confidence: f64,
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
    confidence: f64,
    /// The source's severity score for `severity`.
    severity_score: f64,
    /// A match counts only at or above this Shannon entropy; 0 means always.
    min_entropy: f64,
    /// Capped at warn by this pack (see `provenance.demoted`).
    demoted: bool,
    regex: Regex,
}

#[derive(Debug)]
struct Step {
    detector: String,
    effect: Effect,
    threshold: f64,
}

#[derive(Debug, Clone)]
pub struct LocalFinding {
    pub rule_id: String,
    pub detector: String,
    pub category: String,
    pub severity: Severity,
    /// Weighted score: severity score x confidence, after the combination bonus.
    pub score: f64,
    /// What this pack does with the finding.
    pub effect: Effect,
    /// What the source's pipeline would do, before this pack's demotions.
    pub upstream_effect: Effect,
    pub demoted: bool,
    /// `effect == Effect::Block`: this finding fails the scan.
    pub blocking: bool,
    /// 1-based line of the match in the scanned text.
    pub line: usize,
    /// The matched text, truncated; for a secret, only its first characters.
    pub excerpt: String,
}

#[derive(Debug)]
pub struct RulePack {
    rules: Vec<Rule>,
    steps: Vec<Step>,
}

/// The bundled pack, compiled once.
pub fn bundled() -> Result<&'static RulePack> {
    static PACK: OnceLock<std::result::Result<RulePack, String>> = OnceLock::new();
    PACK.get_or_init(|| RulePack::from_json(PACK_JSON).map_err(|e| e.to_string()))
        .as_ref()
        .map_err(|e| PromptGuardError::Parse(format!("bundled local rule pack: {e}")))
}

/// The strongest effect among `findings`: the scan's decision.
pub fn verdict(findings: &[LocalFinding]) -> Effect {
    findings
        .iter()
        .map(|f| f.effect)
        .max()
        .unwrap_or(Effect::None)
}

impl RulePack {
    pub fn from_json(json: &str) -> Result<Self> {
        let file: PackFile = serde_json::from_str(json)?;
        let steps = file
            .pipeline
            .input_steps
            .into_iter()
            .map(|s| {
                Ok(Step {
                    effect: Effect::from_strategy(&s.strategy)?,
                    detector: s.detector,
                    threshold: s.threshold,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let rules = file
            .patterns
            .into_iter()
            .map(|raw| {
                let severity_score = *file
                    .pipeline
                    .severity_scores
                    .get(raw.severity.as_str())
                    .ok_or_else(|| {
                        PromptGuardError::Parse(format!(
                            "no severity score for {}",
                            raw.severity.as_str()
                        ))
                    })?;
                let demoted = file.provenance.demoted.contains_key(&raw.id);
                compile(raw, severity_score, demoted)
            })
            .collect::<Result<Vec<_>>>()?;
        for id in file.provenance.demoted.keys() {
            if !rules.iter().any(|r| &r.id == id) {
                return Err(PromptGuardError::Parse(format!(
                    "demoted rule {id} is not in the pack"
                )));
            }
        }
        for rule in &rules {
            if !steps.iter().any(|s| s.detector == rule.detector) {
                return Err(PromptGuardError::Parse(format!(
                    "no pipeline step scores detector {}",
                    rule.detector
                )));
            }
        }
        Ok(Self { rules, steps })
    }

    pub fn rules(&self) -> &[Rule] {
        &self.rules
    }

    /// Every rule that matches `text`, at most one finding per rule (its
    /// first counted match), in pack order, each scored as the source would.
    pub fn scan(&self, text: &str, options: &ScanOptions) -> Vec<LocalFinding> {
        let matches: Vec<(&Rule, regex::Match<'_>)> = self
            .rules
            .iter()
            .filter(|rule| options.untrusted || rule.detector != UNTRUSTED_ONLY_DETECTOR)
            .filter_map(|rule| {
                rule.regex
                    .find_iter(text)
                    .find(|m| {
                        rule.min_entropy <= 0.0 || shannon_entropy(m.as_str()) >= rule.min_entropy
                    })
                    .map(|m| (rule, m))
            })
            .collect();

        matches
            .iter()
            .map(|(rule, found)| {
                let bonus = combination_bonus(&rule.detector, &matches);
                let confidence = if bonus > 0.0 {
                    (rule.confidence + bonus).min(BONUS_CONFIDENCE_CAP)
                } else {
                    rule.confidence
                };
                let score = (rule.severity_score * confidence).clamp(0.0, 1.0);
                let upstream_effect = self
                    .steps
                    .iter()
                    .filter(|s| s.detector == rule.detector && score >= s.threshold)
                    .map(|s| s.effect)
                    .max()
                    .unwrap_or(Effect::None);
                let effect = if rule.demoted {
                    upstream_effect.min(Effect::Warn)
                } else {
                    upstream_effect
                };
                LocalFinding {
                    rule_id: rule.id.clone(),
                    detector: rule.detector.clone(),
                    category: rule.category.clone(),
                    severity: rule.severity,
                    score,
                    effect,
                    upstream_effect,
                    demoted: rule.demoted,
                    blocking: effect == Effect::Block,
                    line: text[..found.start()].matches('\n').count() + 1,
                    excerpt: excerpt(&rule.detector, found.as_str()),
                }
            })
            .collect()
    }
}

/// The confidence the source adds to every finding of a detector when several of
/// its rules fire together. Ported from the detectors' `Detect` methods:
/// injection adds 0.1 once its findings span two categories; jailbreak adds
/// 0.1 for two attack families (the sub-category's first segment) and 0.2 for
/// three or more. The other detectors add nothing.
fn combination_bonus(detector: &str, matches: &[(&Rule, regex::Match<'_>)]) -> f64 {
    let own: Vec<&Rule> = matches
        .iter()
        .map(|(r, _)| *r)
        .filter(|r| r.detector == detector)
        .collect();
    if own.len() < 2 {
        return 0.0;
    }
    match detector {
        "injection" => {
            let categories: BTreeSet<&str> = own.iter().map(|r| r.category.as_str()).collect();
            if categories.len() >= 2 {
                0.1
            } else {
                0.0
            }
        },
        "jailbreak" => {
            let families: BTreeSet<&str> = own.iter().map(|r| family(&r.id)).collect();
            match families.len() {
                n if n >= 3 => 0.2,
                2 => 0.1,
                _ => 0.0,
            }
        },
        _ => 0.0,
    }
}

/// `jailbreak/persona.dan#2` -> `persona`: the source's `topLevel(SubCategory)`.
fn family(id: &str) -> &str {
    let sub = id.split_once('/').map_or(id, |(_, s)| s);
    let sub = sub.split_once('#').map_or(sub, |(s, _)| s);
    sub.split_once('.').map_or(sub, |(f, _)| f)
}

fn compile(raw: RawRule, severity_score: f64, demoted: bool) -> Result<Rule> {
    let mut builder = RegexBuilder::new(&raw.regex);
    match raw.detector.as_str() {
        // The source lowercases the input before its injection rules run. Matching
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
        confidence: raw.confidence,
        severity_score,
        min_entropy: raw.min_entropy,
        demoted,
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

/// Bits per character, computed as the source does: character counts over the
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
