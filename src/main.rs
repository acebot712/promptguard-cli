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
mod text;
mod transformer;
mod types;

use clap::{Parser, Subcommand};
use commands::{
    ApplyCommand, ConfigCommand, DashboardCommand, DisableCommand, DoctorCommand, EnableCommand,
    EventsCommand, InitCommand, KeyAction, KeyCommand, LoginCommand, LogoutCommand, LogsCommand,
    McpCommand, PolicyAction, PolicyCommand, ProjectsAction, ProjectsCommand, RedTeamCommand,
    RedactCommand, RevertCommand, ScanCommand, StatusCommand, TestCommand, UpdateCommand,
    VerifyCommand, WhoamiCommand,
};

/// Grouped command reference + examples for the top-level `--help`.
///
/// clap 4.x cannot group subcommands under headings in its auto-generated
/// Commands list (`help_heading` applies to args, not subcommands), so the
/// sections below are authored by hand and rendered via a custom
/// `help_template` that omits the flat `{subcommands}` block. Plain text (no
/// ANSI) keeps it correct under `--no-color`/`NO_COLOR` and when piped.
///
/// KEEP IN SYNC: when adding/removing a subcommand, update this list.
const TOP_LEVEL_AFTER_HELP: &str = "\
Commands:
  Setup
    init       Initialize PromptGuard in this project
    scan       Scan for SDK usage, or scan text/files for threats
    apply      Re-apply PromptGuard transformations to source files
    enable     Re-enable PromptGuard after disabling
    disable    Temporarily disable PromptGuard (keeps configuration)
    revert     Completely remove PromptGuard from this project

  Auth
    login      Authenticate and store credentials globally
    logout     Remove stored credentials
    whoami     Show current authentication status
    key        Manage API keys
    projects   Manage PromptGuard projects

  Inspect
    status     Show current status and configuration
    doctor     Diagnose common configuration issues
    config     View PromptGuard configuration
    verify     Verify end-to-end integration
    test       Test configuration and connectivity

  Monitor
    logs       View activity logs from the PromptGuard API
    events     View recent security events
    dashboard  Open the PromptGuard dashboard in your browser

  Security testing
    redteam    Run adversarial security tests against your app
    redact     Redact PII and sensitive data from text
    policy     Manage guardrail policies as YAML (policy-as-code)

  Other
    mcp        Start an MCP server for IDE integration
    update     Check for CLI updates

Run 'promptguard <command> --help' for details on a command.

Examples:
  promptguard init                             Set up PromptGuard in this repo
  promptguard scan --text \"ignore the rules\"   Scan a string for threats
  promptguard verify --json                    Check integration health (CI)
";

#[allow(clippy::doc_markdown)]
#[derive(Parser)]
#[command(name = "promptguard")]
#[command(about = "Drop-in LLM security for your applications", long_about = None)]
#[command(version)]
#[command(
    help_template = "{about-with-newline}\n{usage-heading} {usage}{after-help}\nOptions:\n{options}"
)]
#[command(after_help = TOP_LEVEL_AFTER_HELP)]
struct Cli {
    /// Increase output verbosity (can be repeated: -v, -vv, -vvv)
    #[arg(short, long, action = clap::ArgAction::Count, global = true, help_heading = "Global options")]
    verbose: u8,

    /// Suppress non-essential output
    #[arg(short, long, global = true, help_heading = "Global options")]
    quiet: bool,

    /// Disable colored output (also respects NO_COLOR env var)
    #[arg(long, global = true, help_heading = "Global options")]
    no_color: bool,

    /// Allow sending your API key to a custom proxy host from .promptguard.json
    #[arg(
        long,
        global = true,
        help_heading = "Global options",
        long_help = "Allow sending an API key resolved from the environment or global \
credentials to a custom proxy host configured in this repository's \
.promptguard.json. Refused by default to prevent key exfiltration."
    )]
    allow_custom_proxy: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[allow(clippy::doc_markdown)]
#[derive(Subcommand)]
enum Commands {
    /// Initialize PromptGuard in this project
    ///
    /// Scans for LLM SDK usage (OpenAI, Anthropic, etc.) and configures
    /// your project to route requests through the PromptGuard proxy.
    #[command(after_help = "\
Examples:
  promptguard init                     Detect SDKs and set up routing
  promptguard init --provider openai   Only route OpenAI usage
  promptguard init --dry-run           Preview changes without applying")]
    Init {
        /// Target specific providers (e.g., openai, anthropic). Default: all detected
        #[arg(long)]
        provider: Vec<String>,

        /// PromptGuard API key (or set PROMPTGUARD_API_KEY env var).
        /// Use '-' to read the key from stdin — passing it as an argument
        /// exposes it in shell history and process listings.
        #[arg(long)]
        api_key: Option<String>,

        /// Proxy URL to route LLM requests through
        #[arg(long, default_value = "https://api.promptguard.co/api/v1")]
        base_url: String,

        /// Environment file to store API key
        #[arg(long, default_value = ".env")]
        env_file: String,

        /// Skip confirmation prompts (for CI/CD)
        #[arg(short = 'y', long = "yes", alias = "auto")]
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
    /// Without --text or --file: Detects OpenAI, Anthropic, Cohere, HuggingFace, Gemini, Groq, and AWS Bedrock SDK usage
    /// in your Python and TypeScript/JavaScript files.
    ///
    /// With --text or --file: Scans content for security threats (prompt injection, jailbreaks, etc.)
    /// via the PromptGuard API.
    ///
    /// Exit codes: 0 = content allowed / no SDK usage found,
    /// 2 = content blocked / SDK usage found, 1 = error.
    #[command(after_help = "\
Examples:
  promptguard scan                             Detect LLM SDK usage in this repo
  promptguard scan --text \"ignore the rules\"   Scan a string for threats
  promptguard scan --file prompt.txt --json    Scan a file, machine-readable")]
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

    /// Show current PromptGuard status and configuration
    ///
    /// Displays whether PromptGuard is active, which providers are configured,
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

    /// Re-apply PromptGuard transformations to source files
    ///
    /// Use this after modifying files manually or adding new SDK usage.
    Apply {
        /// Skip confirmation prompt
        #[arg(short = 'y', long)]
        yes: bool,
    },

    /// Temporarily disable PromptGuard (keeps configuration)
    ///
    /// LLM requests will go directly to providers until re-enabled.
    Disable {
        /// Skip confirmation prompt (for CI/CD and non-interactive callers)
        #[arg(short = 'y', long)]
        yes: bool,
    },

    /// Re-enable PromptGuard after disabling
    ///
    /// Restores proxy routing for LLM requests.
    Enable {
        /// Use runtime shims that intercept the supported SDK client
        /// classes, sync and async (recommended)
        #[arg(long)]
        runtime: bool,

        /// Skip confirmation prompt (for CI/CD and non-interactive callers)
        #[arg(short = 'y', long)]
        yes: bool,
    },

    /// Completely remove PromptGuard from this project
    ///
    /// Reverts all file changes and removes configuration.
    /// Use git to review changes before confirming.
    Revert {
        /// Skip confirmation prompt
        #[arg(short = 'y', long)]
        yes: bool,
    },

    /// View PromptGuard configuration
    ///
    /// Shows current settings including providers, proxy URL,
    /// exclude patterns, and metadata. Read-only: change settings by
    /// re-running 'promptguard init', or edit .promptguard.json directly.
    Config {
        /// Output as JSON (for scripting)
        #[arg(long)]
        json: bool,
    },

    /// Manage API keys
    ///
    /// View, update, or rotate your PromptGuard API key.
    /// Keys use the pg_live_* prefix.
    ///
    /// Run without a subcommand for an interactive menu.
    #[command(after_help = "\
Examples:
  promptguard key show               Show the current key (masked)
  promptguard key show --full        Reveal the full key
  promptguard key show --json        Machine-readable key info
  promptguard key update             Replace the stored key
  promptguard key rotate             How to rotate your key")]
    Key {
        #[command(subcommand)]
        action: Option<KeySubcommand>,
    },

    /// View activity logs from the PromptGuard API
    ///
    /// Fetches recent LLM requests, security events, and usage metrics
    /// directly from the PromptGuard backend.
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

    /// Test PromptGuard configuration
    ///
    /// Validates API key, tests proxy connectivity, and verifies
    /// that your setup is working correctly.
    Test,

    /// Verify end-to-end PromptGuard integration
    ///
    /// Runs connectivity, authentication, threat detection, and PII
    /// redaction checks against the live API. Use after setup to
    /// confirm everything works, or in CI to validate the integration.
    ///
    /// Exit codes: 0 = all checks passed, 2 = one or more checks failed
    /// (including API connectivity failures), 1 = error (e.g. missing
    /// credentials).
    Verify {
        /// Output results as JSON (for CI/scripting)
        #[arg(long)]
        json: bool,
    },

    /// Check for CLI updates
    ///
    /// Checks GitHub releases for a newer version and provides
    /// instructions for updating.
    Update,

    /// Redact PII and sensitive data from text
    ///
    /// Calls the PromptGuard API to identify and redact sensitive information
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
    /// Uses PromptGuard's Red Team API to evaluate security posture
    /// by testing with known attack patterns and jailbreak attempts.
    #[command(after_help = "\
Examples:
  promptguard redteam                             Run the default attack suite
  promptguard redteam --preset strict             Use the strict preset
")]
    Redteam {
        /// PromptGuard API base URL to run the tests against. Must be
        /// HTTPS and point at the PromptGuard API host (or localhost for
        /// development) — the API key is sent with every request and is
        /// never sent to other hosts.
        #[arg(long)]
        target_url: Option<String>,

        /// PromptGuard API key (or uses configured key).
        /// Use '-' to read the key from stdin — passing it as an argument
        /// exposes it in shell history and process listings.
        #[arg(long)]
        api_key: Option<String>,

        /// Output format: human or json
        #[arg(long, default_value = "human")]
        format: String,

        // NOTE: detailed per-test output comes from the GLOBAL -v/--verbose
        // flag. A subcommand-local `verbose: bool` clashed with the global
        // count-typed `verbose` arg and made clap panic on every `redteam`
        // invocation ("Mismatch between definition and access of `verbose`").
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

    /// Manage guardrail policies as YAML files (policy-as-code)
    ///
    /// Define guardrails in YAML, version in git, and apply via CLI.
    /// Supports apply, diff, and export operations.
    #[command(after_help = "\
Examples:
  promptguard policy export --project-id proj_123 > policy.yaml
  promptguard policy diff policy.yaml --project-id proj_123
  promptguard policy apply policy.yaml --project-id proj_123 --dry-run")]
    Policy {
        /// Action to perform: apply, diff, or export
        #[command(subcommand)]
        action: PolicySubcommand,

        /// Project ID to manage policies for. Required.
        // Optional in the parser because clap forbids required global
        // arguments (a required+global arg panics clap's debug assertions
        // on every 'policy' invocation); enforced before execution instead.
        #[arg(long, global = true)]
        project_id: Option<String>,

        /// PromptGuard API key (or uses configured key).
        /// Use '-' to read the key from stdin — passing it as an argument
        /// exposes it in shell history and process listings.
        #[arg(long, global = true)]
        api_key: Option<String>,

        /// API base URL
        #[arg(long, global = true)]
        base_url: Option<String>,
    },

    /// Start MCP (Model Context Protocol) server for IDE integration
    ///
    /// Exposes PromptGuard tools over the MCP protocol so AI-powered
    /// editors (Cursor, Claude Code, Windsurf, etc.) can call them.
    Mcp {
        /// Transport type (currently only 'stdio' is supported)
        #[arg(short, long, default_value = "stdio")]
        transport: String,
    },

    /// Authenticate with PromptGuard and store credentials globally
    ///
    /// Saves your API key to ~/.promptguard/credentials.json so all
    /// commands and projects can use it without per-project setup.
    Login {
        /// API key to authenticate with (or enter interactively).
        /// Use '-' to read the key from stdin — passing it as an argument
        /// exposes it in shell history and process listings.
        #[arg(long)]
        api_key: Option<String>,

        /// Custom API base URL
        #[arg(long)]
        base_url: Option<String>,

        /// Output results as JSON
        #[arg(long)]
        json: bool,
    },

    /// Remove stored PromptGuard credentials
    ///
    /// Deletes ~/.promptguard/credentials.json.
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

    /// Manage PromptGuard projects
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
    /// PromptGuard API. Useful for monitoring and auditing.
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

    /// Open the PromptGuard dashboard in your browser
    Dashboard {
        /// Output the URL as JSON instead of opening browser
        #[arg(long)]
        json: bool,
    },
}

#[allow(clippy::doc_markdown)]
#[derive(Subcommand)]
enum KeySubcommand {
    /// Show the current API key
    Show {
        /// Reveal the full key instead of a masked version
        #[arg(long)]
        full: bool,

        /// Output results as JSON (for scripting)
        #[arg(long)]
        json: bool,
    },

    /// Update the stored API key (prompts for a new one)
    Update,

    /// Show how to rotate your API key
    Rotate,
}

#[allow(clippy::doc_markdown)]
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

#[allow(clippy::doc_markdown)]
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

    auth::set_allow_custom_proxy(cli.allow_custom_proxy);

    let Some(command) = cli.command else {
        print_welcome();
        return;
    };

    // Whether the invoked command requested machine-readable output. On error
    // we emit JSON instead of a human line so `--json` consumers always get
    // parseable output on stdout (see the error handler below).
    let json_mode = command_requested_json(&command);

    let result = match command {
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
        .execute()
        .map(|exit_code| {
            // Differentiated exit codes for scripting/CI:
            // 0 = allow/clean, 2 = block/findings (errors exit 1 below).
            if exit_code != 0 {
                std::process::exit(exit_code);
            }
        }),

        Commands::Status { json } => StatusCommand { json }.execute(),

        Commands::Doctor { json } => DoctorCommand { json }.execute(),

        Commands::Apply { yes } => ApplyCommand { yes }.execute(),

        Commands::Revert { yes } => RevertCommand { yes }.execute(),

        Commands::Disable { yes } => DisableCommand { yes }.execute(),
        Commands::Enable { runtime, yes } => EnableCommand { runtime, yes }.execute(),
        Commands::Config { json } => ConfigCommand { json }.execute(),
        Commands::Key { action } => {
            let key_action = action.map(|a| match a {
                KeySubcommand::Show { full, json } => KeyAction::Show { full, json },
                KeySubcommand::Update => KeyAction::Update,
                KeySubcommand::Rotate => KeyAction::Rotate,
            });
            KeyCommand::run(key_action)
        },
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
        Commands::Verify { json } => VerifyCommand { json }.execute().map(|exit_code| {
            // Same differentiated exit codes as `scan`, so `verify` can gate
            // CI: 0 = all checks passed, 2 = checks failed (errors exit 1
            // below).
            if exit_code != 0 {
                std::process::exit(exit_code);
            }
        }),
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
            test,
            prompt,
            preset,
        } => RedTeamCommand {
            target_url,
            api_key,
            output_format: format,
            // Detailed per-test output rides on the global -v/--verbose.
            verbose: cli.verbose > 0,
            test_name: test,
            custom_prompt: prompt,
            preset,
        }
        .execute(),

        Commands::Policy {
            action,
            project_id,
            api_key,
            base_url,
        } => match project_id {
            Some(project_id) => {
                let policy_action = match action {
                    PolicySubcommand::Apply { file, dry_run } => {
                        PolicyAction::Apply { file, dry_run }
                    },
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
            None => Err(error::PromptGuardError::Config(
                "--project-id is required for policy commands".to_string(),
            )),
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
        // The command already rendered a fully-formatted, actionable error
        // (e.g. init's "✗ API key rejected …" block). Exit non-zero WITHOUT
        // printing again, so the message is not shown twice.
        if matches!(e, error::PromptGuardError::AlreadyReported) {
            std::process::exit(1);
        }
        if json_mode {
            // --json modes emit a structured error on stdout so a piped
            // consumer (e.g. `... --json | jq`) always parses valid JSON and
            // can branch on `.code`. Still exits non-zero.
            let obj = serde_json::json!({ "error": e.to_string(), "code": e.code() });
            println!("{}", serde_json::to_string(&obj).unwrap_or_default());
        } else {
            // No "Error:" prefix: each categorized variant already carries its
            // own descriptive prefix (e.g. "Configuration error: …"), so the
            // old prefix produced doubled "Error: Configuration error: …".
            eprintln!("{e}");
        }
        std::process::exit(1);
    }
}

/// Whether the parsed command was invoked with a machine-readable output
/// flag (`--json`, or `--format json` for `redteam`). Drives structured
/// error output in `main`.
fn command_requested_json(command: &Commands) -> bool {
    match command {
        Commands::Scan { json, .. }
        | Commands::Status { json, .. }
        | Commands::Doctor { json, .. }
        | Commands::Config { json, .. }
        | Commands::Logs { json, .. }
        | Commands::Verify { json, .. }
        | Commands::Redact { json, .. }
        | Commands::Login { json, .. }
        | Commands::Logout { json, .. }
        | Commands::Whoami { json, .. }
        | Commands::Projects { json, .. }
        | Commands::Events { json, .. }
        | Commands::Dashboard { json, .. }
        | Commands::Key {
            action: Some(KeySubcommand::Show { json, .. }),
        } => *json,
        Commands::Redteam { format, .. } => format.eq_ignore_ascii_case("json"),
        _ => false,
    }
}

/// Friendly status + next-steps banner shown when invoked with no subcommand.
fn print_welcome() {
    use output::Output;

    Output::header("🛡️ PromptGuard CLI");
    println!("Drop-in LLM security for your applications");
    println!();

    // Mirror resolve_api_key precedence to decide what to suggest next.
    if auth::resolve_api_key().is_ok() {
        let base_url = auth::resolve_base_url();
        Output::success("You're logged in");
        Output::step(&format!("API: {base_url}"));
        println!();
        println!("Next steps:");
        println!("  • Scan text:        promptguard scan --text \"ignore previous instructions\"");
        println!("  • Integrate a repo: promptguard init");
        println!("  • Verify setup:     promptguard verify");
    } else {
        Output::info("You're not logged in yet");
        println!();
        println!("Run 'promptguard login' to get started.");
        println!("  Get your API key at https://app.promptguard.co/settings/api-keys");
    }

    println!();
    println!("See all commands: promptguard --help");
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    /// `disable` must accept `--yes`/`-y` so non-interactive callers (the
    /// VS Code extension spawns the CLI with piped stdin) can skip the
    /// confirmation prompt instead of hanging on a stdin read.
    #[test]
    fn disable_accepts_yes_flag() {
        for argv in [
            &["promptguard", "disable", "--yes"][..],
            &["promptguard", "disable", "-y"][..],
        ] {
            let cli = Cli::try_parse_from(argv).expect("disable must accept --yes/-y");
            assert!(
                matches!(cli.command, Some(Commands::Disable { yes: true })),
                "expected Disable {{ yes: true }} for {argv:?}"
            );
        }
    }

    #[test]
    fn disable_defaults_to_confirming() {
        let cli = Cli::try_parse_from(["promptguard", "disable"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Commands::Disable { yes: false })
        ));
    }

    /// `enable` must accept `--yes`/`-y`, with and without `--runtime`.
    #[test]
    fn enable_accepts_yes_flag() {
        let cli = Cli::try_parse_from(["promptguard", "enable", "--runtime", "--yes"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Commands::Enable {
                runtime: true,
                yes: true
            })
        ));

        let cli = Cli::try_parse_from(["promptguard", "enable", "-y"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Commands::Enable {
                runtime: false,
                yes: true
            })
        ));
    }

    #[test]
    fn enable_defaults_to_confirming() {
        let cli = Cli::try_parse_from(["promptguard", "enable"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Commands::Enable {
                runtime: false,
                yes: false
            })
        ));
    }

    /// `redteam --api-key -` must parse ('-' is resolved to a stdin read by
    /// `resolve_api_key_flag` at execution time, matching init/login).
    ///
    /// Also a regression test for the clap id clash: a redteam-local
    /// `verbose: bool` conflicted with the global count-typed `-v/--verbose`
    /// and made EVERY `redteam` invocation panic at parse time.
    #[test]
    fn redteam_api_key_accepts_stdin_sentinel() {
        let cli = Cli::try_parse_from(["promptguard", "redteam", "--api-key", "-"]).unwrap();
        match cli.command {
            Some(Commands::Redteam { api_key, .. }) => {
                assert_eq!(api_key.as_deref(), Some("-"));
            },
            _ => panic!("expected Redteam command"),
        }

        // Global -v still parses alongside redteam (drives per-test detail).
        let cli = Cli::try_parse_from(["promptguard", "redteam", "-v"]).unwrap();
        assert_eq!(cli.verbose, 1);
        assert!(matches!(cli.command, Some(Commands::Redteam { .. })));
    }

    /// `key` gains `show`/`update`/`rotate` subcommands (mirroring
    /// projects/policy) so the actions are discoverable via `--help` and
    /// scriptable, while a bare `key` still drops to the interactive menu.
    #[test]
    fn key_parses_subcommands() {
        // Bare `key` → no subcommand (interactive menu fallback).
        let cli = Cli::try_parse_from(["promptguard", "key"]).unwrap();
        assert!(matches!(cli.command, Some(Commands::Key { action: None })));

        // `key show` defaults to masked, non-JSON.
        let cli = Cli::try_parse_from(["promptguard", "key", "show"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Commands::Key {
                action: Some(KeySubcommand::Show {
                    full: false,
                    json: false
                })
            })
        ));

        // `key show --full --json` toggles both flags.
        let cli = Cli::try_parse_from(["promptguard", "key", "show", "--full", "--json"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Commands::Key {
                action: Some(KeySubcommand::Show {
                    full: true,
                    json: true
                })
            })
        ));

        // `key update` and `key rotate` parse to their variants.
        let cli = Cli::try_parse_from(["promptguard", "key", "update"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Commands::Key {
                action: Some(KeySubcommand::Update)
            })
        ));

        let cli = Cli::try_parse_from(["promptguard", "key", "rotate"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Commands::Key {
                action: Some(KeySubcommand::Rotate)
            })
        ));

        // An unknown `key` subcommand is rejected (not silently swallowed).
        assert!(Cli::try_parse_from(["promptguard", "key", "bogus"]).is_err());
    }

    /// `policy --api-key -` must parse the same way.
    ///
    /// Also a regression test for a clap debug-assert panic: `project_id`
    /// was marked required AND global, which clap forbids — every `policy`
    /// invocation panicked in debug builds.
    #[test]
    fn policy_api_key_accepts_stdin_sentinel() {
        let cli = Cli::try_parse_from([
            "promptguard",
            "policy",
            "--project-id",
            "proj_123",
            "--api-key",
            "-",
            "export",
        ])
        .unwrap();
        match cli.command {
            Some(Commands::Policy {
                api_key,
                project_id,
                ..
            }) => {
                assert_eq!(api_key.as_deref(), Some("-"));
                assert_eq!(project_id.as_deref(), Some("proj_123"));
            },
            _ => panic!("expected Policy command"),
        }

        // Global placement after the subcommand still works.
        let cli = Cli::try_parse_from([
            "promptguard",
            "policy",
            "export",
            "--project-id",
            "proj_123",
            "--api-key",
            "-",
        ])
        .unwrap();
        match cli.command {
            Some(Commands::Policy {
                api_key,
                project_id,
                ..
            }) => {
                assert_eq!(api_key.as_deref(), Some("-"));
                assert_eq!(project_id.as_deref(), Some("proj_123"));
            },
            _ => panic!("expected Policy command"),
        }
    }
}
