pub mod apply;
pub mod config;
pub mod dashboard;
pub mod disable;
pub mod doctor;
pub mod enable;
pub mod events;
pub mod init;
pub mod key;
pub mod login;
pub mod logout;
pub mod logs;
pub mod mcp;
pub mod policy;
pub mod projects;
pub mod redact;
pub mod redteam;
pub mod revert;
pub mod scan;
pub mod status;
pub mod test;
pub mod update;
pub mod verify;
pub mod whoami;

use crate::backup::BackupManager;
use crate::types::Provider;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Result of running the shared transform pipeline.
pub struct TransformOutcome {
    /// Files whose contents were actually modified.
    pub files_modified: Vec<PathBuf>,
    /// Backups created, as root-relative path strings for
    /// `metadata.backups` (consulted by `disable` to restore exactly what
    /// `PromptGuard` created).
    pub backups_created: Vec<String>,
}

/// Shared detection loop: map each provider to the files where its SDK
/// usage was detected. Used by `init`, `apply`, and `enable`.
pub fn detect_providers_in_files(
    files: &[PathBuf],
    providers_to_check: &[Provider],
) -> HashMap<Provider, Vec<PathBuf>> {
    let mut detection_results: HashMap<Provider, Vec<PathBuf>> = HashMap::new();

    for file_path in files {
        if let Ok(results) = crate::detector::detect_all_providers(file_path) {
            for (provider, result) in results {
                if providers_to_check.contains(&provider) && !result.instances.is_empty() {
                    detection_results
                        .entry(provider)
                        .or_default()
                        .push(file_path.clone());
                }
            }
        }
    }

    detection_results
}

/// Shared dedup → backup → transform → record pipeline used by `init`,
/// `apply`, and `enable`.
///
/// How [`run_transform_pipeline`] treats the filesystem.
///
/// Dry-run implying "no backups" is modeled in the type: there is no way to
/// combine `DryRun` with a backup manager.
#[derive(Clone, Copy)]
pub enum TransformMode<'a> {
    /// Write transformed files, backing each up first when a
    /// [`BackupManager`] is provided.
    Apply(Option<&'a BackupManager>),
    /// Compute and report every would-be modification but write nothing:
    /// no backups are created and no file is touched on disk.
    DryRun,
}

/// For each provider the detected files are deduped and sorted, backed up
/// BEFORE transformation (in [`TransformMode::Apply`] with a backup manager),
/// then transformed. `on_result` receives each successfully processed file so
/// callers can print their command-specific output; transform failures are
/// reported as warnings and skipped.
pub fn run_transform_pipeline(
    detection_results: &HashMap<Provider, Vec<PathBuf>>,
    root_path: &Path,
    mode: TransformMode<'_>,
    proxy_url: &str,
    env_var_name: &str,
    mut on_result: impl FnMut(Provider, &Path, &crate::types::TransformResult),
) -> TransformOutcome {
    let (backup_manager, dry_run) = match mode {
        TransformMode::Apply(bm) => (bm, false),
        TransformMode::DryRun => (None, true),
    };
    let mut outcome = TransformOutcome {
        files_modified: Vec::new(),
        backups_created: Vec::new(),
    };

    for (provider, files) in detection_results {
        let mut unique_files = files.clone();
        unique_files.sort();
        unique_files.dedup();

        for file_path in unique_files {
            if let Some(bm) = backup_manager {
                if let Ok(backup_path) = bm.create_backup(&file_path) {
                    outcome.backups_created.push(
                        backup_path
                            .strip_prefix(root_path)
                            .unwrap_or(&backup_path)
                            .to_string_lossy()
                            .to_string(),
                    );
                }
            }

            match crate::transformer::transform_file(
                &file_path,
                *provider,
                proxy_url,
                env_var_name,
                dry_run,
            ) {
                Ok(result) => {
                    if result.modified {
                        outcome.files_modified.push(file_path.clone());
                    }
                    on_result(*provider, &file_path, &result);
                },
                Err(e) => {
                    crate::output::Output::warning(&format!(
                        "Failed to transform {}: {}",
                        file_path.display(),
                        e
                    ));
                },
            }
        }
    }

    outcome
}

/// Read a file to a string with terminal-friendly error messages.
///
/// The default `std::io::Error` Display leaks the raw errno (e.g.
/// "No such file or directory (os error 2)"), which is noise for a CLI user.
/// This maps the common not-found case to "File not found: <path>" and keeps
/// other IO errors readable (permission denied, etc.) without the errno tail.
pub fn read_file_friendly(file_path: &str) -> crate::error::Result<String> {
    std::fs::read_to_string(file_path).map_err(|e| {
        let msg = match e.kind() {
            std::io::ErrorKind::NotFound => format!("File not found: {file_path}"),
            std::io::ErrorKind::PermissionDenied => {
                format!("Permission denied reading file: {file_path}")
            },
            _ => format!("Could not read file '{file_path}': {e}"),
        };
        crate::error::PromptGuardError::Io(std::io::Error::new(e.kind(), msg))
    })
}

/// Return `singular` when `count == 1`, otherwise `plural`.
///
/// Tiny grammar helper so summaries read "1 file" not "1 files".
#[must_use]
pub fn pluralize(count: usize, singular: &'static str, plural: &'static str) -> &'static str {
    if count == 1 {
        singular
    } else {
        plural
    }
}

/// Resolve an `--api-key` flag value, supporting `-` to read the key from
/// stdin (avoids exposing the key in shell history and process listings).
pub fn resolve_api_key_flag(value: &str) -> crate::error::Result<String> {
    if value != "-" {
        return Ok(value.to_string());
    }

    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    let key = line.trim().to_string();
    if key.is_empty() {
        return Err(crate::error::PromptGuardError::Custom(
            "No API key received on stdin (expected the key on the first line)".to_string(),
        ));
    }
    Ok(key)
}

pub use apply::ApplyCommand;
pub use config::ConfigCommand;
pub use dashboard::DashboardCommand;
pub use disable::DisableCommand;
pub use doctor::DoctorCommand;
pub use enable::EnableCommand;
pub use events::EventsCommand;
pub use init::InitCommand;
pub use key::{KeyAction, KeyCommand};
pub use login::LoginCommand;
pub use logout::LogoutCommand;
pub use logs::LogsCommand;
pub use mcp::McpCommand;
pub use policy::{PolicyAction, PolicyCommand};
pub use projects::{ProjectsAction, ProjectsCommand};
pub use redact::RedactCommand;
pub use redteam::RedTeamCommand;
pub use revert::RevertCommand;
pub use scan::ScanCommand;
pub use status::StatusCommand;
pub use test::TestCommand;
pub use update::UpdateCommand;
pub use verify::VerifyCommand;
pub use whoami::WhoamiCommand;
