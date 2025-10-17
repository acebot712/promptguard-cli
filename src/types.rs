use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    OpenAI,
    Anthropic,
    Cohere,
    HuggingFace,
}

impl Provider {
    pub fn as_str(&self) -> &'static str {
        match self {
            Provider::OpenAI => "openai",
            Provider::Anthropic => "anthropic",
            Provider::Cohere => "cohere",
            Provider::HuggingFace => "huggingface",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "openai" => Some(Provider::OpenAI),
            "anthropic" => Some(Provider::Anthropic),
            "cohere" => Some(Provider::Cohere),
            "huggingface" | "hf" => Some(Provider::HuggingFace),
            _ => None,
        }
    }

    pub fn class_name(&self) -> &'static str {
        match self {
            Provider::OpenAI => "OpenAI",
            Provider::Anthropic => "Anthropic",
            Provider::Cohere => "CohereClient",
            Provider::HuggingFace => "HfInference",
        }
    }

    pub fn python_class_name(&self) -> &'static str {
        match self {
            Provider::OpenAI => "OpenAI",
            Provider::Anthropic => "Anthropic",
            Provider::Cohere => "CohereClient",
            Provider::HuggingFace => "InferenceClient",
        }
    }

    pub fn base_url_param(&self) -> &'static str {
        match self {
            Provider::HuggingFace => "baseUrl",
            _ => "baseURL",
        }
    }

    pub fn api_key_param(&self) -> &'static str {
        match self {
            Provider::HuggingFace => "accessToken",
            _ => "apiKey",
        }
    }
}

#[derive(Debug, Clone)]
pub struct DetectionInstance {
    pub file_path: PathBuf,
    pub line: usize,
    pub column: usize,
    pub provider: Provider,
    pub language: Language,
    pub has_base_url: bool,
    pub current_base_url: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

impl DetectionResult {
    pub fn new() -> Self {
        Self {
            instances: Vec::new(),
        }
    }

    pub fn file_count(&self) -> usize {
        let mut files: Vec<_> = self.instances.iter().map(|i| &i.file_path).collect();
        files.sort();
        files.dedup();
        files.len()
    }

    pub fn get_files(&self) -> Vec<PathBuf> {
        let mut files: Vec<_> = self.instances.iter().map(|i| i.file_path.clone()).collect();
        files.sort();
        files.dedup();
        files
    }
}

#[derive(Debug, Clone)]
pub struct TransformResult {
    pub file_path: PathBuf,
    pub success: bool,
    pub modified: bool,
    pub error: Option<String>,
}
