/// Tree-sitter queries for SDK detection and transformation.
///
/// TypeScript queries are templated from the registry's class names.
/// Python queries use templates for the 5 standard providers;
/// Gemini and Bedrock have special patterns.
use crate::detector::registry::ProviderInfo;
use crate::types::Provider;

pub fn get_typescript_query(provider: Provider) -> String {
    let info = ProviderInfo::get(provider);
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

fn standard_python_transform_query(class_name: &str) -> String {
    format!(
        r#"
            (call
                function: (identifier) @function
                (#eq? @function "{class_name}")
                arguments: (argument_list) @args
            ) @call_expr
        "#
    )
}

pub fn get_python_detection_query(provider: Provider) -> String {
    let info = ProviderInfo::get(provider);
    match provider {
        Provider::Gemini => r#"
            [
                (call
                    function: (identifier) @function
                    (#eq? @function "Client")
                    arguments: (argument_list) @args
                ) @call_expr

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
        .to_string(),
        Provider::Bedrock => r#"
            [
                (call
                    function: (attribute
                        object: (identifier) @module
                        (#eq? @module "boto3")
                        attribute: (identifier) @method
                        (#eq? @method "client")
                    )
                    arguments: (argument_list) @args
                ) @call_expr
            ]
        "#
        .to_string(),
        _ => standard_python_detection_query(info.py_class_name),
    }
}

pub fn get_python_transform_query(provider: Provider) -> String {
    let info = ProviderInfo::get(provider);
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
        Provider::Bedrock => r#"
            (call
                function: (attribute
                    object: (identifier) @module
                    (#eq? @module "boto3")
                    attribute: (identifier) @method
                    (#eq? @method "client")
                )
                arguments: (argument_list) @args
            ) @call_expr
        "#
        .to_string(),
        _ => standard_python_transform_query(info.py_class_name),
    }
}
