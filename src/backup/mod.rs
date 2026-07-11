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

    /// Restore `file_path` from its backup.
    ///
    /// Returns `Ok(true)` when the backup existed and was copied back, and
    /// `Ok(false)` when no backup file exists — nothing was restored, and
    /// callers must report that honestly instead of counting it as a
    /// successful restore.
    pub fn restore_backup(&self, file_path: &Path) -> Result<bool> {
        let backup_path = self.backup_path(file_path);
        if !backup_path.exists() {
            return Ok(false);
        }
        fs::copy(&backup_path, file_path)?;
        Ok(true)
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

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    /// Regression: `restore_backup` used to return `Ok(())` even when the
    /// backup file was missing, so `disable` printed "Restored N files"
    /// while restoring nothing.
    #[test]
    fn restore_backup_returns_false_when_backup_missing() {
        let temp_dir = TempDir::new().unwrap();
        let file = temp_dir.path().join("client.py");
        fs::write(&file, "transformed").unwrap();

        let manager = BackupManager::new(Some(".pgbak".to_string()));
        let restored = manager.restore_backup(&file).unwrap();

        assert!(!restored, "no backup exists, so nothing was restored");
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            "transformed",
            "file must be untouched when no backup exists"
        );
    }

    #[test]
    fn restore_backup_returns_true_and_restores_content() {
        let temp_dir = TempDir::new().unwrap();
        let file = temp_dir.path().join("client.py");
        fs::write(&file, "original").unwrap();

        let manager = BackupManager::new(Some(".pgbak".to_string()));
        manager.create_backup(&file).unwrap();
        fs::write(&file, "transformed").unwrap();

        let restored = manager.restore_backup(&file).unwrap();

        assert!(restored, "backup exists, restore must report true");
        assert_eq!(fs::read_to_string(&file).unwrap(), "original");
    }
}
