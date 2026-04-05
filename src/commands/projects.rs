use crate::api::PromptGuardClient;
use crate::auth::{load_credentials, resolve_api_key, resolve_base_url, save_credentials};
use crate::error::Result;
use crate::output::Output;

pub enum ProjectsAction {
    List,
    Select { project_id: String },
}

pub struct ProjectsCommand {
    pub action: ProjectsAction,
    pub json: bool,
}

impl ProjectsCommand {
    pub fn execute(&self) -> Result<()> {
        match &self.action {
            ProjectsAction::List => self.list(),
            ProjectsAction::Select { project_id } => self.select(project_id),
        }
    }

    fn list(&self) -> Result<()> {
        let api_key = resolve_api_key()?;
        let base_url = resolve_base_url();
        let client = PromptGuardClient::new(api_key, Some(base_url))?;

        let active_project = load_credentials()
            .ok()
            .flatten()
            .and_then(|c| c.active_project);

        let projects: serde_json::Value = client.get("/projects")?;

        if self.json {
            let result = serde_json::json!({
                "projects": projects,
                "active_project": active_project,
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&result).unwrap_or_default()
            );
            return Ok(());
        }

        Output::header("Projects");

        if let Some(arr) = projects.as_array() {
            if arr.is_empty() {
                Output::info("No projects found. Create one at https://app.promptguard.co");
                return Ok(());
            }

            for project in arr {
                let id = project.get("id").and_then(|v| v.as_str()).unwrap_or("?");
                let name = project
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Unnamed");
                let marker = if active_project.as_deref() == Some(id) {
                    " (active)"
                } else {
                    ""
                };
                Output::step(&format!("{name} [{id}]{marker}"));
            }
        } else {
            Output::info("No projects found");
        }

        Ok(())
    }

    fn select(&self, project_id: &str) -> Result<()> {
        let mut creds = load_credentials()?.unwrap_or_else(|| crate::auth::GlobalCredentials {
            api_key: String::new(),
            base_url: None,
            active_project: None,
        });

        // If no API key in global creds, try resolving
        if creds.api_key.is_empty() {
            creds.api_key = resolve_api_key()?;
        }

        creds.active_project = Some(project_id.to_string());
        save_credentials(&creds)?;

        if self.json {
            let result = serde_json::json!({
                "active_project": project_id,
                "status": "selected",
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&result).unwrap_or_default()
            );
        } else {
            Output::success(&format!("Active project set to: {project_id}"));
        }

        Ok(())
    }
}
