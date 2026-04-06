use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    OpenAI,
    Anthropic,
    Cohere,
    HuggingFace,
    Gemini,
    Groq,
    Bedrock,
}

impl Provider {
    pub fn as_str(&self) -> &'static str {
        match self {
            Provider::OpenAI => "openai",
            Provider::Anthropic => "anthropic",
            Provider::Cohere => "cohere",
            Provider::HuggingFace => "huggingface",
            Provider::Gemini => "gemini",
            Provider::Groq => "groq",
            Provider::Bedrock => "bedrock",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "openai" => Some(Provider::OpenAI),
            "anthropic" => Some(Provider::Anthropic),
            "cohere" => Some(Provider::Cohere),
            "huggingface" | "hf" => Some(Provider::HuggingFace),
            "gemini" | "google" => Some(Provider::Gemini),
            "groq" => Some(Provider::Groq),
            "bedrock" | "aws-bedrock" | "aws" => Some(Provider::Bedrock),
            _ => None,
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Provider::OpenAI => "OpenAI",
            Provider::Anthropic => "Anthropic",
            Provider::Cohere => "Cohere",
            Provider::HuggingFace => "HuggingFace",
            Provider::Gemini => "Gemini",
            Provider::Groq => "Groq",
            Provider::Bedrock => "AWS Bedrock",
        }
    }
}

/// A detected instance of LLM SDK usage in a source file.
#[derive(Debug, Clone)]
pub struct DetectionInstance {
    pub file_path: PathBuf,
    pub line: usize,
    pub column: usize,
    pub has_base_url: bool,
    pub current_base_url: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Language {
    TypeScript,
    JavaScript,
    Python,
}

impl Language {
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext {
            "ts" | "tsx" => Some(Language::TypeScript),
            "js" | "jsx" => Some(Language::JavaScript),
            "py" => Some(Language::Python),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Language::TypeScript => "typescript",
            Language::JavaScript => "javascript",
            Language::Python => "python",
        }
    }
}

#[derive(Debug, Clone)]
pub struct DetectionResult {
    pub instances: Vec<DetectionInstance>,
}

impl Default for DetectionResult {
    fn default() -> Self {
        Self::new()
    }
}

impl DetectionResult {
    pub fn new() -> Self {
        Self {
            instances: Vec::new(),
        }
    }
}

/// Result of a file transformation operation.
#[derive(Debug, Clone)]
pub struct TransformResult {
    pub modified: bool,
}
