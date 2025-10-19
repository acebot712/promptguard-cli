/// Provider Registry - The Single Source of Truth
///
/// Adding a new LLM provider? Add ONE entry here. That's it.
/// No scattered changes across multiple files. No hardcoded class names.
/// Data-driven, not code-driven.
///
/// This is how you scale without creating technical debt.
use crate::types::Provider;

/// Provider detection metadata
#[derive(Debug, Clone)]
pub struct ProviderInfo {
    /// Provider enum variant
    pub provider: Provider,

    /// Python package names that indicate this provider
    /// Examples: ["openai"], ["anthropic"], ["google.genai"]
    pub python_packages: &'static [&'static str],

    /// TypeScript/JavaScript package names
    /// Examples: ["openai"], ["@anthropic-ai/sdk"], ["groq-sdk"]
    pub typescript_packages: &'static [&'static str],

    /// API endpoint domains (for URL-based fallback detection)
    /// Examples: ["api.openai.com"], ["api.anthropic.com"]
    pub api_endpoints: &'static [&'static str],

    /// Parameter name for base URL in this provider's SDK
    /// Most use "baseURL", some use "base_url" or custom names
    pub base_url_param: &'static str,
}

/// THE REGISTRY - Add new providers here (1 entry = full support)
pub const PROVIDERS: &[ProviderInfo] = &[
    ProviderInfo {
        provider: Provider::OpenAI,
        python_packages: &["openai"],
        typescript_packages: &["openai"],
        api_endpoints: &["api.openai.com", "openai.azure.com"],
        base_url_param: "base_url",
    },
    ProviderInfo {
        provider: Provider::Anthropic,
        python_packages: &["anthropic"],
        typescript_packages: &["@anthropic-ai/sdk"],
        api_endpoints: &["api.anthropic.com"],
        base_url_param: "base_url",
    },
    ProviderInfo {
        provider: Provider::Cohere,
        python_packages: &["cohere"],
        typescript_packages: &["cohere-ai"],
        api_endpoints: &["api.cohere.ai", "api.cohere.com"],
        base_url_param: "base_url",
    },
    ProviderInfo {
        provider: Provider::HuggingFace,
        python_packages: &["huggingface_hub"],
        typescript_packages: &["@huggingface/inference"],
        api_endpoints: &["huggingface.co", "api-inference.huggingface.co"],
        base_url_param: "base_url",
    },
    ProviderInfo {
        provider: Provider::Gemini,
        python_packages: &["google.genai", "google.generativeai"],
        typescript_packages: &["@google/genai", "@google/generative-ai"],
        api_endpoints: &["generativelanguage.googleapis.com", "ai.google.dev"],
        base_url_param: "base_url",
    },
    ProviderInfo {
        provider: Provider::Groq,
        python_packages: &["groq"],
        typescript_packages: &["groq-sdk"],
        api_endpoints: &["api.groq.com"],
        base_url_param: "base_url",
    },
];

impl ProviderInfo {
    /// Find provider by Python package import
    pub fn from_python_package(package: &str) -> Option<&'static ProviderInfo> {
        PROVIDERS.iter().find(|p| {
            p.python_packages.iter().any(|pkg| {
                // Match exact package or submodule
                // "google.genai" matches "google.genai" or "google"
                package == *pkg || package.starts_with(&format!("{}.", pkg))
            })
        })
    }

    /// Find provider by TypeScript/JavaScript package import
    pub fn from_typescript_package(package: &str) -> Option<&'static ProviderInfo> {
        PROVIDERS
            .iter()
            .find(|p| p.typescript_packages.iter().any(|pkg| *pkg == package))
    }

    /// Find provider by API endpoint URL
    pub fn from_api_url(url: &str) -> Option<&'static ProviderInfo> {
        PROVIDERS.iter().find(|p| {
            p.api_endpoints
                .iter()
                .any(|endpoint| url.contains(endpoint))
        })
    }

    /// Get all provider names as strings (for CLI display)
    pub fn all_provider_names() -> Vec<&'static str> {
        PROVIDERS.iter().map(|p| p.provider.as_str()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_python_package_lookup() {
        assert!(ProviderInfo::from_python_package("openai").is_some());
        assert!(ProviderInfo::from_python_package("anthropic").is_some());
        assert!(ProviderInfo::from_python_package("groq").is_some());
        assert!(ProviderInfo::from_python_package("google.genai").is_some());
        assert!(ProviderInfo::from_python_package("unknown_package").is_none());
    }

    #[test]
    fn test_typescript_package_lookup() {
        assert!(ProviderInfo::from_typescript_package("openai").is_some());
        assert!(ProviderInfo::from_typescript_package("@anthropic-ai/sdk").is_some());
        assert!(ProviderInfo::from_typescript_package("groq-sdk").is_some());
    }

    #[test]
    fn test_api_url_lookup() {
        assert!(ProviderInfo::from_api_url("https://api.openai.com/v1/chat").is_some());
        assert!(ProviderInfo::from_api_url("https://api.anthropic.com/v1/messages").is_some());
        assert!(ProviderInfo::from_api_url("https://api.groq.com/openai/v1/chat").is_some());
    }
}
