use super::core::{transform_file_generic, TransformConfig};
use crate::detector::get_python_transform_query;
use crate::transformer::Transformer;
use crate::types::{Provider, TransformResult};
use std::fmt::Write;
use std::path::Path;

pub struct PythonTransformer;

impl Default for PythonTransformer {
    fn default() -> Self {
        Self::new()
    }
}

impl PythonTransformer {
    pub fn new() -> Self {
        Self
    }
}

fn has_base_url(source: &str, args_node: tree_sitter::Node) -> bool {
    let args_text = &source[args_node.start_byte()..args_node.end_byte()];
    args_text.contains("base_url=") || args_text.contains("base_url =")
}

fn transform_args(
    source: &str,
    args_node: tree_sitter::Node,
    proxy_url: &str,
    api_key_env_var: &str,
) -> Option<String> {
    if has_base_url(source, args_node) {
        return None;
    }

    let args_text = &source[args_node.start_byte()..args_node.end_byte()];
    let inner = args_text
        .trim_start_matches('(')
        .trim_end_matches(')')
        .trim();

    let mut new_args = String::from("(\n");

    if inner.is_empty() {
        let _ = writeln!(
            new_args,
            "    api_key=os.environ.get(\"{api_key_env_var}\"),"
        );
        let _ = writeln!(new_args, "    base_url=\"{proxy_url}\"");
    } else {
        let trimmed = inner.trim();
        new_args.push_str("    ");
        new_args.push_str(trimmed);
        if !trimmed.ends_with(',') {
            new_args.push(',');
        }
        new_args.push('\n');
        let _ = writeln!(new_args, "    base_url=\"{proxy_url}\"");
    }

    new_args.push(')');
    Some(new_args)
}

/// Whether the module already binds the name `os` via an import.
///
/// Recognizes `import os`, `import os.path` (which binds `os`), and `os`
/// inside multi-imports (`import sys, os`). Rejects aliased imports
/// (`import os as x` binds `x`, not `os`) and unrelated substrings that the
/// old `contains("import os")` check false-positived on (comments, strings,
/// `from os import ...`, `import ossystem`).
fn has_os_import(source: &str) -> bool {
    for line in source.lines() {
        let trimmed = line.trim_start();
        let Some(rest) = trimmed.strip_prefix("import ") else {
            continue;
        };
        // Strip trailing comment, then check each comma-separated module
        let rest = rest.split('#').next().unwrap_or("");
        for part in rest.split(',') {
            let part = part.trim();
            if part.contains(" as ") {
                continue; // binds the alias, not `os`
            }
            if part == "os" || part.starts_with("os.") {
                return true;
            }
        }
    }
    false
}

/// Prepend `import os` at a syntactically valid location.
///
/// Inserting at byte 0 breaks shebang lines, module docstrings, and
/// `from __future__` imports (which must precede all other statements), so
/// this reuses the injector's insertion-point logic.
fn ensure_os_import(source: String) -> String {
    if has_os_import(&source) {
        return source;
    }

    let lines: Vec<&str> = source.lines().collect();
    let insert_at = crate::shim::injector::python_insertion_line(&lines);

    let mut result = String::with_capacity(source.len() + 16);
    for (i, line) in lines.iter().enumerate() {
        if i == insert_at {
            result.push_str("import os\n");
        }
        result.push_str(line);
        result.push('\n');
    }
    if insert_at >= lines.len() {
        result.push_str("import os\n");
    }
    result
}

impl Transformer for PythonTransformer {
    fn transform_file(
        &self,
        file_path: &Path,
        provider: Provider,
        proxy_url: &str,
        api_key_env_var: &str,
    ) -> crate::error::Result<TransformResult> {
        let config = TransformConfig {
            parser_language: tree_sitter_python::LANGUAGE.into(),
            language_name: "Python",
        };
        let query_str = get_python_transform_query(provider);

        transform_file_generic(
            file_path,
            &config,
            &query_str,
            |source, args_node| {
                transform_args(source, args_node, proxy_url, api_key_env_var)
                    .map(|new_args| (args_node.start_byte(), args_node.end_byte(), new_args))
            },
            ensure_os_import,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn os_import_detection_true_positives() {
        assert!(has_os_import("import os\n"));
        assert!(has_os_import("import os.path\n"));
        assert!(has_os_import("import sys, os\n"));
        assert!(has_os_import("import sys, os  # comment\n"));
        assert!(has_os_import("    import os\n"));
    }

    #[test]
    fn os_import_detection_false_positives_rejected() {
        // These all contain the substring "import os" but do not bind `os`
        assert!(!has_os_import("import ossystem\n"));
        assert!(!has_os_import("# import os\n"));
        assert!(!has_os_import("import os as operating_system\n"));
        assert!(!has_os_import("from os import environ\n"));
        assert!(!has_os_import("x = 'import os'\n"));
    }

    #[test]
    fn os_import_inserted_after_shebang_docstring_and_future() {
        let source = "#!/usr/bin/env python3\n\"\"\"Module docstring.\"\"\"\nfrom __future__ import annotations\n\nclient = OpenAI()\n";
        let result = ensure_os_import(source.to_string());
        let lines: Vec<&str> = result.lines().collect();
        assert_eq!(lines[0], "#!/usr/bin/env python3");
        assert_eq!(lines[1], "\"\"\"Module docstring.\"\"\"");
        assert_eq!(lines[2], "from __future__ import annotations");
        assert!(
            result.find("import os").unwrap_or(0) > result.find("__future__").unwrap_or(usize::MAX),
            "import os must come after __future__ imports:\n{result}"
        );
    }

    #[test]
    fn os_import_not_duplicated() {
        let source = "import os\nclient = OpenAI()\n";
        let result = ensure_os_import(source.to_string());
        assert_eq!(result.matches("import os").count(), 1);
    }
}
