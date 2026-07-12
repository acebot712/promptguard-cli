//! Line-ending-preserving text helpers.
//!
//! Rewrites that go through `str::lines()` and rejoin with a hard-coded
//! `"\n"` silently convert CRLF files to LF and churn the trailing-newline
//! state. These helpers detect the source file's dominant line ending and
//! whether it ended with a newline, so a rewrite round-trips byte-for-byte on
//! CRLF and LF alike.

/// The dominant line ending of `source`: `"\r\n"` if CRLF pairs outnumber
/// bare LFs, otherwise `"\n"`. Empty or newline-free input defaults to LF.
pub fn dominant_line_ending(source: &str) -> &'static str {
    let crlf = source.matches("\r\n").count();
    let total_lf = source.matches('\n').count();
    let bare_lf = total_lf.saturating_sub(crlf);
    if crlf > bare_lf {
        "\r\n"
    } else {
        "\n"
    }
}

/// Whether `source` ends with a line terminator (`\n`, and by extension
/// `\r\n`).
pub fn has_trailing_newline(source: &str) -> bool {
    source.ends_with('\n')
}

/// Join `lines` using the dominant line ending of `original`, re-adding a
/// trailing line ending only if `original` had one.
///
/// `lines` must be line *content* with no embedded terminators (as produced
/// by [`str::lines`]). This is the inverse of `original.lines()` that
/// preserves CRLF/LF and the final-newline state.
pub fn join_preserving_style<S: AsRef<str>>(lines: &[S], original: &str) -> String {
    let eol = dominant_line_ending(original);
    let mut out = String::with_capacity(original.len() + 16);
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            out.push_str(eol);
        }
        out.push_str(line.as_ref());
    }
    if has_trailing_newline(original) {
        out.push_str(eol);
    }
    out
}

/// Rewrite every line ending in `content` to `eol`, preserving whether
/// `content` ended with a newline.
///
/// Used to re-normalize a rewritten file whose in-place edits introduced LF
/// snippets, so the whole output keeps the source file's original ending.
pub fn reencode_line_endings(content: &str, eol: &str) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let mut out = String::with_capacity(content.len() + 8);
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            out.push_str(eol);
        }
        out.push_str(line);
    }
    if has_trailing_newline(content) {
        out.push_str(eol);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reencodes_mixed_endings_to_target() {
        // A rewrite that spliced LF into a CRLF file: renormalize to CRLF.
        let mixed = "a\r\nb\nc\nd\r\n";
        assert_eq!(reencode_line_endings(mixed, "\r\n"), "a\r\nb\r\nc\r\nd\r\n");
    }

    #[test]
    fn detects_dominant_ending() {
        assert_eq!(dominant_line_ending("a\nb\n"), "\n");
        assert_eq!(dominant_line_ending("a\r\nb\r\n"), "\r\n");
        assert_eq!(dominant_line_ending(""), "\n");
        assert_eq!(dominant_line_ending("no newline"), "\n");
        // Mixed: majority wins.
        assert_eq!(dominant_line_ending("a\r\nb\r\nc\n"), "\r\n");
    }

    #[test]
    fn preserves_crlf_and_trailing_newline() {
        let original = "a\r\nb\r\nc\r\n";
        let lines: Vec<&str> = original.lines().collect();
        assert_eq!(join_preserving_style(&lines, original), original);
    }

    #[test]
    fn preserves_lf_without_trailing_newline() {
        let original = "a\nb\nc";
        let lines: Vec<&str> = original.lines().collect();
        assert_eq!(join_preserving_style(&lines, original), original);
    }

    #[test]
    fn inserted_line_uses_source_ending() {
        let original = "first\r\nsecond\r\n";
        let mut lines: Vec<String> = original.lines().map(str::to_string).collect();
        lines.insert(1, "inserted".to_string());
        assert_eq!(
            join_preserving_style(&lines, original),
            "first\r\ninserted\r\nsecond\r\n"
        );
    }
}
