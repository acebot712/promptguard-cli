use colored::Colorize;
use std::io::{self, Write};

pub struct Output;

impl Output {
    pub fn header(text: &str) {
        println!("\n{}", text.cyan().bold());
        println!("{}", "=".repeat(50).cyan());
    }

    pub fn section(title: &str, icon: &str) {
        println!("\n{} {}", icon, title.bold());
    }

    pub fn success(message: &str) {
        println!("{} {}", "✓".green().bold(), message.green());
    }

    pub fn error(message: &str) {
        eprintln!("{} {}", "✗".red().bold(), message.red());
    }

    pub fn warning(message: &str) {
        println!("{} {}", "⚠".yellow().bold(), message.yellow());
    }

    pub fn info(message: &str) {
        println!("{} {}", "ℹ".blue().bold(), message);
    }

    pub fn step(message: &str) {
        println!("  {} {}", "•".bright_black(), message);
    }

    pub fn excluded(message: &str) {
        println!("  {} {}", "○".bright_black(), message.bright_black());
    }

    pub fn mask_api_key(key: &str) -> String {
        if key.len() <= 12 {
            return "*".repeat(key.len());
        }

        let prefix = &key[..12];
        let masked_part = "*".repeat(key.len() - 12);
        format!("{prefix}{masked_part}")
    }

    pub fn confirm(prompt: &str, default: bool) -> bool {
        let default_str = if default { "Y/n" } else { "y/N" };
        print!("{} [{}]: ", prompt.bold(), default_str);
        io::stdout().flush().expect("Failed to flush stdout");

        let mut input = String::new();
        io::stdin()
            .read_line(&mut input)
            .expect("Failed to read user input");

        let input = input.trim().to_lowercase();

        if input.is_empty() {
            return default;
        }

        matches!(input.as_str(), "y" | "yes")
    }

    pub fn input(prompt: &str) -> String {
        print!("{}: ", prompt.bold());
        io::stdout().flush().expect("Failed to flush stdout");

        let mut input = String::new();
        io::stdin()
            .read_line(&mut input)
            .expect("Failed to read user input");

        input.trim().to_string()
    }
}
