# First Release: PromptGuard CLI v1.0.0

## Quick Start - Release in 5 Commands

```bash
# 1. Commit any pending changes
cd /Users/abhijoysarkar/projects/promptguard
git add -A
git commit -m "feat(cli): Production-ready Rust CLI v1.0.0

- Real AST transformations using Tree-sitter
- TypeScript, JavaScript, Python support
- 4 provider support (OpenAI, Anthropic, Cohere, HuggingFace)
- 12 production commands
- Automatic backups with safe revert
- 5.3MB optimized binary"

# 2. Push to main
git push origin staging  # Or your main branch

# 3. Create annotated tag
git tag -a cli-v1.0.0 -m "PromptGuard CLI v1.0.0

Production-ready Rust CLI with Tree-sitter AST transformations

Features:
- Real AST parsing (no regex)
- TypeScript/JavaScript/Python support
- 4 LLM providers
- 12 commands (init, scan, status, revert, etc.)
- Automatic backup/restore
- Single 5.3MB static binary

Platforms: macOS (ARM64/x86_64), Linux (x86_64/ARM64)"

# 4. Push tag (this triggers GitHub Actions)
git push origin cli-v1.0.0

# 5. Monitor release
open https://github.com/promptguard/promptguard/actions
```

**That's it!** GitHub Actions will build binaries for all platforms and create the release.

---

## What Happens Next

### In ~10 minutes:

GitHub Actions will:
1. ✅ Build macOS ARM64 binary
2. ✅ Build macOS x86_64 binary
3. ✅ Build Linux x86_64 binary
4. ✅ Build Linux ARM64 binary
5. ✅ Generate SHA256 checksums
6. ✅ Create GitHub release
7. ✅ Upload all binaries

### After release completes:

**Users can install immediately:**
```bash
curl -fsSL https://raw.githubusercontent.com/promptguard/promptguard/main/promptguard-cli/install.sh | sh
```

**Or download manually:**
- Visit: https://github.com/promptguard/promptguard/releases/latest
- Download binary for their platform
- Install and run

---

## Verify Release

Once GitHub Actions completes (check the Actions tab):

```bash
# 1. Visit release page
open https://github.com/promptguard/promptguard/releases/latest

# Should see 8 files:
# - promptguard-macos-arm64
# - promptguard-macos-arm64.sha256
# - promptguard-macos-x86_64
# - promptguard-macos-x86_64.sha256
# - promptguard-linux-x86_64
# - promptguard-linux-x86_64.sha256
# - promptguard-linux-arm64
# - promptguard-linux-arm64.sha256

# 2. Test install script in clean environment
cd /tmp && rm -rf install-test && mkdir install-test && cd install-test
curl -fsSL https://raw.githubusercontent.com/promptguard/promptguard/main/promptguard-cli/install.sh | sh

# 3. Verify installation
promptguard --version
# Output: promptguard 1.0.0

# 4. Test basic functionality
mkdir test && cd test
echo 'import OpenAI from "openai"; const client = new OpenAI({apiKey: "sk-test"});' > test.ts
promptguard scan
# Should detect OpenAI SDK

# 5. Test init
promptguard init --provider openai --api-key pg_sk_test_12345 -y
cat test.ts
# Should have baseURL added

# 6. Test revert
promptguard revert -y
cat test.ts
# Should be back to original
```

---

## Pre-Release Checklist

Before pushing the tag, verify:

- [x] ✅ All code tested locally
- [x] ✅ Release binary built: `cargo build --release`
- [x] ✅ Binary tested: All 12 commands work
- [x] ✅ Transformations tested: TypeScript and Python
- [x] ✅ Backup/restore tested: Files correctly restored
- [x] ✅ Version in Cargo.toml: `version = "1.0.0"`
- [x] ✅ README updated with installation instructions
- [x] ✅ GitHub Actions workflow exists: `.github/workflows/release-cli.yml`
- [x] ✅ Install script exists: `promptguard-cli/install.sh`
- [x] ✅ Install script is executable: `chmod +x install.sh`

**All checks passed!** ✅ Ready to release.

---

## If Something Goes Wrong

### GitHub Actions fails

**Check the logs:**
```bash
# Visit Actions tab
open https://github.com/promptguard/promptguard/actions

# Click on the failed workflow
# Check which build failed
# Common issues:
# - Cross-compilation setup (Linux ARM64)
# - Missing Rust target
# - Build timeout
```

**Fix and re-release:**
```bash
# Delete the tag
git tag -d cli-v1.0.0
git push origin :refs/tags/cli-v1.0.0

# Delete the GitHub release (if created)
# Go to releases page and delete manually

# Fix the issue in workflow
# Commit and push fix

# Re-create tag
git tag -a cli-v1.0.0 -m "..."
git push origin cli-v1.0.0
```

### Install script fails

**Test locally first:**
```bash
# Download binary manually
cd /tmp
curl -fsSL https://github.com/promptguard/promptguard/releases/download/cli-v1.0.0/promptguard-macos-arm64 -o promptguard
chmod +x promptguard
./promptguard --version

# If this works, issue is in install script
# If this fails, issue is with binary
```

### Binary doesn't work

**Check architecture:**
```bash
file promptguard-macos-arm64
# Should show: Mach-O 64-bit executable arm64

ldd promptguard-linux-x86_64  # On Linux
# Should be statically linked (minimal dependencies)
```

---

## Post-Release Tasks

### Immediate (Day 1)

- [ ] Test install script on clean machine
- [ ] Update main project README if needed
- [ ] Announce on internal channels

### Week 1

- [ ] Monitor GitHub issues for installation problems
- [ ] Collect user feedback
- [ ] Plan bug fix release if needed (v2.0.1)

### Week 2-3 (Optional)

- [ ] Submit to Homebrew tap
- [ ] Publish to crates.io
- [ ] Create npm wrapper package

---

## Success Criteria

Release is successful when:

✅ GitHub Actions completes without errors
✅ All 8 files present in release
✅ Install script downloads and installs CLI
✅ `promptguard --version` outputs v1.0.0
✅ `promptguard init` successfully transforms code
✅ No critical bugs reported in first 24 hours

---

## Repository Check

**Before releasing, ensure:**

```bash
# GitHub repository exists
# URL: https://github.com/promptguard/promptguard

# You have permissions
# - Push access
# - Tag creation access
# - Release creation access

# GitHub Actions enabled
# Settings > Actions > General > Allow all actions

# Workflow permissions
# Settings > Actions > General > Workflow permissions > Read and write
```

---

## Ready?

**Run the 5 commands at the top of this file.**

The entire process takes ~15 minutes from tag to published release.

**Good luck! 🚀**
