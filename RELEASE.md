# PromptGuard CLI Release Guide

## Pre-Release Checklist

### Code Quality
- [ ] All tests pass locally: `cargo test`
- [ ] No compiler warnings: `cargo build --release 2>&1 | grep warning`
- [ ] Code formatted: `cargo fmt --check`
- [ ] Clippy checks pass: `cargo clippy`

### Functionality Testing
- [ ] Test on macOS (your current platform)
- [ ] Test init workflow: `promptguard init --api-key pg_sk_test_xxx -y`
- [ ] Test transformation: Verify baseURL added to TypeScript/Python files
- [ ] Test revert: Verify files restored to original state
- [ ] Test disable/enable: Verify toggle works correctly
- [ ] Test all 12 commands execute without errors

### Version Bump
- [ ] Update version in `Cargo.toml`: `version = "1.0.0"`
- [ ] Update version in README badges
- [ ] Update CHANGELOG (if exists) or create release notes

### Documentation
- [ ] README.md is up to date
- [ ] Installation instructions are correct
- [ ] All command examples work

---

## Release Process

### 1. Prepare Release

```bash
# Ensure you're on main/staging branch
git checkout main
git pull origin main

# Update version in Cargo.toml
# Current: version = "1.0.0"

# Commit version bump
git add promptguard-cli/Cargo.toml
git commit -m "chore(cli): bump version to 1.0.0"
git push origin main
```

### 2. Create and Push Tag

```bash
# Create annotated tag
git tag -a cli-v1.0.0 -m "PromptGuard CLI v1.0.0

- Real AST transformations using Tree-sitter
- TypeScript, JavaScript, Python support
- OpenAI, Anthropic, Cohere, HuggingFace providers
- Automatic backups with safe revert
- 12 production-ready commands
- Single 5.3MB static binary"

# Push tag (this triggers GitHub Actions)
git push origin cli-v1.0.0
```

### 3. Monitor GitHub Actions

```bash
# Go to: https://github.com/promptguard/promptguard/actions
# Watch the "Release CLI" workflow

# It will:
# 1. Build binaries for all 4 platforms (macOS arm64/x86_64, Linux x86_64/arm64)
# 2. Generate SHA256 checksums
# 3. Create GitHub release
# 4. Upload all binaries and checksums
```

### 4. Verify Release

Once GitHub Actions completes:

```bash
# Check release page
open https://github.com/promptguard/promptguard/releases/latest

# Verify all 8 files are present:
# - promptguard-macos-arm64
# - promptguard-macos-arm64.sha256
# - promptguard-macos-x86_64
# - promptguard-macos-x86_64.sha256
# - promptguard-linux-x86_64
# - promptguard-linux-x86_64.sha256
# - promptguard-linux-arm64
# - promptguard-linux-arm64.sha256
```

### 5. Test Installation

Test the install script works:

```bash
# Create a clean test environment
cd /tmp
rm -rf test-install
mkdir test-install
cd test-install

# Run install script
curl -fsSL https://raw.githubusercontent.com/promptguard/promptguard/main/promptguard-cli/install.sh | sh

# Verify installation
promptguard --version
# Should output: promptguard 1.0.0

# Test basic functionality
mkdir test-project
cd test-project
echo 'import OpenAI from "openai"; const client = new OpenAI({apiKey: "test"});' > test.ts
promptguard scan
# Should detect OpenAI SDK
```

### 6. Announce Release

Update the following:

- [ ] Main README.md (if CLI is mentioned)
- [ ] Documentation site (docs.promptguard.co)
- [ ] Changelog or Release notes
- [ ] Twitter/social media (optional)
- [ ] Discord/Slack community (if exists)

---

## Post-Release (Optional)

### Submit to Package Managers

#### Homebrew (Week 2)
```bash
# 1. Create Homebrew tap repository: promptguard/homebrew-tap
# 2. Create Formula/promptguard.rb:

class Promptguard < Formula
  desc "Drop-in LLM security for your applications"
  homepage "https://promptguard.co"
  version "1.0.0"

  if OS.mac? && Hardware::CPU.arm?
    url "https://github.com/promptguard/promptguard/releases/download/cli-v1.0.0/promptguard-macos-arm64"
    sha256 "CHECKSUM_HERE"
  elsif OS.mac?
    url "https://github.com/promptguard/promptguard/releases/download/cli-v1.0.0/promptguard-macos-x86_64"
    sha256 "CHECKSUM_HERE"
  elsif OS.linux? && Hardware::CPU.arm?
    url "https://github.com/promptguard/promptguard/releases/download/cli-v1.0.0/promptguard-linux-arm64"
    sha256 "CHECKSUM_HERE"
  elsif OS.linux?
    url "https://github.com/promptguard/promptguard/releases/download/cli-v1.0.0/promptguard-linux-x86_64"
    sha256 "CHECKSUM_HERE"
  end

  def install
    bin.install "promptguard-#{OS.mac? ? 'macos' : 'linux'}-#{Hardware::CPU.arm? ? 'arm64' : 'x86_64'}" => "promptguard"
  end

  test do
    system "#{bin}/promptguard", "--version"
  end
end

# Users install: brew install promptguard/tap/promptguard
```

#### Cargo / crates.io (Week 2)
```bash
# 1. Ensure Cargo.toml has proper metadata
# 2. cargo login
# 3. cargo publish

# Users install: cargo install promptguard-cli
```

#### npm (Week 3)
Create wrapper package that downloads binary in postinstall hook.

---

## Rollback Process

If something goes wrong:

```bash
# Delete the tag locally
git tag -d cli-v1.0.0

# Delete the tag remotely
git push origin :refs/tags/cli-v1.0.0

# Delete the GitHub release manually
# Go to: https://github.com/promptguard/promptguard/releases
# Click on the release -> Delete

# Fix the issue, then re-release with a patch version
# cli-v2.0.1
```

---

## Troubleshooting

### GitHub Actions fails to build

**Linux ARM64 cross-compilation issue:**
- The workflow installs `gcc-aarch64-linux-gnu`
- If it fails, check the build logs in GitHub Actions

**macOS builds fail:**
- Ensure you have the right Rust targets
- GitHub Actions runners are updated

### Install script fails

**Check the URL is correct:**
```bash
# Test manually:
curl -I https://github.com/promptguard/promptguard/releases/latest/download/promptguard-macos-arm64
# Should return 302 redirect (not 404)
```

**Checksum verification fails:**
- Verify the .sha256 files were uploaded
- Check they contain correct checksums

### Binary doesn't work on user's machine

**"cannot execute binary file":**
- User downloaded wrong architecture
- Update install script to better detect OS/arch

**"dyld: Library not loaded":**
- Shouldn't happen with static Rust binaries
- Check if dynamic linking was accidentally enabled

---

## Version Numbering

Follow semantic versioning:

- **Major (3.0.0)**: Breaking changes, major rewrites
- **Minor (2.1.0)**: New features, backward compatible
- **Patch (2.0.1)**: Bug fixes, no new features

Tag format: `cli-v{major}.{minor}.{patch}`

Examples:
- `cli-v1.0.0` - Initial Rust release
- `cli-v2.1.0` - Add new provider support
- `cli-v2.0.1` - Fix transformation bug
- `cli-v3.0.0` - Breaking API changes

---

## First Release Checklist (v1.0.0)

Since this is the first release of the Rust CLI:

- [ ] Verify GitHub repository exists: `promptguard/promptguard`
- [ ] Ensure you have push access to create tags
- [ ] GitHub Actions is enabled on the repository
- [ ] No restrictions on workflow permissions
- [ ] Test on your local macOS first
- [ ] Create the tag: `cli-v1.0.0`
- [ ] Monitor GitHub Actions workflow
- [ ] Test install script after release
- [ ] Update main README if needed
- [ ] Celebrate! 🎉

---

**Ready to release?** Run through the checklist and execute the commands above.
