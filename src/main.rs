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
    KeyCommand, LogsCommand, RevertCommand, ScanCommand, StatusCommand, TestCommand, UpdateCommand,
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

    /// Scan project for LLM SDK usage without making changes
    ///
    /// Detects `OpenAI`, Anthropic, Cohere, and `HuggingFace` SDK usage
    /// in your Python and TypeScript/JavaScript files.
    Scan {
        /// Filter by specific provider
        #[arg(long)]
        provider: Option<String>,

        /// Output results as JSON (for scripting)
        #[arg(long)]
        json: bool,
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

    /// View activity logs from `PromptGuard` dashboard
    ///
    /// Shows recent LLM requests, security events, and usage metrics.
    Logs,

    /// Test `PromptGuard` configuration
    ///
    /// Validates API key, tests proxy connectivity, and verifies
    /// that your setup is working correctly.
    Test,

    /// Update CLI to the latest version
    Update,
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

        Commands::Scan { provider, json } => ScanCommand { provider, json }.execute(),

        Commands::Status { json } => StatusCommand { json }.execute(),

        Commands::Doctor => DoctorCommand::execute(),

        Commands::Apply { yes } => ApplyCommand { yes }.execute(),

        Commands::Revert { yes } => RevertCommand { yes }.execute(),

        Commands::Disable => DisableCommand::execute(),
        Commands::Enable { runtime } => EnableCommand { runtime }.execute(),
        Commands::Config => ConfigCommand::execute(),
        Commands::Key => KeyCommand::execute(),
        Commands::Logs => LogsCommand::execute(),
        Commands::Test => TestCommand::execute(),
        Commands::Update => UpdateCommand::execute(),
    };

    if let Err(e) = result {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}
