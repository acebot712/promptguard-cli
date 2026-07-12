use crate::config::{ConfigManager, PromptGuardConfig};
use crate::env::EnvManager;
use crate::error::{PromptGuardError, Result};
use crate::output::Output;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// A `key` subcommand action. A bare `key` (no subcommand, i.e. `None`) falls
/// back to the interactive menu in [`KeyCommand::run`], mirroring the
/// projects/policy subcommand pattern while staying scriptable.
#[derive(Clone, Copy)]
pub enum KeyAction {
    /// Show the current key, masked unless `full`. `json` emits a
    /// machine-readable object.
    Show { full: bool, json: bool },
    /// Update the stored key (prompts for a new one).
    Update,
    /// Print rotation instructions (rotation is performed in the dashboard).
    Rotate,
}

pub struct KeyCommand;

impl KeyCommand {
    /// Dispatch a `key` invocation: an explicit subcommand runs directly, a
    /// bare `key` falls back to the interactive numbered menu.
    pub fn run(action: Option<KeyAction>) -> Result<()> {
        match action {
            Some(KeyAction::Show { full, json }) => Self::show(full, json),
            Some(KeyAction::Update) => Self::update(),
            Some(KeyAction::Rotate) => Self::rotate(),
            None => Self::interactive(),
        }
    }

    /// Load the project config, erroring if not initialized in this directory.
    /// Returns the manager (for saving), the config, and the resolved `.env`
    /// path.
    fn load() -> Result<(ConfigManager, PromptGuardConfig, PathBuf)> {
        let config_manager = ConfigManager::new(None)?;
        if !config_manager.exists() {
            return Err(PromptGuardError::NotInitialized);
        }
        let config = config_manager.load()?;
        let root_path = std::env::current_dir()?;
        let env_path = root_path.join(&config.env_file);
        Ok((config_manager, config, env_path))
    }

    fn show(full: bool, json: bool) -> Result<()> {
        let (_config_manager, config, _env_path) = Self::load()?;

        if json {
            let key_display = if full {
                config.api_key.clone()
            } else {
                Output::mask_api_key(&config.api_key)
            };
            let obj = serde_json::json!({
                "env_var": config.env_var_name,
                "env_file": config.env_file,
                "api_key": key_display,
                "masked": !full,
            });
            println!("{}", serde_json::to_string_pretty(&obj).unwrap_or_default());
            return Ok(());
        }

        Output::header("API Key Management");
        println!("\nCurrent API key:");
        if full {
            println!("  {} = {}", config.env_var_name, config.api_key);
            println!("\n⚠️  Keep this key secure. Don't share it publicly.");
        } else {
            println!(
                "  {} = {}",
                config.env_var_name,
                Output::mask_api_key(&config.api_key)
            );
            println!("\nRun 'promptguard key show --full' to reveal the full key.");
        }

        Ok(())
    }

    fn update() -> Result<()> {
        let (config_manager, mut config, env_path) = Self::load()?;

        Output::header("API Key Management");
        println!("\nCurrent API key:");
        println!(
            "  {} = {}",
            config.env_var_name,
            Output::mask_api_key(&config.api_key)
        );

        Self::prompt_and_store_new_key(&config_manager, &mut config, &env_path)
    }

    fn rotate() -> Result<()> {
        // Rotation issues a fresh key server-side; there is no key-issuing API
        // from the CLI, so print instructions (mirrors the old menu option 3).
        let (_config_manager, _config, _env_path) = Self::load()?;

        Output::header("API Key Management");
        Output::info("Key rotation requires API access.");
        println!("\nTo rotate your API key:");
        println!("  1. Visit: https://app.promptguard.co/settings/api-keys");
        println!("  2. Generate a new key");
        println!("  3. Run: promptguard key update");

        Ok(())
    }

    /// Interactive numbered menu, kept ONLY as the bare-`key` fallback for
    /// backwards compatibility. Scriptable callers should use the
    /// `show`/`update`/`rotate` subcommands instead.
    fn interactive() -> Result<()> {
        let (config_manager, mut config, env_path) = Self::load()?;

        Output::header("API Key Management");

        // Show current key (masked)
        println!("\nCurrent API key:");
        println!(
            "  {} = {}",
            config.env_var_name,
            Output::mask_api_key(&config.api_key)
        );

        println!("\nOptions:");
        println!("  1. Update API key");
        println!("  2. Show full key");
        println!("  3. Rotate key");
        println!("  4. Cancel");

        print!("\nSelect option (1-4): ");
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;

        match input.trim() {
            "1" => Self::prompt_and_store_new_key(&config_manager, &mut config, &env_path),
            "2" => {
                // Show full key
                println!("\nFull API key:");
                println!("  {}", config.api_key);
                println!("\n⚠️  Keep this key secure. Don't share it publicly.");
                Ok(())
            },
            "3" => {
                // Rotate key - requires dashboard access
                Output::info("Key rotation requires API access.");
                println!("\nTo rotate your API key:");
                println!("  1. Visit: https://app.promptguard.co/settings/api-keys");
                println!("  2. Generate a new key");
                println!("  3. Run: promptguard key update");
                Ok(())
            },
            _ => {
                Output::info("Cancelled");
                Ok(())
            },
        }
    }

    /// Prompt for a new key on stdin, validate it, and persist it to both
    /// `.promptguard.json` and the `.env` file. Shared by the `update`
    /// subcommand and the interactive menu.
    fn prompt_and_store_new_key(
        config_manager: &ConfigManager,
        config: &mut PromptGuardConfig,
        env_path: &Path,
    ) -> Result<()> {
        // Terminal echo is not disabled (no raw-mode dependency); warn so the
        // user can clear their screen or use a non-observed terminal.
        Output::warning("Input is not hidden — the key will be visible on screen.");
        print!("Enter new API key: ");
        io::stdout().flush()?;

        let mut new_key = String::new();
        io::stdin().read_line(&mut new_key)?;
        let new_key = new_key.trim().to_string();

        // Validate key format
        if !crate::config::is_valid_api_key(&new_key) {
            return Err(PromptGuardError::InvalidApiKey);
        }

        // Update config
        config.api_key = new_key.clone();
        config_manager.save(config)?;

        // Update .env
        EnvManager::add_or_update_key(env_path, &config.env_var_name, &new_key)?;

        Output::success("API key updated successfully!");
        println!("\nThe new key has been saved to:");
        println!("  • .promptguard.json");
        println!("  • {}", config.env_file);

        Ok(())
    }
}
