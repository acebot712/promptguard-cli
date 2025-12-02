/// Shared Tree-sitter queries for SDK detection and transformation
///
/// These queries are used by both the detector and transformer modules.
/// Centralizing them here eliminates duplication and ensures consistency.
use crate::types::Provider;

/// Get the TypeScript/JavaScript Tree-sitter query for a provider
pub fn get_typescript_query(provider: Provider) -> &'static str {
    match provider {
        Provider::OpenAI => {
            r#"
            (new_expression
                constructor: (identifier) @constructor
                (#eq? @constructor "OpenAI")
                arguments: (arguments) @args
            ) @new_expr
        "#
        },
        Provider::Anthropic => {
            r#"
            (new_expression
                constructor: (identifier) @constructor
                (#eq? @constructor "Anthropic")
                arguments: (arguments) @args
            ) @new_expr
        "#
        },
        Provider::Cohere => {
            r#"
            (new_expression
                constructor: (identifier) @constructor
                (#eq? @constructor "CohereClient")
                arguments: (arguments) @args
            ) @new_expr
        "#
        },
        Provider::HuggingFace => {
            r#"
            (new_expression
                constructor: (identifier) @constructor
                (#eq? @constructor "HfInference")
                arguments: (arguments) @args
            ) @new_expr
        "#
        },
        Provider::Gemini => {
            r#"
            (new_expression
                constructor: (identifier) @constructor
                (#eq? @constructor "GoogleGenAI")
                arguments: (arguments) @args
            ) @new_expr
        "#
        },
        Provider::Groq => {
            r#"
            (new_expression
                constructor: (identifier) @constructor
                (#eq? @constructor "Groq")
                arguments: (arguments) @args
            ) @new_expr
        "#
        },
    }
}

/// Get the Python Tree-sitter query for detection (supports multiple patterns)
pub fn get_python_detection_query(provider: Provider) -> &'static str {
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

/// Get the Python Tree-sitter query for transformation (simpler pattern)
pub fn get_python_transform_query(provider: Provider) -> &'static str {
    match provider {
        Provider::OpenAI => {
            r#"
            (call
                function: (identifier) @function
                (#eq? @function "OpenAI")
                arguments: (argument_list) @args
            ) @call_expr
        "#
        },
        Provider::Anthropic => {
            r#"
            (call
                function: (identifier) @function
                (#eq? @function "Anthropic")
                arguments: (argument_list) @args
            ) @call_expr
        "#
        },
        Provider::Cohere => {
            r#"
            (call
                function: (identifier) @function
                (#eq? @function "CohereClient")
                arguments: (argument_list) @args
            ) @call_expr
        "#
        },
        Provider::HuggingFace => {
            r#"
            (call
                function: (identifier) @function
                (#eq? @function "InferenceClient")
                arguments: (argument_list) @args
            ) @call_expr
        "#
        },
        Provider::Gemini => {
            r#"
            (call
                function: (attribute
                    object: (identifier) @module
                    (#eq? @module "genai")
                    attribute: (identifier) @class
                    (#eq? @class "Client")
                )
                arguments: (argument_list) @args
            ) @call_expr
        "#
        },
        Provider::Groq => {
            r#"
            (call
                function: (identifier) @function
                (#eq? @function "Groq")
                arguments: (argument_list) @args
            ) @call_expr
        "#
        },
    }
}
