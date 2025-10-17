use crate::error::Result;
use crate::output::Output;

pub struct UpdateCommand;

impl UpdateCommand {
    pub fn execute() -> Result<()> {
        Output::header("Update PromptGuard CLI");

        let current_version = env!("CARGO_PKG_VERSION");
        println!("\nCurrent version: v{}", current_version);

        Output::info("Checking for updates...");

        println!("\nTo update PromptGuard CLI:");
        println!("  • Using Homebrew:");
        println!("      brew upgrade promptguard");
        println!();
        println!("  • Using npm:");
        println!("      npm update -g promptguard");
        println!();
        println!("  • Using curl:");
        println!("      curl -fsSL https://get.promptguard.co | sh");
        println!();
        println!("  • Using cargo:");
        println!("      cargo install --force promptguard-cli");

        println!("\nRelease notes:");
        println!("  https://github.com/promptguard/promptguard-cli/releases");

        println!("\nDocumentation:");
        println!("  https://docs.promptguard.co/cli");

        Ok(())
    }
}
