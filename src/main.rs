// Clippy configuration for CLI binary
#![allow(clippy::exit)] // process::exit is appropriate in main.rs
#![allow(clippy::expect_used)] // CLI startup should fail fast with clear messages
#![allow(clippy::trivially_copy_pass_by_ref)] // &self for Copy types maintains API consistency
#![allow(clippy::if_not_else)] // Negative conditions often clearer in CLI code
#![allow(clippy::assigning_clones)] // Micro-optimization, not worth the noise
#![allow(clippy::too_many_lines)] // Some CLI commands are legitimately complex
#![allow(clippy::unnecessary_wraps)] // Consistent Result returns across commands
#![allow(clippy::unused_self)] // Methods may need self for future extension
#![allow(clippy::manual_let_else)] // Match syntax can be clearer
#![allow(dead_code)] // Some public API fields/methods unused internally

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
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize `PromptGuard` in this project
    Init {
        #[arg(long)]
        provider: Vec<String>,

        #[arg(long)]
        api_key: Option<String>,

        #[arg(long, default_value = "https://api.promptguard.co/api/v1/proxy")]
        base_url: String,

        #[arg(long, default_value = ".env")]
        env_file: String,

        #[arg(short = 'y', long)]
        auto: bool,

        #[arg(long)]
        dry_run: bool,

        /// Proceed without version control (not recommended)
        #[arg(long)]
        force: bool,

        #[arg(long)]
        exclude: Vec<String>,

        #[arg(long)]
        framework: Option<String>,
    },

    /// Scan project for LLM SDK usage
    Scan {
        #[arg(long)]
        provider: Option<String>,

        #[arg(long)]
        json: bool,
    },

    /// Show current `PromptGuard` configuration
    Status {
        #[arg(long)]
        json: bool,
    },

    /// Diagnose common configuration issues
    Doctor,

    /// Apply pending configuration changes
    Apply {
        #[arg(short = 'y', long)]
        yes: bool,
    },

    /// Temporarily disable `PromptGuard`
    Disable,

    /// Re-enable `PromptGuard`
    Enable {
        /// Use runtime shims for 100% coverage
        #[arg(long)]
        runtime: bool,
    },

    /// Completely remove `PromptGuard`
    Revert {
        /// Skip confirmation prompt
        #[arg(short = 'y', long)]
        yes: bool,
    },

    /// Manage configuration
    Config,

    /// Manage API keys
    Key,

    /// View activity logs
    Logs,

    /// Test configuration
    Test,

    /// Update CLI to latest version
    Update,
}

fn main() {
    let cli = Cli::parse();

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
