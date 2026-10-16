#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Keeps every install method the README lists honest against what a release
//! actually publishes.
//!
//! The README once offered `brew tap promptguard/tap` (no tap exists and no
//! workflow publishes a formula), `cargo install promptguard` (the crate is not
//! on crates.io) and a Linux ARM64 binary (the release matrix never built one).
//! Each read as fine until a user ran it. The release workflow is the source of
//! truth for what exists; these tests fail when the README, `install.sh` or
//! `promptguard update` promise something it does not publish.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

const REPO_URL: &str = "https://github.com/acebot712/promptguard-cli";

fn repo_file(rel: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// The `## Installation` section of the README, up to the next `## ` heading.
fn readme_install_section() -> String {
    let readme = repo_file("README.md");
    let start = readme
        .find("## Installation")
        .expect("README has an `## Installation` section");
    let rest = &readme[start + "## Installation".len()..];
    let end = rest.find("\n## ").unwrap_or(rest.len());
    rest[..end].to_string()
}

/// Asset names the release workflow publishes, read from its build matrix
/// (`name: promptguard-...`). Windows assets carry `.exe` (see the
/// "Prepare binary" step).
fn released_assets() -> BTreeSet<String> {
    let workflow = repo_file(".github/workflows/release-cli.yml");
    let assets: BTreeSet<String> = workflow
        .lines()
        .filter_map(|l| l.trim().strip_prefix("name: promptguard-"))
        .map(|suffix| {
            let name = format!("promptguard-{}", suffix.trim());
            if name.contains("windows") {
                format!("{name}.exe")
            } else {
                name
            }
        })
        .collect();
    assert!(
        assets.len() >= 3,
        "expected the release matrix to name its assets, found {assets:?}"
    );
    assets
}

/// Every `promptguard-<os>-<arch>[.exe]` token in `text`.
fn asset_tokens(text: &str) -> BTreeSet<String> {
    let re = regex::Regex::new(r"promptguard-(?:macos|linux|windows)-[a-z0-9_]+(?:\.exe)?")
        .expect("valid regex");
    re.find_iter(text).map(|m| m.as_str().to_string()).collect()
}

fn cargo_install_command() -> String {
    format!(
        "cargo install --locked --git {REPO_URL} --tag cli-v{}",
        env!("CARGO_PKG_VERSION")
    )
}

#[test]
fn readme_offers_no_homebrew_install() {
    let section = readme_install_section().to_lowercase();
    assert!(
        !section.contains("brew"),
        "no Homebrew tap is published; the README must not offer one"
    );
}

#[test]
fn readme_binary_table_matches_the_release_matrix() {
    let listed = asset_tokens(&readme_install_section());
    assert_eq!(
        listed,
        released_assets(),
        "the README's prebuilt-binary list must name exactly the assets \
         release-cli.yml publishes"
    );
}

#[test]
fn readme_cargo_install_builds_the_current_release_tag() {
    let section = readme_install_section();
    let expected = cargo_install_command();
    assert!(
        section.contains(&expected),
        "README must give `{expected}`: the crate is not on crates.io, so the \
         Cargo method installs the release tag from git, and the tag must be \
         the version Cargo.toml declares"
    );
    let bare = regex::Regex::new(r"cargo install (?:--force )?promptguard\b").unwrap();
    assert!(
        !bare.is_match(&section),
        "`cargo install promptguard` fails: no such crate on crates.io"
    );
}

#[test]
fn install_script_suggests_no_unpublished_crate() {
    let script = repo_file("install.sh");
    let bare = regex::Regex::new(r"cargo install (?:--force )?promptguard\b").unwrap();
    assert!(
        !bare.is_match(&script),
        "install.sh must not suggest `cargo install promptguard`: no such crate"
    );
}

/// A fake `uname` and `curl` on PATH: `uname` reports the requested platform,
/// `curl` records the URL it was asked for and fails, so `install.sh` stops
/// before installing anything.
#[cfg(unix)]
fn run_install_script(os: &str, arch: &str) -> (Option<String>, String) {
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;

    let dir = tempfile::TempDir::new().unwrap();
    let bin = dir.path().join("bin");
    fs::create_dir(&bin).unwrap();
    let log: PathBuf = dir.path().join("curl.log");

    let uname = format!(
        "#!/bin/sh\ncase \"$1\" in -s) echo {os} ;; -m) echo {arch} ;; *) echo {os} ;; esac\n"
    );
    let curl = format!(
        "#!/bin/sh\nfor a in \"$@\"; do case \"$a\" in http*) echo \"$a\" >> '{}' ;; esac; done\nexit 22\n",
        log.display()
    );
    for (name, body) in [("uname", uname), ("curl", curl)] {
        let p = bin.join(name);
        fs::write(&p, body).unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    }

    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("install.sh");
    let path = format!("{}:/usr/bin:/bin", bin.display());
    let out = Command::new("/bin/sh")
        .arg(&script)
        .env("PATH", path)
        .output()
        .expect("run install.sh");
    let stdout =
        String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
    let first_url = fs::read_to_string(&log)
        .ok()
        .and_then(|l| l.lines().next().map(str::to_string));
    (first_url, stdout)
}

#[cfg(unix)]
#[test]
fn install_script_downloads_only_published_assets() {
    let assets = released_assets();
    for (os, arch) in [
        ("Darwin", "arm64"),
        ("Darwin", "x86_64"),
        ("Linux", "x86_64"),
        ("Linux", "amd64"),
    ] {
        let (url, output) = run_install_script(os, arch);
        let url = url.unwrap_or_else(|| panic!("{os}/{arch}: no download attempted\n{output}"));
        let asset = url
            .strip_prefix(&format!("{REPO_URL}/releases/latest/download/"))
            .unwrap_or_else(|| panic!("{os}/{arch}: unexpected URL {url}"));
        assert!(
            assets.contains(asset),
            "{os}/{arch}: install.sh downloads {asset}, which no release publishes ({assets:?})"
        );
    }
}

#[cfg(unix)]
#[test]
fn install_script_sends_unbuilt_platforms_to_the_cargo_method() {
    let (url, output) = run_install_script("Linux", "aarch64");
    assert!(
        url.is_none(),
        "no Linux ARM64 binary is published, yet install.sh fetched {url:?}"
    );
    assert!(
        output.contains(&cargo_install_command()),
        "install.sh should point Linux ARM64 at `{}`, got:\n{output}",
        cargo_install_command()
    );
}
