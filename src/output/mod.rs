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

    /// Print debug information (only with -v or higher)
    /// This is a public API for verbose logging in commands.
    #[allow(dead_code)]
    pub fn debug(message: &str) {
        if Self::verbosity() >= 1 {
            let prefix = Self::colorize("[DEBUG]", |s| s.bright_black());
            println!("{prefix} {message}");
        }
    }

    /// Print trace information (only with -vv or higher)
    /// This is a public API for verbose logging in commands.
    #[allow(dead_code)]
    pub fn trace(message: &str) {
        if Self::verbosity() >= 2 {
            let prefix = Self::colorize("[TRACE]", |s| s.bright_black());
            println!("{prefix} {message}");
        }
    }

    pub fn mask_api_key(key: &str) -> String {
        if key.len() <= 12 {
            return "*".repeat(key.len());
        }

        let prefix = &key[..12];
        let masked_part = "*".repeat(key.len() - 12);
        format!("{prefix}{masked_part}")
    }

    pub fn confirm(prompt: &str, default: bool) -> Result<bool> {
        let default_str = if default { "Y/n" } else { "y/N" };
        let bold_prompt = Self::colorize(prompt, |s| s.bold());
        print!("{bold_prompt} [{default_str}]: ");
        io::stdout().flush().map_err(PromptGuardError::Io)?;

        let mut input = String::new();
        io::stdin()
            .read_line(&mut input)
            .map_err(PromptGuardError::Io)?;

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
