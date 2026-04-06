// Clippy configuration for CLI binary
#![allow(clippy::exit)]
#![allow(clippy::trivially_copy_pass_by_ref)]
#![allow(clippy::if_not_else)]
#![allow(clippy::assigning_clones)]
#![allow(clippy::too_many_lines)]
#![allow(clippy::manual_let_else)]
#![allow(clippy::unused_self)]
#![allow(clippy::unnecessary_wraps)]

mod analyzer;
mod api;
mod auth;
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
    ApplyCommand, ConfigCommand, DashboardCommand, DisableCommand, DoctorCommand, EnableCommand,
    EventsCommand, InitCommand, KeyCommand, LoginCommand, LogoutCommand, LogsCommand, McpCommand,
    PolicyAction, PolicyCommand, ProjectsAction, ProjectsCommand, RedTeamCommand, RedactCommand,
    RevertCommand, ScanCommand, StatusCommand, TestCommand, UpdateCommand, WhoamiCommand,
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
    /// Without --text or --file: Detects `OpenAI`, Anthropic, Cohere, `HuggingFace`, Gemini, Groq, and AWS Bedrock SDK usage
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
    Doctor {
        /// Output as JSON (for scripting)
        #[arg(long)]
        json: bool,
    },

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
    Config {
        /// Output as JSON (for scripting)
        #[arg(long)]
        json: bool,
    },

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
    Update,

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

        /// Run the autonomous red team agent (LLM-powered mutation)
        #[arg(long)]
        autonomous: bool,

        /// Max iterations for autonomous mode (1-1000)
        #[arg(long, default_value = "100")]
        budget: u32,
    },

    /// Manage guardrail policies as YAML files (policy-as-code)
    ///
    /// Define guardrails in YAML, version in git, and apply via CLI.
    /// Supports apply, diff, and export operations.
    Policy {
        /// Action to perform: apply, diff, or export
        #[command(subcommand)]
        action: PolicySubcommand,

        /// Project ID to manage policies for
        #[arg(long, global = true)]
        project_id: String,

        /// `PromptGuard` API key (or uses configured key)
        #[arg(long, global = true)]
        api_key: Option<String>,

        /// API base URL
        #[arg(long, global = true)]
        base_url: Option<String>,
    },

    /// Start MCP (Model Context Protocol) server for IDE integration
    ///
    /// Exposes `PromptGuard` tools over the MCP protocol so AI-powered
    /// editors (Cursor, Claude Code, Windsurf, etc.) can call them.
    Mcp {
        /// Transport type (currently only 'stdio' is supported)
        #[arg(short, long, default_value = "stdio")]
        transport: String,
    },

    /// Authenticate with `PromptGuard` and store credentials globally
    ///
    /// Saves your API key to `~/.promptguard/credentials.json` so all
    /// commands and projects can use it without per-project setup.
    Login {
        /// API key to authenticate with (or enter interactively)
        #[arg(long)]
        api_key: Option<String>,

        /// Custom API base URL
        #[arg(long)]
        base_url: Option<String>,

        /// Output results as JSON
        #[arg(long)]
        json: bool,
    },

    /// Remove stored `PromptGuard` credentials
    ///
    /// Deletes `~/.promptguard/credentials.json`.
    Logout {
        /// Output results as JSON
        #[arg(long)]
        json: bool,
    },

    /// Show current authentication status
    ///
    /// Displays which API key is active, its source (env, project, global),
    /// and whether the API is reachable.
    Whoami {
        /// Output results as JSON
        #[arg(long)]
        json: bool,
    },

    /// Manage `PromptGuard` projects
    ///
    /// List, select, and view projects associated with your account.
    Projects {
        #[command(subcommand)]
        action: ProjectsSubcommand,

        /// Output results as JSON
        #[arg(long, global = true)]
        json: bool,
    },

    /// View recent security events
    ///
    /// Lists security events (blocks, alerts, redactions) from the
    /// `PromptGuard` API. Useful for monitoring and auditing.
    Events {
        /// Number of events to fetch
        #[arg(short, long, default_value = "20")]
        limit: usize,

        /// Filter by event type
        #[arg(short = 't', long = "type")]
        event_type: Option<String>,

        /// Output results as JSON
        #[arg(long)]
        json: bool,
    },

    /// Open the `PromptGuard` dashboard in your browser
    Dashboard {
        /// Output the URL as JSON instead of opening browser
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum ProjectsSubcommand {
    /// List all projects
    List,

    /// Set the active project
    Select {
        /// Project ID to select
        project_id: String,
    },
}

#[derive(Subcommand)]
enum PolicySubcommand {
    /// Apply a YAML policy file to the project
    Apply {
        /// Path to the YAML policy file
        file: String,

        /// Preview changes without applying
        #[arg(long)]
        dry_run: bool,
    },

    /// Show differences between a YAML file and the live config
    Diff {
        /// Path to the YAML policy file
        file: String,
    },

    /// Export the current live config as YAML (to stdout)
    Export,
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

        Commands::Doctor { json } => DoctorCommand { json }.execute(),

        Commands::Apply { yes } => ApplyCommand { yes }.execute(),

        Commands::Revert { yes } => RevertCommand { yes }.execute(),

        Commands::Disable => DisableCommand::execute(),
        Commands::Enable { runtime } => EnableCommand { runtime }.execute(),
        Commands::Config { json } => ConfigCommand { json }.execute(),
        Commands::Key => KeyCommand::execute(),
        Commands::Logs {
            limit,
            log_type,
            json,
        } => LogsCommand {
            limit,
            log_type,
            json,
        }
        .execute(),
        Commands::Test => TestCommand::execute(),
        Commands::Update => UpdateCommand.execute(),

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
            format,
            verbose,
            test,
            prompt,
            preset,
            autonomous,
            budget,
        } => RedTeamCommand {
            target_url,
            api_key,
            output_format: format,
            verbose,
            test_name: test,
            custom_prompt: prompt,
            preset,
            autonomous,
            budget,
        }
        .execute(),

        Commands::Policy {
            action,
            project_id,
            api_key,
            base_url,
        } => {
            let policy_action = match action {
                PolicySubcommand::Apply { file, dry_run } => PolicyAction::Apply { file, dry_run },
                PolicySubcommand::Diff { file } => PolicyAction::Diff { file },
                PolicySubcommand::Export => PolicyAction::Export,
            };
            PolicyCommand {
                action: policy_action,
                project_id,
                api_key,
                base_url,
            }
            .execute()
        },

        Commands::Mcp { transport } => McpCommand { transport }.execute(),

        Commands::Login {
            api_key,
            base_url,
            json,
        } => LoginCommand {
            api_key,
            base_url,
            json,
        }
        .execute(),

        Commands::Logout { json } => LogoutCommand { json }.execute(),

        Commands::Whoami { json } => WhoamiCommand { json }.execute(),

        Commands::Projects { action, json } => {
            let projects_action = match action {
                ProjectsSubcommand::List => ProjectsAction::List,
                ProjectsSubcommand::Select { project_id } => ProjectsAction::Select { project_id },
            };
            ProjectsCommand {
                action: projects_action,
                json,
            }
            .execute()
        },

        Commands::Events {
            limit,
            event_type,
            json,
        } => EventsCommand {
            limit,
            event_type,
            json,
        }
        .execute(),

        Commands::Dashboard { json } => DashboardCommand { json }.execute(),
    };

    if let Err(e) = result {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}
