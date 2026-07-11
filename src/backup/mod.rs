use crate::error::Result;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub struct BackupManager {
    backup_extension: String,
}

impl BackupManager {
    pub fn new(backup_extension: Option<String>) -> Self {
        Self {
            backup_extension: backup_extension.unwrap_or_else(|| ".bak".to_string()),
        }
    }

    pub fn backup_path(&self, file_path: &Path) -> PathBuf {
        let mut backup = file_path.to_path_buf();
        let current_extension = backup
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_string();

        // strip_prefix instead of [1..]: an extension without a leading dot
        // (or an empty one, which used to panic here) is handled gracefully.
        // Config load validates the extension, but BackupManager can be
        // constructed with arbitrary strings.
        let bare_extension = self
            .backup_extension
            .strip_prefix('.')
            .unwrap_or(&self.backup_extension);

        let new_extension = if current_extension.is_empty() {
            bare_extension.to_string()
        } else {
            format!("{current_extension}.{bare_extension}")
        };

        backup.set_extension(&new_extension);
        backup
    }

    pub fn create_backup(&self, file_path: &Path) -> Result<PathBuf> {
        let backup_path = self.backup_path(file_path);
        // CRITICAL: Never overwrite existing backups - they contain the original state
        if !backup_path.exists() {
            fs::copy(file_path, &backup_path)?;
        }
        Ok(backup_path)
    }

    pub fn restore_backup(&self, file_path: &Path) -> Result<()> {
        let backup_path = self.backup_path(file_path);
        if backup_path.exists() {
            fs::copy(&backup_path, file_path)?;
        }
        Ok(())
    }

    pub fn list_backups(&self, root_path: &Path) -> Vec<PathBuf> {
        let mut backups = Vec::new();
        let pattern = format!("*{}", self.backup_extension);

        for entry in WalkDir::new(root_path)
            .follow_links(false)
            .into_iter()
            .filter_map(std::result::Result::ok)
        {
            let path = entry.path();
            if path.is_file() {
                if let Some(filename) = path.file_name() {
                    if let Ok(glob_pattern) = glob::Pattern::new(&pattern) {
                        if glob_pattern.matches(&filename.to_string_lossy()) {
                            backups.push(path.to_path_buf());
                        }
                    }
                }
            }
        }

        backups
    }
}
