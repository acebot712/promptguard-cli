use crate::error::{PromptGuardError, Result};
use colored::{ColoredString, Colorize};
use std::io::{self, Write};
use std::sync::OnceLock;

/// Global output configuration
static OUTPUT_CONFIG: OnceLock<OutputConfig> = OnceLock::new();

#[derive(Debug, Clone, Default)]
struct OutputConfig {
    verbose: u8,
    quiet: bool,
    no_color: bool,
}

pub struct Output;

impl Output {
    /// Initialize output settings (call once at startup)
    pub fn init(verbose: u8, quiet: bool, no_color: bool) {
        let config = OutputConfig {
            verbose,
            quiet,
            no_color,
        };
        let _ = OUTPUT_CONFIG.set(config);

        // Disable colors globally if requested
        if no_color {
            colored::control::set_override(false);
        }
    }

    fn config() -> &'static OutputConfig {
        OUTPUT_CONFIG.get_or_init(OutputConfig::default)
    }

    fn is_quiet() -> bool {
        Self::config().quiet
    }

    fn verbosity() -> u8 {
        Self::config().verbose
    }

    /// Apply color only if colors are enabled
    fn colorize(text: &str, color_fn: impl FnOnce(&str) -> ColoredString) -> String {
        if Self::config().no_color {
            text.to_string()
        } else {
            color_fn(text).to_string()
        }
    }

    pub fn header(text: &str) {
        if Self::is_quiet() {
            return;
        }
        let colored_text = Self::colorize(text, |s| s.cyan().bold());
        let separator = Self::colorize(&"=".repeat(50), |s| s.cyan());
        println!("\n{colored_text}");
        println!("{separator}");
    }

    pub fn section(title: &str, icon: &str) {
        if Self::is_quiet() {
            return;
        }
        let bold_title = Self::colorize(title, |s| s.bold());
        println!("\n{icon} {bold_title}");
    }

    pub fn success(message: &str) {
        let check = Self::colorize("✓", |s| s.green().bold());
        let msg = Self::colorize(message, |s| s.green());
        println!("{check} {msg}");
    }

    pub fn error(message: &str) {
        let x_mark = Self::colorize("✗", |s| s.red().bold());
        let msg = Self::colorize(message, |s| s.red());
        eprintln!("{x_mark} {msg}");
    }

    pub fn warning(message: &str) {
        let warn = Self::colorize("⚠", |s| s.yellow().bold());
        let msg = Self::colorize(message, |s| s.yellow());
        println!("{warn} {msg}");
    }

    pub fn info(message: &str) {
        if Self::is_quiet() {
            return;
        }
        let info = Self::colorize("ℹ", |s| s.blue().bold());
        println!("{info} {message}");
    }

    pub fn step(message: &str) {
        if Self::is_quiet() {
            return;
        }
        let bullet = Self::colorize("•", |s| s.bright_black());
        println!("  {bullet} {message}");
    }

    pub fn excluded(message: &str) {
        if Self::is_quiet() || Self::verbosity() == 0 {
            return;
        }
        let circle = Self::colorize("○", |s| s.bright_black());
        let msg = Self::colorize(message, |s| s.bright_black());
        println!("  {circle} {msg}");
    }

    /// Mask an API key for display, revealing only the non-secret class
    /// prefix and the last 4 characters (e.g. `pg_live_…a1b2`).
    ///
    /// The `pg_live_` segment is a public key-class marker, not part of the
    /// secret, so it is safe to show. Everything between the prefix and the
    /// trailing 4 characters is hidden.
    pub fn mask_api_key(key: &str) -> String {
        const SUFFIX_LEN: usize = 4;

        // Show the recognised key-class prefix if present; otherwise fall
        // back to the generic `pg_` marker (or nothing for unknown keys).
        let prefix = if key.starts_with("pg_live_") {
            "pg_live_"
        } else if key.starts_with("pg_") {
            "pg_"
        } else {
            ""
        };

        // Count by characters, not bytes: byte-slicing a non-ASCII key on a
        // non-char-boundary panics. `prefix` is ASCII so `prefix.len()` is its
        // char count.
        let total_chars = key.chars().count();
        let prefix_chars = prefix.len();

        // If the key is too short to mask meaningfully, fully redact it
        // (one '*' per character).
        if total_chars <= prefix_chars + SUFFIX_LEN {
            return "*".repeat(total_chars);
        }

        // Take the last SUFFIX_LEN characters in order, char-boundary-safe.
        // Never reveals more than the prefix + last 4 characters.
        let mut tail: Vec<char> = key.chars().rev().take(SUFFIX_LEN).collect();
        tail.reverse();
        let suffix: String = tail.into_iter().collect();
        format!("{prefix}…{suffix}")
    }

    /// Truncate `s` to at most `max_chars` characters, char-boundary safe.
    ///
    /// Byte-index slicing (`&s[..n]`) panics on multi-byte UTF-8 content;
    /// use this for any display truncation of user- or API-provided text.
    pub fn truncate_chars(s: &str, max_chars: usize) -> &str {
        match s.char_indices().nth(max_chars) {
            Some((byte_idx, _)) => &s[..byte_idx],
            None => s,
        }
    }

    /// Ask a yes/no question and return the answer.
    ///
    /// Non-interactive safety: when stdin is not a TTY (piped, redirected, or
    /// closed — e.g. spawned by an editor extension or CI), or when
    /// `read_line` hits EOF, this falls through to `default` instead of
    /// blocking forever on input that will never arrive. Callers that need a
    /// scripted "yes" should pass their command's `--yes` flag rather than
    /// piping input.
    pub fn confirm(prompt: &str, default: bool) -> Result<bool> {
        use std::io::IsTerminal;

        let default_str = if default { "Y/n" } else { "y/N" };
        let bold_prompt = Self::colorize(prompt, |s| s.bold());

        if !io::stdin().is_terminal() {
            let answer = if default { "yes" } else { "no" };
            println!(
                "{bold_prompt} [{default_str}]: {answer} (stdin is not interactive; using default)"
            );
            return Ok(default);
        }

        print!("{bold_prompt} [{default_str}]: ");
        io::stdout().flush().map_err(PromptGuardError::Io)?;

        let mut input = String::new();
        let bytes_read = io::stdin()
            .read_line(&mut input)
            .map_err(PromptGuardError::Io)?;

        // EOF (Ctrl-D / stream closed): no answer is coming.
        if bytes_read == 0 {
            println!();
            return Ok(default);
        }

        let input = input.trim().to_lowercase();

        if input.is_empty() {
            return Ok(default);
        }

        Ok(matches!(input.as_str(), "y" | "yes"))
    }

    pub fn input(prompt: &str) -> Result<String> {
        let bold_prompt = Self::colorize(prompt, |s| s.bold());
        print!("{bold_prompt}: ");
        io::stdout().flush().map_err(PromptGuardError::Io)?;

        let mut input = String::new();
        io::stdin()
            .read_line(&mut input)
            .map_err(PromptGuardError::Io)?;

        Ok(input.trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mask_reveals_prefix_and_last_four() {
        let masked = Output::mask_api_key("pg_live_abcdefghijklmnop1234");
        assert_eq!(masked, "pg_live_…1234");
    }

    #[test]
    fn mask_generic_pg_prefix() {
        let masked = Output::mask_api_key("pg_secrettokenvalue9876");
        assert_eq!(masked, "pg_…9876");
    }

    #[test]
    fn mask_unknown_prefix_has_no_class_marker() {
        let masked = Output::mask_api_key("sk-someothertoken5678");
        assert_eq!(masked, "…5678");
    }

    #[test]
    fn mask_short_key_is_fully_redacted() {
        // Shorter than prefix + 4 suffix chars: never reveal anything.
        let masked = Output::mask_api_key("pg_live_");
        assert_eq!(masked, "********");
        assert!(!masked.contains("pg_live_"));

        let masked = Output::mask_api_key("pg_ab");
        assert_eq!(masked, "*****");
    }

    #[test]
    fn mask_non_ascii_key_does_not_panic() {
        // A corrupted / non-ASCII key must not panic on byte-slicing.
        // Multi-byte chars at the suffix boundary are the failure case the
        // old `&key[len-4..]` implementation would hit.
        let masked = Output::mask_api_key("pg_live_corrupted_кириллица");
        // Last 4 *characters* are "лица"; prefix is recognised and shown.
        assert_eq!(masked, "pg_live_…лица");

        // Emoji (4-byte chars) at the boundary.
        let masked = Output::mask_api_key("pg_live_xxxx🔑🔑🔑🔑");
        assert_eq!(masked, "pg_live_…🔑🔑🔑🔑");
    }

    #[test]
    fn truncate_chars_is_char_boundary_safe() {
        // ASCII: plain prefix
        assert_eq!(Output::truncate_chars("hello world", 5), "hello");
        // Shorter than the limit: unchanged
        assert_eq!(Output::truncate_chars("hi", 60), "hi");
        // Multi-byte characters at the boundary must not panic
        assert_eq!(Output::truncate_chars("🚀🚀🚀🚀", 2), "🚀🚀");
        assert_eq!(Output::truncate_chars("héllo wörld", 4), "héll");
        // Emoji mixed with ASCII around the cut point
        let s = "attack 🎯 prompt with émojis 🚀 and more text";
        let t = Output::truncate_chars(s, 10);
        assert_eq!(t.chars().count(), 10);
        assert!(s.starts_with(t));
    }

    #[test]
    fn mask_short_non_ascii_key_is_fully_redacted_by_char_count() {
        // 4 characters total (all multi-byte), no recognised prefix: too
        // short to mask meaningfully (<= 0 prefix + 4 suffix), so it must be
        // fully redacted with one '*' per *character*, not per byte.
        let masked = Output::mask_api_key("αβγδ");
        assert_eq!(masked, "****");
    }
}
