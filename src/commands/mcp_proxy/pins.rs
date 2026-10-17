//! Tool-list pins: what each wrapped server's `tools/list` was the first time
//! the proxy scanned it, kept in `~/.promptguard/mcp-pins.json`.
//!
//! The manifest-scan endpoint is stateless (acebot712/promptguard#942): it
//! returns a pin for the list it scanned and compares the list against the
//! pin it is sent. Keeping the pin is the caller's job, so it lives here, on
//! the developer's machine, one per server.
//!
//! A server is identified by the SHA-256 of its command line and the
//! canonical directory it runs in (`npx server .` in two projects is two
//! servers), so the file never holds the arguments or the path themselves,
//! which can carry tokens and names. Nothing from the environment is part of
//! the key or stored. Each entry keeps a readable label (`mcp-proxy:<program>`
//! and, for a package runner, the package, with any credentials in a URL
//! removed), and no other argument.
//!
//! The file is a cache of trust decisions, never a reason to fail. Missing,
//! it is created. Corrupt, it is moved aside as
//! `mcp-pins.json.corrupt-<timestamp>`, so what was lost can be seen, and
//! every server is pinned again. Written by a newer promptguard (a `version`
//! above this one's), or unreadable, it is left untouched and the server runs
//! unpinned. Several proxies may run at once (one per server an agent uses),
//! so a write takes a lock, re-reads the file under it, and replaces it with
//! a rename, so a reader never sees a half-written file and no proxy's pin is
//! lost to another's write.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const FILE_NAME: &str = "mcp-pins.json";
const LOCK_NAME: &str = "mcp-pins.json.lock";
const FORMAT_VERSION: u32 = 1;

/// One server's pin, as the manifest-scan endpoint returned it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pin {
    /// A readable name for the server (`mcp-proxy:<program>`); not the key.
    pub server: String,
    /// The list's pin: opaque, sent back as `pin`.
    pub sha256: String,
    /// Each tool's own hash, to name the tools a rug-pull changed.
    #[serde(default)]
    pub tools: Vec<ToolHash>,
    pub pinned_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolHash {
    pub name: String,
    pub sha256: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct PinFile {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    servers: BTreeMap<String, Pin>,
}

/// The pins file, seen from one server.
pub struct PinStore {
    dir: PathBuf,
    key: String,
}

/// What [`PinStore::load`] found.
#[derive(Debug, PartialEq, Eq)]
pub struct Loaded {
    /// This server's pin, if one is stored and well formed.
    pub pin: Option<Pin>,
    /// What went wrong with the file, for the log; `None` when nothing did
    /// (a missing file is not a problem).
    pub problem: Option<String>,
    /// Whether a pin may be saved: false for a file this version must leave
    /// untouched.
    pub writable: bool,
}

/// The file's state on disk.
enum FileState {
    Missing,
    Read(PinFile),
    Corrupt(String),
    Newer(u64),
    Unreadable(String),
}

impl PinStore {
    pub fn new(dir: PathBuf, command: &[String], cwd: Option<&Path>) -> Self {
        Self {
            dir,
            key: server_key(command, cwd),
        }
    }

    pub fn path(&self) -> PathBuf {
        self.dir.join(FILE_NAME)
    }

    /// This server's pin, and what went wrong with the file. A corrupt file
    /// is moved aside here, under the lock, so every server re-pins.
    pub fn load(&self) -> Loaded {
        let unpinned = |problem: String, writable| Loaded {
            pin: None,
            problem: Some(problem),
            writable,
        };
        match read(&self.path()) {
            FileState::Missing => Loaded {
                pin: None,
                problem: None,
                writable: true,
            },
            FileState::Read(file) => Loaded {
                pin: file
                    .servers
                    .get(&self.key)
                    .filter(|p| is_pin(&p.sha256))
                    .cloned(),
                problem: None,
                writable: true,
            },
            FileState::Newer(version) => unpinned(
                format!(
                    "its version {version} is newer than this promptguard understands, so it \
                     was left untouched and this server runs unpinned; upgrade promptguard to \
                     use it"
                ),
                false,
            ),
            FileState::Unreadable(e) => unpinned(
                format!("it could not be read ({e}), so it was left untouched and this server runs unpinned"),
                false,
            ),
            FileState::Corrupt(e) => match self.move_aside() {
                Ok(Some(to)) => unpinned(
                    format!(
                        "it is corrupt ({e}); it was moved to {} and ALL pins were reset, for \
                         every server: each is pinned again on its next run, and a rug-pull \
                         from before now goes unreported",
                        to.display()
                    ),
                    true,
                ),
                Ok(None) => self.load(),
                Err(io) => unpinned(
                    format!(
                        "it is corrupt ({e}) and could not be moved aside ({io}), so it was \
                         left untouched and this server runs unpinned"
                    ),
                    false,
                ),
            },
        }
    }

    /// Move a corrupt file to `mcp-pins.json.corrupt-<timestamp>`, under the
    /// lock. `None` when, re-read under the lock, it is no longer corrupt
    /// (another proxy got there first).
    fn move_aside(&self) -> io::Result<Option<PathBuf>> {
        let lock = self.lock()?;
        if !matches!(read(&self.path()), FileState::Corrupt(_)) {
            return Ok(None);
        }
        let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%S%.3fZ");
        let to = self.dir.join(format!("{FILE_NAME}.corrupt-{stamp}"));
        fs::rename(self.path(), &to)?;
        drop(lock);
        Ok(Some(to))
    }

    fn lock(&self) -> io::Result<File> {
        create_private_dir(&self.dir)?;
        let lock = open_private(&self.dir.join(LOCK_NAME), false)?;
        lock.lock()?;
        Ok(lock)
    }

    /// Store this server's pin, replacing any it had and keeping every other
    /// server's. Refused for a file this version must leave untouched.
    pub fn save(&self, pin: &Pin) -> io::Result<()> {
        let lock = self.lock()?;
        let mut file = match read(&self.path()) {
            FileState::Missing | FileState::Corrupt(_) => PinFile::default(),
            FileState::Read(file) => file,
            FileState::Newer(version) => {
                return Err(io::Error::other(format!(
                    "its version {version} is newer than this promptguard understands"
                )))
            },
            FileState::Unreadable(e) => return Err(io::Error::other(e)),
        };
        file.version = FORMAT_VERSION;
        file.servers.insert(self.key.clone(), pin.clone());
        let text = serde_json::to_string_pretty(&file).map_err(io::Error::other)?;
        let result = replace(&self.dir, &self.path(), text.as_bytes());
        drop(lock);
        result
    }
}

/// A pin as the endpoint writes them: 64 lowercase hex digits. Anything else
/// would be refused with a 422 if sent, so it is treated as no pin.
fn is_pin(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// The SHA-256, in hex, of the command line and the canonical directory the
/// server runs in, as JSON: `{"command": [...], "cwd": "..."}`.
pub fn server_key(command: &[String], cwd: Option<&Path>) -> String {
    let identity = serde_json::json!({
        "command": command,
        "cwd": cwd.map(|d| d.to_string_lossy()),
    });
    sha256_hex(&serde_json::to_vec(&identity).unwrap_or_default())
}

/// The SHA-256 of `bytes`, as 64 lowercase hex digits.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = aws_lc_rs::digest::digest(&aws_lc_rs::digest::SHA256, bytes);
    digest
        .as_ref()
        .iter()
        .fold(String::with_capacity(64), |mut hex, byte| {
            use std::fmt::Write as _;
            let _ = write!(hex, "{byte:02x}");
            hex
        })
}

/// The file as it is on disk. The version is read before the shape, so a
/// newer file whose shape this version does not know is not taken for a
/// corrupt one.
fn read(path: &Path) -> FileState {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return FileState::Missing,
        Err(e) => return FileState::Unreadable(e.to_string()),
    };
    let value: serde_json::Value = match serde_json::from_str(&text) {
        Ok(value) => value,
        Err(e) => return FileState::Corrupt(e.to_string()),
    };
    match value.get("version").and_then(serde_json::Value::as_u64) {
        Some(version) if version > u64::from(FORMAT_VERSION) => FileState::Newer(version),
        _ => serde_json::from_value(value)
            .map_or_else(|e| FileState::Corrupt(e.to_string()), FileState::Read),
    }
}

/// Write `contents` to a new owner-only file beside `path`, then rename it
/// over `path`, so a reader sees the old file or the new one, never part of
/// either.
fn replace(dir: &Path, path: &Path, contents: &[u8]) -> io::Result<()> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let temp = dir.join(format!(".{FILE_NAME}.{}.{nanos}.tmp", std::process::id()));
    let written = (|| {
        let mut file = open_private(&temp, true)?;
        file.write_all(contents)?;
        file.sync_all()?;
        fs::rename(&temp, path)
    })();
    if written.is_err() {
        let _ = fs::remove_file(&temp);
    }
    written
}

/// Open a file readable by its owner only; `new` refuses an existing one.
fn open_private(path: &Path, new: bool) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true);
    if new {
        options.create_new(true);
    } else {
        options.create(true).truncate(false);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

/// `~/.promptguard`, created owner-only if it does not exist, as
/// `save_credentials` creates it.
fn create_private_dir(dir: &Path) -> io::Result<()> {
    if dir.is_dir() {
        return Ok(());
    }
    fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn cmd(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn the_key_is_a_hash_of_the_whole_command_line_and_its_directory() {
        let key = server_key(&cmd(&["npx", "-y", "server", "--token", "secret"]), None);
        assert_eq!(key.len(), 64);
        assert!(is_pin(&key));
        assert!(!key.contains("secret"));
        assert_ne!(key, server_key(&cmd(&["npx", "-y", "server"]), None));
        // Joining arguments differently is a different command.
        assert_ne!(
            server_key(&cmd(&["a b", "c"]), None),
            server_key(&cmd(&["a", "b c"]), None)
        );
        let here = server_key(&cmd(&["x"]), Some(Path::new("/a")));
        assert_ne!(here, server_key(&cmd(&["x"]), Some(Path::new("/b"))));
        assert_ne!(here, server_key(&cmd(&["x"]), None));
        // Known value: SHA-256 of `{"command":["x"],"cwd":"/a"}`.
        assert_eq!(here, sha256_hex(br#"{"command":["x"],"cwd":"/a"}"#));
    }

    #[test]
    fn a_malformed_stored_pin_is_no_pin() {
        let dir = tempfile::TempDir::new().unwrap();
        let store = PinStore::new(dir.path().to_path_buf(), &cmd(&["server"]), None);
        let mut pin = Pin {
            server: "mcp-proxy:server".into(),
            sha256: "A".repeat(64),
            tools: vec![],
            pinned_at: String::new(),
        };
        store.save(&pin).unwrap();
        assert_eq!(store.load().pin, None);
        pin.sha256 = "a".repeat(64);
        store.save(&pin).unwrap();
        assert_eq!(store.load().pin, Some(pin));
    }
}
