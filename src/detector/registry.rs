/// Provider Registry - The Single Source of Truth
///
/// Adding a new LLM provider? Add ONE entry here.
/// All provider metadata lives in this single table:
/// package names, class names, parameter names, API endpoints.
use crate::types::Provider;

#[derive(Debug, Clone)]
pub struct ProviderInfo {
    pub provider: Provider,
    pub py_class_name: &'static str,
    pub ts_class_name: &'static str,
    pub ts_base_url_param: &'static str,
    pub ts_api_key_param: &'static str,
}

pub const PROVIDERS: &[ProviderInfo] = &[
    ProviderInfo {
        provider: Provider::OpenAI,
        py_class_name: "OpenAI",
        ts_class_name: "OpenAI",
        ts_base_url_param: "baseURL",
        ts_api_key_param: "apiKey",
    },
    ProviderInfo {
        provider: Provider::Anthropic,
        py_class_name: "Anthropic",
        ts_class_name: "Anthropic",
        ts_base_url_param: "baseURL",
        ts_api_key_param: "apiKey",
    },
    ProviderInfo {
        provider: Provider::Cohere,
        py_class_name: "CohereClient",
        ts_class_name: "CohereClient",
        ts_base_url_param: "baseURL",
        ts_api_key_param: "apiKey",
    },
    ProviderInfo {
        provider: Provider::HuggingFace,
        py_class_name: "InferenceClient",
        ts_class_name: "HfInference",
        ts_base_url_param: "baseUrl",
        ts_api_key_param: "accessToken",
    },
    ProviderInfo {
        provider: Provider::Gemini,
        py_class_name: "Client",
        ts_class_name: "GoogleGenAI",
        ts_base_url_param: "baseURL",
        ts_api_key_param: "apiKey",
    },
    ProviderInfo {
        provider: Provider::Groq,
        py_class_name: "Groq",
        ts_class_name: "Groq",
        ts_base_url_param: "baseURL",
        ts_api_key_param: "apiKey",
    },
    ProviderInfo {
        provider: Provider::Bedrock,
        py_class_name: "",
        ts_class_name: "BedrockRuntimeClient",
        ts_base_url_param: "",
        ts_api_key_param: "",
    },
];

impl ProviderInfo {
    pub fn get(provider: Provider) -> &'static ProviderInfo {
        for info in PROVIDERS {
            if info.provider == provider {
                return info;
            }
        }
        &PROVIDERS[0]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_providers_in_registry() {
        let all = [
            Provider::OpenAI,
            Provider::Anthropic,
            Provider::Cohere,
            Provider::HuggingFace,
            Provider::Gemini,
            Provider::Groq,
            Provider::Bedrock,
        ];
        for p in all {
            assert_eq!(
                ProviderInfo::get(p).provider,
                p,
                "Provider {p:?} not found or mismatched in registry"
            );
        }
    }
}
