use crate::detector::core::{detect_in_file_generic, DetectorConfig};
use crate::detector::Detector;
use crate::error::Result;
use crate::types::{DetectionResult, Language, Provider};
use std::path::Path;

pub struct PythonDetector;

impl PythonDetector {
    pub fn new() -> Self {
        Self
    }

    fn get_query_for_provider(provider: Provider) -> &'static str {
        match provider {
            Provider::OpenAI => {
                r#"
                [
                    ; Pattern 1: Direct import - from openai import OpenAI; client = OpenAI()
                    (call
                        function: (identifier) @function
                        (#eq? @function "OpenAI")
                        arguments: (argument_list) @args
                    ) @call_expr

                    ; Pattern 2: Module import - import openai; client = openai.OpenAI()
                    (call
                        function: (attribute
                            attribute: (identifier) @class
                            (#eq? @class "OpenAI")
                        )
                        arguments: (argument_list) @args
                    ) @call_expr
                ]
            "#
            },
            Provider::Anthropic => {
                r#"
                [
                    ; Pattern 1: Direct import - from anthropic import Anthropic; client = Anthropic()
                    (call
                        function: (identifier) @function
                        (#eq? @function "Anthropic")
                        arguments: (argument_list) @args
                    ) @call_expr

                    ; Pattern 2: Module import - import anthropic; client = anthropic.Anthropic()
                    (call
                        function: (attribute
                            attribute: (identifier) @class
                            (#eq? @class "Anthropic")
                        )
                        arguments: (argument_list) @args
                    ) @call_expr
                ]
            "#
            },
            Provider::Cohere => {
                r#"
                [
                    ; Pattern 1: Direct import - from cohere import CohereClient; client = CohereClient()
                    (call
                        function: (identifier) @function
                        (#eq? @function "CohereClient")
                        arguments: (argument_list) @args
                    ) @call_expr

                    ; Pattern 2: Module import - import cohere; client = cohere.CohereClient()
                    (call
                        function: (attribute
                            attribute: (identifier) @class
                            (#eq? @class "CohereClient")
                        )
                        arguments: (argument_list) @args
                    ) @call_expr
                ]
            "#
            },
            Provider::HuggingFace => {
                r#"
                [
                    ; Pattern 1: Direct import - from huggingface_hub import InferenceClient; client = InferenceClient()
                    (call
                        function: (identifier) @function
                        (#eq? @function "InferenceClient")
                        arguments: (argument_list) @args
                    ) @call_expr

                    ; Pattern 2: Module import - import huggingface_hub; client = huggingface_hub.InferenceClient()
                    (call
                        function: (attribute
                            attribute: (identifier) @class
                            (#eq? @class "InferenceClient")
                        )
                        arguments: (argument_list) @args
                    ) @call_expr
                ]
            "#
            },
            Provider::Gemini => {
                r#"
                [
                    ; Pattern 1: Direct import - from google.genai import Client; client = Client()
                    ; NOTE: "Client" is too generic, this pattern is rare
                    (call
                        function: (identifier) @function
                        (#eq? @function "Client")
                        arguments: (argument_list) @args
                    ) @call_expr

                    ; Pattern 2: Module import - from google import genai; client = genai.Client()
                    ; This is the official Gemini pattern
                    (call
                        function: (attribute
                            object: (identifier) @module
                            (#eq? @module "genai")
                            attribute: (identifier) @class
                            (#eq? @class "Client")
                        )
                        arguments: (argument_list) @args
                    ) @call_expr
                ]
            "#
            },
            Provider::Groq => {
                r#"
                [
                    ; Pattern 1: Direct import - from groq import Groq; client = Groq()
                    (call
                        function: (identifier) @function
                        (#eq? @function "Groq")
                        arguments: (argument_list) @args
                    ) @call_expr

                    ; Pattern 2: Module import - import groq; client = groq.Groq()
                    (call
                        function: (attribute
                            attribute: (identifier) @class
                            (#eq? @class "Groq")
                        )
                        arguments: (argument_list) @args
                    ) @call_expr
                ]
            "#
            },
        }
    }

    fn check_has_base_url(
        source: &str,
        args_node: tree_sitter::Node,
        _provider: Provider,
    ) -> (bool, Option<String>) {
        let args_text = &source[args_node.start_byte()..args_node.end_byte()];
        let has_base_url = args_text.contains("base_url=") || args_text.contains("base_url =");

        let current_base_url = if has_base_url {
            Some("(configured)".to_string())
        } else {
            None
        };

        (has_base_url, current_base_url)
    }
}

impl Detector for PythonDetector {
    fn detect_in_file(&self, file_path: &Path, provider: Provider) -> Result<DetectionResult> {
        let config = DetectorConfig {
            ts_language: tree_sitter_python::language(),
            language: Language::Python,
            capture_name: "call_expr",
        };

        let query_str = Self::get_query_for_provider(provider);

        detect_in_file_generic(
            file_path,
            provider,
            &config,
            query_str,
            Self::check_has_base_url,
        )
    }

    fn language(&self) -> Language {
        Language::Python
    }
}
