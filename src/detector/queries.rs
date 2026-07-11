/// Tree-sitter queries for SDK detection and transformation.
///
/// TypeScript queries are templated from the registry's class names.
/// Python queries use templates for the 5 standard providers;
/// Gemini and Bedrock have special patterns.
use crate::detector::registry::ProviderInfo;
use crate::types::Provider;

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

fn standard_python_detection_query(class_name: &str) -> String {
    format!(
        r#"
            [
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
            ]
        "#
    )
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
    let class_name = info.py_class_name;
    let module_name = info.py_module_name;

    let identifier_pattern = format!(
        r#"
                (call
                    function: (identifier) @function
                    (#eq? @function "{class_name}")
                    arguments: (argument_list) @args
                ) @call_expr
        "#
    );

    let attribute_pattern = if module_name.is_empty() {
        String::new()
    } else {
        format!(
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
        )
    };

    format!(
        r"
            [
{identifier_pattern}
{attribute_pattern}
            ]
        "
    )
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
            .map(|info| standard_python_detection_query(info.py_class_name))
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
