//! Tool-list pins: what each wrapped server's `tools/list` was the first time
//! the proxy scanned it, kept in `~/.promptguard/mcp-pins.json`.
//!
//! The manifest-scan endpoint is stateless (acebot712/promptguard#942): it
//! returns a pin for the list it scanned and compares the list against the
//! pin it is sent. Keeping the pin is the caller's job, so it lives here, on
//! the developer's machine, one per server.
//!
//! A server is identified by the SHA-256 of its command line, so the file
//! never holds the arguments themselves, which can carry tokens. Nothing from
//! the environment is part of the key or stored.
//!
//! The file is a cache of trust decisions, never a reason to fail: missing or
//! unreadable, it is treated as empty and rewritten. Several proxies may run
//! at once (one per server an agent uses), so a write takes a lock, re-reads
//! the file under it, and replaces it with a rename, so a reader never sees a
//! half-written file and no proxy's pin is lost to another's write.

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

impl PinStore {
    pub fn new(dir: PathBuf, command: &[String]) -> Self {
        Self {
            dir,
            key: server_key(command),
        }
    }

    pub fn path(&self) -> PathBuf {
        self.dir.join(FILE_NAME)
    }

    /// This server's pin, if one is stored and well formed. The second value
    /// says why the file could not be read, when it could not; a missing file
    /// is not a problem and says nothing.
    pub fn load(&self) -> (Option<Pin>, Option<String>) {
        let (file, problem) = read(&self.path());
        let pin = file
            .servers
            .get(&self.key)
            .filter(|p| is_pin(&p.sha256))
            .cloned();
        (pin, problem)
    }

    /// Store this server's pin, keeping every other server's.
    pub fn save(&self, pin: &Pin) -> io::Result<()> {
        create_private_dir(&self.dir)?;
        let lock = open_private(&self.dir.join(LOCK_NAME), false)?;
        lock.lock()?;
        let (mut file, _) = read(&self.path());
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

/// The SHA-256 of the command line as a JSON array, in hex.
pub fn server_key(command: &[String]) -> String {
    sha256_hex(&serde_json::to_vec(command).unwrap_or_default())
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

/// The file's contents, or an empty set and why it could not be read.
fn read(path: &Path) -> (PinFile, Option<String>) {
    match fs::read_to_string(path) {
        Ok(text) => match serde_json::from_str(&text) {
            Ok(file) => (file, None),
            Err(e) => (PinFile::default(), Some(format!("it is not valid ({e})"))),
        },
        Err(e) if e.kind() == io::ErrorKind::NotFound => (PinFile::default(), None),
        Err(e) => (PinFile::default(), Some(e.to_string())),
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
    fn the_key_is_a_hash_of_the_whole_command_line() {
        let key = server_key(&cmd(&["npx", "-y", "server", "--token", "secret"]));
        assert_eq!(key.len(), 64);
        assert!(is_pin(&key));
        assert!(!key.contains("secret"));
        assert_ne!(key, server_key(&cmd(&["npx", "-y", "server"])));
        // Joining arguments differently is a different command.
        assert_ne!(
            server_key(&cmd(&["a b", "c"])),
            server_key(&cmd(&["a", "b c"]))
        );
        // Known value: SHA-256 of `["x"]`.
        assert_eq!(
            server_key(&cmd(&["x"])),
            "cd65ea2c2ad99e94a85b1b6df72efef9cb2ed0ae933a60c32ce16317f7d7d6aa"
        );
    }

    #[test]
    fn a_malformed_stored_pin_is_no_pin() {
        let dir = tempfile::TempDir::new().unwrap();
        let store = PinStore::new(dir.path().to_path_buf(), &cmd(&["server"]));
        let mut pin = Pin {
            server: "mcp-proxy:server".into(),
            sha256: "A".repeat(64),
            tools: vec![],
            pinned_at: String::new(),
        };
        store.save(&pin).unwrap();
        assert_eq!(store.load(), (None, None));
        pin.sha256 = "a".repeat(64);
        store.save(&pin).unwrap();
        assert_eq!(store.load(), (Some(pin), None));
    }
}
