// Clippy configuration for CLI binary
#![allow(clippy::exit)] // process::exit is appropriate in main.rs
#![allow(clippy::trivially_copy_pass_by_ref)] // &self for Copy types maintains API consistency
#![allow(clippy::if_not_else)] // Negative conditions often clearer in CLI code
#![allow(clippy::assigning_clones)] // Micro-optimization, not worth the noise
#![allow(clippy::too_many_lines)] // Some CLI commands are legitimately complex
#![allow(clippy::unnecessary_wraps)] // Consistent Result returns across commands
#![allow(clippy::unused_self)] // Methods may need self for future extension
#![allow(clippy::manual_let_else)] // Match syntax can be clearer

mod analyzer;
mod api;
mod backup;
mod commands;
mod config;
mod detector;
mod env;
mod error;
mod output;
mod scanner;
mod shim;
mod transformer;
mod types;

use clap::{Parser, Subcommand};
use commands::{
    ApplyCommand, ConfigCommand, DisableCommand, DoctorCommand, EnableCommand, InitCommand,
    KeyCommand, LogsCommand, RedTeamCommand, RedactCommand, RevertCommand, ScanCommand,
    StatusCommand, TestCommand, UpdateCommand,
};

#[derive(Parser)]
#[command(name = "promptguard")]
#[command(about = "Drop-in LLM security for your applications", long_about = None)]
#[command(version)]
struct Cli {
    /// Increase output verbosity (can be repeated: -v, -vv, -vvv)
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    verbose: u8,

    /// Suppress non-essential output
    #[arg(short, long, global = true)]
    quiet: bool,

    /// Disable colored output (also respects `NO_COLOR` env var)
    #[arg(long, global = true)]
    no_color: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize `PromptGuard` in this project
    ///
    /// Scans for LLM SDK usage (`OpenAI`, Anthropic, etc.) and configures
    /// your project to route requests through the `PromptGuard` proxy.
    Init {
        /// Target specific providers (e.g., openai, anthropic). Default: all detected
        #[arg(long)]
        provider: Vec<String>,

        /// `PromptGuard` API key (or set `PROMPTGUARD_API_KEY` env var)
        #[arg(long)]
        api_key: Option<String>,

        /// Proxy URL to route LLM requests through
        #[arg(long, default_value = "https://api.promptguard.co/api/v1")]
        base_url: String,

        /// Environment file to store API key
        #[arg(long, default_value = ".env")]
        env_file: String,

        /// Skip confirmation prompts (for CI/CD)
        #[arg(short = 'y', long)]
        auto: bool,

        /// Preview changes without applying them
        #[arg(long)]
        dry_run: bool,

        /// Proceed without version control (not recommended)
        #[arg(long)]
        force: bool,

        /// Glob patterns for files to exclude from transformation
        #[arg(long)]
        exclude: Vec<String>,

        /// Override detected framework (nextjs, express, django, fastapi, flask)
        #[arg(long)]
        framework: Option<String>,
    },

    /// Scan project for LLM SDK usage or scan text for security threats
    ///
    /// Without --text or --file: Detects `OpenAI`, Anthropic, Cohere, and `HuggingFace` SDK usage
    /// in your Python and TypeScript/JavaScript files.
    ///
    /// With --text or --file: Scans content for security threats (prompt injection, jailbreaks, etc.)
    /// via the `PromptGuard` API.
    Scan {
        /// Filter by specific provider (for SDK detection mode)
        #[arg(long)]
        provider: Option<String>,

        /// Output results as JSON (for scripting)
        #[arg(long)]
        json: bool,

        /// Text content to scan for security threats via the API
        #[arg(long, conflicts_with = "file")]
        text: Option<String>,

        /// File path to scan for security threats via the API
        #[arg(long, conflicts_with = "text")]
        file: Option<String>,
    },

    /// Show current `PromptGuard` status and configuration
    ///
    /// Displays whether `PromptGuard` is active, which providers are configured,
    /// and details about the current setup.
    Status {
        /// Output as JSON (for scripting)
        #[arg(long)]
        json: bool,
    },

    /// Diagnose common configuration issues
    ///
    /// Checks API key validity, file permissions, security settings,
    /// and other common problems. Run this if something isn't working.
    Doctor,

    /// Re-apply `PromptGuard` transformations to source files
    ///
    /// Use this after modifying files manually or adding new SDK usage.
    Apply {
        /// Skip confirmation prompt
        #[arg(short = 'y', long)]
        yes: bool,
    },

    /// Temporarily disable `PromptGuard` (keeps configuration)
    ///
    /// LLM requests will go directly to providers until re-enabled.
    Disable,

    /// Re-enable `PromptGuard` after disabling
    ///
    /// Restores proxy routing for LLM requests.
    Enable {
        /// Use runtime shims for 100% SDK call coverage (recommended)
        #[arg(long)]
        runtime: bool,
    },

    /// Completely remove `PromptGuard` from this project
    ///
    /// Reverts all file changes and removes configuration.
    /// Use git to review changes before confirming.
    Revert {
        /// Skip confirmation prompt
        #[arg(short = 'y', long)]
        yes: bool,
    },

    /// View and manage `PromptGuard` configuration
    ///
    /// Shows current settings including providers, proxy URL,
    /// exclude patterns, and metadata.
    Config,

    /// Manage API keys
    ///
    /// View, update, or rotate your `PromptGuard` API key.
    /// Keys can be test (`pg_sk_test`_*) or production (`pg_sk_prod`_*).
    Key,

    /// View activity logs from `PromptGuard` API
    ///
    /// Fetches recent LLM requests, security events, and usage metrics
    /// directly from the `PromptGuard` backend.
    Logs {
        /// Number of log entries to fetch
        #[arg(short, long, default_value = "20")]
        limit: usize,

        /// Filter by log type (security, request, response, error)
        #[arg(short = 't', long = "type")]
        log_type: Option<String>,

        /// Output results as JSON (for scripting)
        #[arg(long)]
        json: bool,

        /// Continuously poll for new logs (not yet implemented)
        #[arg(short, long)]
        follow: bool,
    },

    /// Test `PromptGuard` configuration
    ///
    /// Validates API key, tests proxy connectivity, and verifies
    /// that your setup is working correctly.
    Test,

    /// Check for CLI updates
    ///
    /// Checks GitHub releases for a newer version and provides
    /// instructions for updating.
    Update {
        /// Only check for updates, don't install
        #[arg(long, default_value = "true")]
        check_only: bool,
    },

    /// Redact PII and sensitive data from text
    ///
    /// Calls the `PromptGuard` API to identify and redact sensitive information
    /// like emails, phone numbers, SSNs, credit cards, etc.
    Redact {
        /// Text content to redact
        #[arg(long, conflicts_with = "file")]
        text: Option<String>,

        /// File path to read and redact
        #[arg(long, conflicts_with = "text")]
        file: Option<String>,

        /// Output file path (if not provided, prints to stdout)
        #[arg(short, long)]
        output: Option<String>,

        /// Output results as JSON (for scripting)
        #[arg(long)]
        json: bool,
    },

    /// Run adversarial security tests against your AI application
    ///
    /// Uses `PromptGuard`'s Red Team API to evaluate security posture
    /// by testing with known attack patterns and jailbreak attempts.
    Redteam {
        /// Target API URL to test against
        #[arg(long)]
        target_url: Option<String>,

        /// `PromptGuard` API key (or uses configured key)
        #[arg(long)]
        api_key: Option<String>,

        /// Categories of attacks to test (e.g., jailbreak, injection)
        #[arg(long)]
        category: Vec<String>,

        /// Output format: human or json
        #[arg(long, default_value = "human")]
        format: String,

        /// Show detailed output for each test
        #[arg(short, long)]
        verbose: bool,

        /// Run a specific test by name
        #[arg(long)]
        test: Option<String>,

        /// Custom prompt to test
        #[arg(long)]
        prompt: Option<String>,

        /// Preset to use for testing (default, strict, permissive)
        #[arg(long, default_value = "default")]
        preset: String,
    },
}

fn main() {
    let cli = Cli::parse();

    // Initialize output settings based on global flags
    output::Output::init(
        cli.verbose,
        cli.quiet,
        cli.no_color || std::env::var("NO_COLOR").is_ok(),
    );

    let result = match cli.command {
        Commands::Init {
            provider,
            api_key,
            base_url,
            env_file,
            auto,
            dry_run,
            force,
            exclude,
            framework,
        } => InitCommand {
            provider,
            api_key,
            base_url,
            env_file,
            auto,
            dry_run,
            force,
            exclude,
            framework,
        }
        .execute(),

        Commands::Scan {
            provider,
            json,
            text,
            file,
        } => ScanCommand {
            provider,
            json,
            text,
            file,
        }
        .execute(),

        Commands::Status { json } => StatusCommand { json }.execute(),

        Commands::Doctor => DoctorCommand::execute(),

        Commands::Apply { yes } => ApplyCommand { yes }.execute(),

        Commands::Revert { yes } => RevertCommand { yes }.execute(),

        Commands::Disable => DisableCommand::execute(),
        Commands::Enable { runtime } => EnableCommand { runtime }.execute(),
        Commands::Config => ConfigCommand::execute(),
        Commands::Key => KeyCommand::execute(),
        Commands::Logs {
            limit,
            log_type,
            json,
            follow,
        } => LogsCommand {
            limit,
            log_type,
            json,
            follow,
        }
        .execute(),
        Commands::Test => TestCommand::execute(),
        Commands::Update { check_only } => UpdateCommand { check_only }.execute(),

        Commands::Redact {
            text,
            file,
            output,
            json,
        } => RedactCommand {
            text,
            file,
            output,
            json,
        }
        .execute(),

        Commands::Redteam {
            target_url,
            api_key,
            category,
            format,
            verbose,
            test,
            prompt,
            preset,
        } => RedTeamCommand {
            target_url,
            api_key,
            categories: category,
            output_format: format,
            verbose,
            test_name: test,
            custom_prompt: prompt,
            preset,
        }
        .execute(),
    };

    if let Err(e) = result {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}
