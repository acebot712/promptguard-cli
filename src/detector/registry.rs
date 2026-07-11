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
    /// Python module the SDK class is imported from, used to match
    /// module-qualified constructor calls (`openai.OpenAI(...)`). Empty for
    /// providers whose queries are special-cased (Gemini, Bedrock).
    pub py_module_name: &'static str,
    pub ts_class_name: &'static str,
    pub ts_base_url_param: &'static str,
    pub ts_api_key_param: &'static str,
}

pub const PROVIDERS: &[ProviderInfo] = &[
    ProviderInfo {
        provider: Provider::OpenAI,
        py_class_name: "OpenAI",
        py_module_name: "openai",
        ts_class_name: "OpenAI",
        ts_base_url_param: "baseURL",
        ts_api_key_param: "apiKey",
    },
    ProviderInfo {
        provider: Provider::Anthropic,
        py_class_name: "Anthropic",
        py_module_name: "anthropic",
        ts_class_name: "Anthropic",
        ts_base_url_param: "baseURL",
        ts_api_key_param: "apiKey",
    },
    ProviderInfo {
        provider: Provider::Cohere,
        py_class_name: "CohereClient",
        py_module_name: "cohere",
        ts_class_name: "CohereClient",
        ts_base_url_param: "baseURL",
        ts_api_key_param: "apiKey",
    },
    ProviderInfo {
        provider: Provider::HuggingFace,
        py_class_name: "InferenceClient",
        py_module_name: "huggingface_hub",
        ts_class_name: "HfInference",
        ts_base_url_param: "baseUrl",
        ts_api_key_param: "accessToken",
    },
    ProviderInfo {
        provider: Provider::Gemini,
        py_class_name: "Client",
        py_module_name: "",
        ts_class_name: "GoogleGenAI",
        ts_base_url_param: "baseURL",
        ts_api_key_param: "apiKey",
    },
    ProviderInfo {
        provider: Provider::Groq,
        py_class_name: "Groq",
        py_module_name: "groq",
        ts_class_name: "Groq",
        ts_base_url_param: "baseURL",
        ts_api_key_param: "apiKey",
    },
    ProviderInfo {
        provider: Provider::Bedrock,
        py_class_name: "",
        py_module_name: "",
        ts_class_name: "BedrockRuntimeClient",
        ts_base_url_param: "",
        ts_api_key_param: "",
    },
];

impl ProviderInfo {
    /// Look up a provider's registry entry.
    ///
    /// Returns `None` when the provider is missing from the registry
    /// (previously this silently fell back to the `OpenAI` entry, producing
    /// wrong parameter names instead of an explicit failure). The registry
    /// is exhaustive over `Provider` — enforced by
    /// `test_all_providers_in_registry` — so callers may treat `None` as
    /// "no metadata available" and skip gracefully.
    pub fn get(provider: Provider) -> Option<&'static ProviderInfo> {
        PROVIDERS.iter().find(|info| info.provider == provider)
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
            let info = ProviderInfo::get(p);
            assert_eq!(
                info.map(|i| i.provider),
                Some(p),
                "Provider {p:?} not found or mismatched in registry"
            );
        }
    }
}
