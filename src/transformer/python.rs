use super::core::transform_file_generic;
use crate::detector::{python_args_have_base_url, Grammar};
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
    python_args_have_base_url(args_text)
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
    // Strip exactly ONE outer `(` / `)` — the delimiters of this
    // `argument_list` node. `trim_end_matches(')')` stripped *every* trailing
    // paren, so `OpenAI(api_key=os.getenv("KEY"))` lost the inner call's
    // closing paren too and produced a SyntaxError.
    let inner = args_text
        .strip_prefix('(')
        .and_then(|s| s.strip_suffix(')'))
        .unwrap_or(args_text);

    // End of the last real argument (comments excluded): the injected comma
    // must go right after it. Appending the comma to the raw text swallowed
    // it into any trailing line comment (`api_key=key  # note` became
    // `api_key=key  # note,`) and produced a SyntaxError.
    let last_arg_end = last_non_comment_child_end(args_node);

    let mut new_args = String::from("(\n");

    match last_arg_end {
        Some(end) if args_text.starts_with('(') => {
            // Split `inner` right after the last argument; the tail is only
            // ever an optional trailing comma, whitespace, and comments.
            let split_at = end - args_node.start_byte() - 1;
            let (args_part, tail) = inner.split_at(split_at);

            let mut tail = tail.trim_start();
            if let Some(rest) = tail.strip_prefix(',') {
                tail = rest.trim_start();
            }
            let tail = tail.trim_end();

            new_args.push_str("    ");
            new_args.push_str(args_part.trim());
            new_args.push(',');
            if !tail.is_empty() {
                new_args.push_str("  ");
                new_args.push_str(tail);
            }
            new_args.push('\n');
            let _ = writeln!(new_args, "    base_url=\"{proxy_url}\"");
        },
        _ => {
            // No arguments: inject both the api_key and the proxy URL. Keep
            // any comment-only content after the injected arguments.
            let _ = writeln!(
                new_args,
                "    api_key=os.environ.get(\"{api_key_env_var}\"),"
            );
            let _ = writeln!(new_args, "    base_url=\"{proxy_url}\"");
            let comments = inner.trim();
            if !comments.is_empty() {
                let _ = writeln!(new_args, "    {comments}");
            }
        },
    }

    new_args.push(')');
    Some(new_args)
}

/// Byte offset just past the last named child of `node` that is not a
/// comment, or `None` when there is no such child.
fn last_non_comment_child_end(node: tree_sitter::Node) -> Option<usize> {
    let mut cursor = node.walk();
    let mut last_end = None;
    for child in node.named_children(&mut cursor) {
        if child.kind() != "comment" {
            last_end = Some(child.end_byte());
        }
    }
    last_end
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

    let mut out: Vec<String> = Vec::with_capacity(lines.len() + 1);
    for (i, line) in lines.iter().enumerate() {
        if i == insert_at {
            out.push("import os".to_string());
        }
        out.push((*line).to_string());
    }
    if insert_at >= lines.len() {
        out.push("import os".to_string());
    }
    // Preserve the file's dominant line ending and trailing-newline state
    // rather than forcing LF and a final newline.
    crate::text::join_preserving_style(&out, &source)
}

impl Transformer for PythonTransformer {
    fn transform_file(
        &self,
        file_path: &Path,
        provider: Provider,
        proxy_url: &str,
        api_key_env_var: &str,
    ) -> crate::error::Result<TransformResult> {
        transform_file_generic(
            file_path,
            Grammar::Python,
            provider,
            |source, args_node| {
                transform_args(source, args_node, proxy_url, api_key_env_var)
                    .map(|new_args| (args_node.start_byte(), args_node.end_byte(), new_args))
            },
            ensure_os_import,
        )
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::transformer::Transformer;
    use std::fs;
    use tempfile::TempDir;
    use tree_sitter::Parser;

    /// Parse `source` as Python and assert the tree has zero ERROR nodes.
    fn assert_reparses_clean(source: &str) {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_python::LANGUAGE.into())
            .expect("set python language");
        let tree = parser.parse(source, None).expect("parse");
        assert!(
            !tree.root_node().has_error(),
            "transformed Python has tree-sitter ERROR node(s):\n{source}"
        );
    }

    fn transform_python(input: &str) -> String {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("client.py");
        fs::write(&path, input).unwrap();
        PythonTransformer::new()
            .transform_file(
                &path,
                Provider::OpenAI,
                "https://api.promptguard.co/api/v1",
                "PROMPTGUARD_API_KEY",
            )
            .unwrap();
        fs::read_to_string(&path).unwrap()
    }

    /// Regression: `trim_end_matches(')')` stripped the inner `os.getenv(...)`
    /// closing paren too, producing unbalanced parens / a `SyntaxError`.
    #[test]
    fn nested_call_arg_keeps_balanced_parens() {
        let input =
            "from openai import OpenAI\nclient = OpenAI(api_key=os.getenv(\"OPENAI_API_KEY\"))\n";
        let out = transform_python(input);
        assert!(
            out.contains("os.getenv(\"OPENAI_API_KEY\")"),
            "output:\n{out}"
        );
        assert!(out.contains("base_url=\"https://api.promptguard.co/api/v1\""));
        // The original nested-call paren must survive.
        assert!(
            out.contains("getenv(\"OPENAI_API_KEY\"),"),
            "output:\n{out}"
        );
        assert_reparses_clean(&out);
    }

    #[test]
    fn empty_args_transform_reparses_clean() {
        let input = "from openai import OpenAI\nclient = OpenAI()\n";
        let out = transform_python(input);
        assert!(out.contains("base_url="));
        assert_reparses_clean(&out);
    }

    /// Regression: appending the comma to the raw argument text swallowed it
    /// into a trailing line comment (`api_key=key  # note,`) → `SyntaxError`.
    #[test]
    fn trailing_line_comment_keeps_comma_out_of_comment() {
        let input = "from openai import OpenAI\nclient = OpenAI(\n    api_key=key  # loaded from vault\n)\n";
        let out = transform_python(input);
        assert!(out.contains("base_url=\"https://api.promptguard.co/api/v1\""));
        assert!(
            out.contains("api_key=key,"),
            "comma must land after the argument, not inside the comment:\n{out}"
        );
        assert!(out.contains("# loaded from vault"), "comment kept:\n{out}");
        assert_reparses_clean(&out);
    }

    /// Trailing comma plus trailing comment must not produce a double comma.
    #[test]
    fn trailing_comma_and_comment_reparses_clean() {
        let input = "from openai import OpenAI\nclient = OpenAI(\n    api_key=key,  # note\n)\n";
        let out = transform_python(input);
        assert!(!out.contains(",,"), "no double comma:\n{out}");
        assert!(out.contains("# note"));
        assert_reparses_clean(&out);
    }

    /// Comment-only argument lists must keep the comment and stay valid.
    #[test]
    fn comment_only_args_reparses_clean() {
        let input = "from openai import OpenAI\nclient = OpenAI(\n    # configured elsewhere\n)\n";
        let out = transform_python(input);
        assert!(out.contains("base_url="));
        assert!(out.contains("# configured elsewhere"));
        assert_reparses_clean(&out);
    }

    #[test]
    fn crlf_source_transform_preserves_crlf() {
        let input = "from openai import OpenAI\r\nclient = OpenAI(api_key=os.getenv(\"K\"))\r\n";
        let out = transform_python(input);
        assert!(out.contains("\r\n"), "CRLF must be preserved:\n{out:?}");
        assert!(!out.contains("\n\r"), "no mangled endings:\n{out:?}");
        // No bare LF that isn't part of a CRLF pair.
        assert_eq!(out.matches('\n').count(), out.matches("\r\n").count());
        assert_reparses_clean(&out);
    }

    /// Bedrock is detect-only: `boto3.client()` accepts neither `api_key=`
    /// nor `base_url=`, so the transformer must leave every boto3 call
    /// untouched (previously it rewrote ALL `boto3.client(...)` calls — S3,
    /// `DynamoDB`, and bedrock-runtime alike — into `TypeError`-raising
    /// constructor calls).
    #[test]
    fn bedrock_transform_is_a_noop_for_all_boto3_clients() {
        let input = "import boto3\n\
                     s3 = boto3.client(\"s3\")\n\
                     br = boto3.client(\"bedrock-runtime\")\n";

        let dir = TempDir::new().unwrap();
        let path = dir.path().join("aws.py");
        fs::write(&path, input).unwrap();
        let result = PythonTransformer::new()
            .transform_file(
                &path,
                Provider::Bedrock,
                "https://api.promptguard.co/api/v1",
                "PROMPTGUARD_API_KEY",
            )
            .unwrap();

        assert!(!result.modified, "bedrock must be detect-only");
        let out = fs::read_to_string(&path).unwrap();
        assert_eq!(out, input, "boto3 calls must be byte-for-byte untouched");
    }

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
