use crate::error::Result;
use crate::output::Output;

pub struct DashboardCommand {
    pub json: bool,
}

impl DashboardCommand {
    pub fn execute(&self) -> Result<()> {
        let url = "https://app.promptguard.co";

        if self.json {
            let result = serde_json::json!({
                "url": url,
                "action": "open_browser",
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&result).unwrap_or_default()
            );
        } else {
            Output::info(&format!("Opening dashboard: {url}"));
            if let Err(e) = open::that(url) {
                Output::warning(&format!("Could not open browser: {e}"));
                Output::info(&format!("Open manually: {url}"));
            }
        }

        Ok(())
    }
}
