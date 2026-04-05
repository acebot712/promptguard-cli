use crate::auth::delete_credentials;
use crate::error::Result;
use crate::output::Output;

pub struct LogoutCommand {
    pub json: bool,
}

impl LogoutCommand {
    pub fn execute(&self) -> Result<()> {
        delete_credentials()?;

        if self.json {
            let result = serde_json::json!({ "status": "logged_out" });
            println!(
                "{}",
                serde_json::to_string_pretty(&result).unwrap_or_default()
            );
        } else {
            Output::success("Logged out. Credentials removed from ~/.promptguard/credentials.json");
        }

        Ok(())
    }
}
