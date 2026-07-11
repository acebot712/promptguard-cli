/// Tree-sitter queries for SDK detection and transformation.
///
/// TypeScript queries are templated from the registry's class names.
/// Python queries use templates for the 5 standard providers;
/// Gemini and Bedrock have special patterns.
use crate::detector::registry::ProviderInfo;
use crate::types::Provider;
use std::fmt::Write;

pub fn get_typescript_query(provider: Provider) -> String {
    // An unregistered provider yields an empty query (matches nothing)
    // rather than silently borrowing another provider's class name.
    let Some(info) = ProviderInfo::get(provider) else {
        return String::new();
    };
    format!(
        r#"
            (new_expression
                constructor: (identifier) @constructor
                (#eq? @constructor "{}")
                arguments: (arguments) @args
            ) @new_expr
        "#,
        info.ts_class_name
    )
}

/// Detection query for the standard providers: bare-identifier and
/// attribute-form constructor calls, for the sync class name AND the SDK's
/// async client class (`AsyncOpenAI`, `AsyncAnthropic`, `AsyncGroq`,
/// `AsyncInferenceClient`) when the registry declares one. Async clients
/// used to be invisible to detection entirely.
///
/// NOTE: the patterns are emitted as SEPARATE top-level query patterns, not
/// one `[...]` alternation. Branches of a top-level alternation share a
/// single pattern index, so their `#eq?` predicates accumulate — a capture
/// name reused across branches with different expected values (`@function`
/// = `"OpenAI"` vs `"AsyncOpenAI"`) could then never match anything.
fn standard_python_detection_query(info: &ProviderInfo) -> String {
    let mut patterns = String::new();

    for class_name in [info.py_class_name, info.py_async_class_name] {
        if class_name.is_empty() {
            continue;
        }
        let _ = write!(
            patterns,
            r#"
            (call
                function: (identifier) @function
                (#eq? @function "{class_name}")
                arguments: (argument_list) @args
            ) @call_expr

            (call
                function: (attribute
                    attribute: (identifier) @class
                    (#eq? @class "{class_name}")
                )
                arguments: (argument_list) @args
            ) @call_expr
        "#
        );
    }

    patterns
}

/// Transform query for the standard providers: bare-identifier calls
/// (`OpenAI(...)`) plus module-qualified attribute calls constrained to the
/// SDK's own module (`openai.OpenAI(...)`).
///
/// The detection query matches ANY attribute-form call ending in the class
/// name, but the transformer must only rewrite calls it is sure about:
/// an unconstrained attribute pattern would rewrite `mymod.OpenAI(...)`
/// (some unrelated class that happens to share the name). Previously the
/// transform query had no attribute pattern at all, so detected
/// `openai.OpenAI(...)` calls were silently reported "(no changes needed)"
/// and never routed through the proxy.
fn standard_python_transform_query(info: &ProviderInfo) -> String {
    let module_name = info.py_module_name;
    let mut patterns = String::new();

    // Both the sync class and (when the registry declares one) the async
    // client class accept the same base_url= keyword, so both are
    // transformable. Separate top-level patterns, NOT one alternation —
    // see standard_python_detection_query for why.
    for class_name in [info.py_class_name, info.py_async_class_name] {
        if class_name.is_empty() {
            continue;
        }

        let _ = write!(
            patterns,
            r#"
            (call
                function: (identifier) @function
                (#eq? @function "{class_name}")
                arguments: (argument_list) @args
            ) @call_expr
        "#
        );

        if !module_name.is_empty() {
            let _ = write!(
                patterns,
                r#"
            (call
                function: (attribute
                    object: (identifier) @module
                    (#eq? @module "{module_name}")
                    attribute: (identifier) @class
                    (#eq? @class "{class_name}")
                )
                arguments: (argument_list) @args
            ) @call_expr
        "#
            );
        }
    }

    patterns
}

pub fn get_python_detection_query(provider: Provider) -> String {
    match provider {
        // Match genai.Client(...) and google.genai.Client(...) only. A bare
        // Client(...) alternative used to be included, but "Client" is far
        // too generic an identifier (database clients, HTTP clients, ...)
        // and produced false positives on unrelated code.
        Provider::Gemini => r#"
            [
                (call
                    function: (attribute
                        object: (identifier) @module
                        (#eq? @module "genai")
                        attribute: (identifier) @class
                        (#eq? @class "Client")
                    )
                    arguments: (argument_list) @args
                ) @call_expr

                (call
                    function: (attribute
                        object: (attribute
                            attribute: (identifier) @submodule
                            (#eq? @submodule "genai")
                        )
                        attribute: (identifier) @class
                        (#eq? @class "Client")
                    )
                    arguments: (argument_list) @args
                ) @call_expr
            ]
        "#
        .to_string(),
        // boto3.client(...) constructs clients for EVERY AWS service; only
        // "bedrock-runtime" clients are LLM-related. Match the service name
        // as the first positional string argument or as service_name=...,
        // otherwise boto3.client("s3") etc. would be reported (and,
        // previously, corrupted by the transformer).
        Provider::Bedrock => r#"
            [
                (call
                    function: (attribute
                        object: (identifier) @module
                        (#eq? @module "boto3")
                        attribute: (identifier) @method
                        (#eq? @method "client")
                    )
                    arguments: (argument_list
                        .
                        (string) @service
                    ) @args
                    (#match? @service "bedrock-runtime")
                ) @call_expr

                (call
                    function: (attribute
                        object: (identifier) @module
                        (#eq? @module "boto3")
                        attribute: (identifier) @method
                        (#eq? @method "client")
                    )
                    arguments: (argument_list
                        (keyword_argument
                            name: (identifier) @kwname
                            (#eq? @kwname "service_name")
                            value: (string) @service
                        )
                    ) @args
                    (#match? @service "bedrock-runtime")
                ) @call_expr
            ]
        "#
        .to_string(),
        _ => ProviderInfo::get(provider)
            .map(standard_python_detection_query)
            .unwrap_or_default(),
    }
}

pub fn get_python_transform_query(provider: Provider) -> String {
    match provider {
        Provider::Gemini => r#"
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
        .to_string(),
        // Bedrock is detect-only: boto3 clients authenticate via AWS
        // credentials/SigV4, and boto3.client() accepts neither api_key= nor
        // base_url= — injecting them raised TypeError at runtime. An empty
        // query compiles and matches nothing, so the transformer is a no-op.
        Provider::Bedrock => String::new(),
        _ => ProviderInfo::get(provider)
            .map(standard_python_transform_query)
            .unwrap_or_default(),
    }
}
