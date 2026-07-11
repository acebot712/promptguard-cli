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

/// Whether the argument list forwards dynamic arguments via a splat
/// (`OpenAI(**cfg)` / `OpenAI(*args)`).
///
/// Appending `base_url=...` after `**cfg` raises `TypeError: got multiple
/// values for keyword argument 'base_url'` at runtime when `cfg` already
/// carries a `base_url` — the transformer cannot know what the splat expands
/// to, so these calls must be left untouched (mirroring the TypeScript
/// identifier-argument skip) and reported as needing manual routing.
fn args_have_splat(args_node: tree_sitter::Node) -> bool {
    let mut cursor = args_node.walk();
    for child in args_node.named_children(&mut cursor) {
        if matches!(child.kind(), "dictionary_splat" | "list_splat") {
            return true;
        }
    }
    false
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
        dry_run: bool,
    ) -> crate::error::Result<TransformResult> {
        // Counted from inside the Fn closure, so interior mutability.
        let manual_skips = std::cell::Cell::new(0usize);

        let mut result = transform_file_generic(
            file_path,
            Grammar::Python,
            provider,
            |source, args_node| {
                if !has_base_url(source, args_node) && args_have_splat(args_node) {
                    manual_skips.set(manual_skips.get() + 1);
                    return None;
                }
                transform_args(source, args_node, proxy_url, api_key_env_var)
                    .map(|new_args| (args_node.start_byte(), args_node.end_byte(), new_args))
            },
            ensure_os_import,
            dry_run,
        )?;
        result.needs_manual_routing = manual_skips.get();
        Ok(result)
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
                false,
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

    /// Transform `input` with the `OpenAI` provider and return the result plus
    /// the (possibly rewritten) file contents.
    fn transform_python_result(input: &str) -> (TransformResult, String) {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("client.py");
        fs::write(&path, input).unwrap();
        let result = PythonTransformer::new()
            .transform_file(
                &path,
                Provider::OpenAI,
                "https://api.promptguard.co/api/v1",
                "PROMPTGUARD_API_KEY",
                false,
            )
            .unwrap();
        (result, fs::read_to_string(&path).unwrap())
    }

    /// Regression: `OpenAI(**cfg)` became `OpenAI(**cfg, base_url=...)`,
    /// which raises `TypeError: got multiple values for keyword argument
    /// 'base_url'` at runtime whenever `cfg` carries a `base_url`. Splatted
    /// argument lists must be left untouched and reported as needing manual
    /// routing.
    #[test]
    fn dict_splat_args_left_untouched_and_reported() {
        let input = "from openai import OpenAI\nclient = OpenAI(**cfg)\n";
        let (result, out) = transform_python_result(input);
        assert!(!result.modified, "**cfg call must not be rewritten");
        assert_eq!(result.needs_manual_routing, 1, "skip must be reported");
        assert_eq!(out, input, "file must be byte-for-byte untouched");
    }

    /// Splats mixed with explicit keywords are just as dangerous.
    #[test]
    fn mixed_keyword_and_dict_splat_args_left_untouched() {
        let input = "from openai import OpenAI\nclient = OpenAI(api_key=key, **cfg)\n";
        let (result, out) = transform_python_result(input);
        assert!(!result.modified);
        assert_eq!(result.needs_manual_routing, 1);
        assert_eq!(out, input);
    }

    /// `*args` forwarding is also dynamic — leave it alone.
    #[test]
    fn list_splat_args_left_untouched_and_reported() {
        let input = "from openai import OpenAI\nclient = OpenAI(*args)\n";
        let (result, out) = transform_python_result(input);
        assert!(!result.modified);
        assert_eq!(result.needs_manual_routing, 1);
        assert_eq!(out, input);
    }

    /// A splat NEXT TO an explicit `base_url=` is already configured: nothing
    /// to do, and it must NOT be counted as needing manual routing.
    #[test]
    fn dict_splat_with_explicit_base_url_not_counted_as_manual() {
        let input =
            "from openai import OpenAI\nclient = OpenAI(**cfg, base_url=\"https://x.example\")\n";
        let (result, out) = transform_python_result(input);
        assert!(!result.modified);
        assert_eq!(
            result.needs_manual_routing, 0,
            "already-configured calls are not manual-routing work"
        );
        assert_eq!(out, input);
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
                false,
            )
            .unwrap();

        assert!(!result.modified, "bedrock must be detect-only");
        let out = fs::read_to_string(&path).unwrap();
        assert_eq!(out, input, "boto3 calls must be byte-for-byte untouched");
    }

    /// Gemini is detect-only: `genai.Client.__init__` has NO `base_url` param
    /// (verified against `google-genai` — injecting one raised `TypeError` at
    /// runtime), so the transformer must leave `genai.Client(...)` calls
    /// byte-for-byte unchanged (mirrors the Bedrock detect-only test).
    #[test]
    fn gemini_transform_is_a_noop() {
        let input = "import google.genai as genai\nclient = genai.Client(api_key=key)\n";

        let dir = TempDir::new().unwrap();
        let path = dir.path().join("gem.py");
        fs::write(&path, input).unwrap();
        let result = PythonTransformer::new()
            .transform_file(
                &path,
                Provider::Gemini,
                "https://api.promptguard.co/api/v1",
                "PROMPTGUARD_API_KEY",
                false,
            )
            .unwrap();

        assert!(!result.modified, "gemini must be detect-only");
        let out = fs::read_to_string(&path).unwrap();
        assert_eq!(
            out, input,
            "genai.Client(...) must be byte-for-byte untouched"
        );
    }

    /// Cohere IS transformable: `cohere.Client` / `cohere.ClientV2` both accept
    /// a valid `base_url=` kwarg. The real classes are `cohere.Client` and
    /// `cohere.ClientV2` (there is NO `CohereClient`), matched module-qualified.
    #[test]
    fn cohere_client_and_clientv2_are_transformed() {
        for call in ["cohere.Client(api_key=key)", "cohere.ClientV2(api_key=key)"] {
            let input = format!("import cohere\nclient = {call}\n");
            let dir = TempDir::new().unwrap();
            let path = dir.path().join("co.py");
            fs::write(&path, &input).unwrap();
            let result = PythonTransformer::new()
                .transform_file(
                    &path,
                    Provider::Cohere,
                    "https://api.promptguard.co/api/v1",
                    "PROMPTGUARD_API_KEY",
                    false,
                )
                .unwrap();
            assert!(result.modified, "{call} must be transformed");
            let out = fs::read_to_string(&path).unwrap();
            assert!(
                out.contains("base_url=\"https://api.promptguard.co/api/v1\""),
                "{call} must get base_url injected:\n{out}"
            );
            assert_reparses_clean(&out);
        }
    }

    /// Regression: attribute-form calls (`openai.OpenAI(...)`) were detected
    /// but the transform query matched only bare identifiers, so the file
    /// was silently reported "(no changes needed)" and never routed through
    /// the proxy.
    #[test]
    fn module_qualified_attribute_call_is_transformed() {
        let input = "import openai\nclient = openai.OpenAI(api_key=key)\n";
        let out = transform_python(input);
        assert!(
            out.contains("base_url=\"https://api.promptguard.co/api/v1\""),
            "openai.OpenAI(...) must be transformed:\n{out}"
        );
        assert!(
            out.contains("openai.OpenAI("),
            "call form preserved:\n{out}"
        );
        assert_reparses_clean(&out);
    }

    /// The attribute pattern is constrained to the SDK module: some other
    /// module's `OpenAI` attribute must NOT be rewritten.
    #[test]
    fn unrelated_module_attribute_call_is_not_transformed() {
        let input = "import mymod\nclient = mymod.OpenAI(api_key=key)\n";
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("client.py");
        fs::write(&path, input).unwrap();
        let result = PythonTransformer::new()
            .transform_file(
                &path,
                Provider::OpenAI,
                "https://api.promptguard.co/api/v1",
                "PROMPTGUARD_API_KEY",
                false,
            )
            .unwrap();
        assert!(
            !result.modified,
            "mymod.OpenAI(...) must not be rewritten as if it were the SDK"
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), input);
    }

    /// Async client constructors (`AsyncOpenAI`) accept `base_url` too and must
    /// be transformed — previously they were neither detected nor rewritten.
    #[test]
    fn async_client_constructor_is_transformed() {
        let input = "from openai import AsyncOpenAI\nclient = AsyncOpenAI(api_key=key)\n";
        let out = transform_python(input);
        assert!(
            out.contains("base_url=\"https://api.promptguard.co/api/v1\""),
            "AsyncOpenAI(...) must be transformed:\n{out}"
        );
        assert_reparses_clean(&out);
    }

    /// Module-qualified async constructor calls are transformed as well.
    #[test]
    fn module_qualified_async_client_is_transformed() {
        let input = "import openai\nclient = openai.AsyncOpenAI(api_key=key)\n";
        let out = transform_python(input);
        assert!(
            out.contains("base_url=\"https://api.promptguard.co/api/v1\""),
            "openai.AsyncOpenAI(...) must be transformed:\n{out}"
        );
        assert_reparses_clean(&out);
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
