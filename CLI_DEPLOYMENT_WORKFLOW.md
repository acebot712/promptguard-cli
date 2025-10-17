# CLI Deployment Workflow - After Repository Split

## Repository Separation Summary

### Private Repo: `acebot712/promptguard`
**Location**: `/Users/abhijoysarkar/projects/promptguard`

**Contains**:
- ✅ Backend API (`backend/api/`)
- ✅ Dashboard (`apps/dashboard/`)
- ✅ Landing page (`apps/landing/`)
- ✅ Documentation (`apps/docs/`)
- ❌ **NO CLI CODE** (completely removed)

**CLI-related files that remain** (documentation only):
```
apps/docs/cli/
├── overview.mdx          # Links to acebot712/promptguard-cli
├── installation.mdx      # Install script from public repo
├── quickstart.mdx        # Usage guide
├── commands.mdx          # Command reference
├── configuration.mdx     # Config guide
└── troubleshooting.mdx   # FAQ

apps/docs/docs.json       # Navigation with "CLI GitHub" link
```

**These docs point to the public repo** - they don't contain any CLI code.

---

### Public Repo: `acebot712/promptguard-cli`
**Location**: `/Users/abhijoysarkar/projects/promptguard-cli`
**GitHub**: https://github.com/acebot712/promptguard-cli

**Contains**:
- ✅ All Rust CLI source code (`src/`)
- ✅ Cargo.toml + dependencies
- ✅ GitHub Actions workflow (`.github/workflows/release-cli.yml`)
- ✅ Install script (`install.sh`)
- ✅ README, LICENSE, docs
- ❌ **NO backend code**
- ❌ **NO frontend code**
- ❌ **NO Python**
- ❌ **NO TypeScript** (except Tree-sitter parsers)

---

## Complete Workflow Comparison

### Before (Monorepo)

```bash
# Private repo structure
promptguard/
├── backend/
├── apps/
└── promptguard-cli/     # CLI was here

# Deployment
cd promptguard/
git tag -a cli-v1.0.0 -m "Release"
git push origin cli-v1.0.0

# GitHub Actions would:
# 1. cd promptguard-cli/
# 2. cargo build --release
# 3. Upload binaries
```

**Problems**:
- ❌ Install script needs GitHub token (private repo)
- ❌ Users can't fork/contribute without backend access
- ❌ Mixed version history (backend + CLI)

---

### After (Separate Repos) ✅

```bash
# Two independent repositories
promptguard/              # Private (backend, dashboard, docs)
promptguard-cli/          # Public (CLI only)

# CLI Deployment (NEW WORKFLOW)
cd /Users/abhijoysarkar/projects/promptguard-cli
git tag -a cli-v1.0.1 -m "Release v1.0.1"
git push origin cli-v1.0.1

# GitHub Actions automatically:
# 1. Builds binaries for 4 platforms
# 2. Creates GitHub release
# 3. Uploads binaries + checksums
# 4. Users can install immediately
```

**Benefits**:
- ✅ No GitHub token needed (public repo)
- ✅ Users can fork/contribute freely
- ✅ Clean version history
- ✅ Independent releases

---

## New Deployment Workflows

### 1. CLI Release (Public Repo)

**Location**: `/Users/abhijoysarkar/projects/promptguard-cli`

```bash
# 1. Make changes to CLI code
cd /Users/abhijoysarkar/projects/promptguard-cli
vim src/commands/init.rs

# 2. Commit changes
git add .
git commit -m "feat: Add new validation in init command"
git push origin main

# 3. Create release tag
git tag -a cli-v1.0.1 -m "Release v1.0.1

- Add validation for API key format
- Fix bug in TypeScript detection
- Improve error messages"

# 4. Push tag (triggers GitHub Actions)
git push origin cli-v1.0.1

# 5. GitHub Actions automatically:
#    - Builds for macOS ARM64, macOS x86_64, Linux x86_64, Linux ARM64
#    - Creates release: https://github.com/acebot712/promptguard-cli/releases/tag/cli-v1.0.1
#    - Uploads binaries + checksums
#    - Users can install: curl -fsSL ... | sh
```

**That's it!** No involvement of private repo needed.

---

### 2. Backend/Dashboard Release (Private Repo)

**Location**: `/Users/abhijoysarkar/projects/promptguard`

```bash
# Same as before - nothing changed
cd /Users/abhijoysarkar/projects/promptguard

# Push to staging
git push origin staging
# → Deploys to api.staging.promptguard.co

# Push to main
git push origin main
# → Deploys to api.promptguard.co
```

**CLI is completely unaffected** - it's in a different repo.

---

### 3. Update CLI Documentation (Private Repo)

**Location**: `/Users/abhijoysarkar/projects/promptguard`

```bash
# If you need to update CLI docs (installation guide, etc.)
cd /Users/abhijoysarkar/projects/promptguard

# Edit docs
vim apps/docs/cli/installation.mdx

# Commit and push
git add apps/docs/cli/
git commit -m "docs: Update CLI installation instructions"
git push origin staging

# Deploy docs
cd apps/docs
mintlify dev  # Preview locally
# Then push to main to deploy
```

**Note**: This only updates the documentation website, not the CLI itself.

---

## GitHub Actions Workflow

### CLI Workflow (`.github/workflows/release-cli.yml` in public repo)

```yaml
name: Release CLI

on:
  push:
    tags:
      - 'cli-v*'  # Triggers on: cli-v1.0.0, cli-v1.0.1, etc.

jobs:
  build:
    strategy:
      matrix:
        include:
          - os: macos-latest
            target: aarch64-apple-darwin
            name: promptguard-macos-arm64
          - os: macos-latest
            target: x86_64-apple-darwin
            name: promptguard-macos-x86_64
          - os: ubuntu-latest
            target: x86_64-unknown-linux-gnu
            name: promptguard-linux-x86_64
          - os: ubuntu-latest
            target: aarch64-unknown-linux-gnu
            name: promptguard-linux-arm64

    steps:
      - uses: actions/checkout@v4
      - name: Install Rust
        uses: dtolnay/rust-toolchain@stable
      - name: Build
        run: cargo build --release --target ${{ matrix.target }}
      - name: Upload artifacts
        uses: actions/upload-artifact@v4

  release:
    needs: build
    steps:
      - name: Create Release
        uses: softprops/action-gh-release@v1
        with:
          files: release/*
```

**Key points**:
- ✅ No `working-directory: promptguard-cli` (repo root IS the CLI)
- ✅ No private repo references
- ✅ Runs on public repo only
- ✅ Anyone can see the workflow (transparency)

---

## Installing CLI (Users)

### From Public Repo (New Way)

```bash
# One-line install
curl -fsSL https://raw.githubusercontent.com/acebot712/promptguard-cli/main/install.sh | sh

# The script:
# 1. Detects OS/architecture
# 2. Downloads from: https://github.com/acebot712/promptguard-cli/releases/latest/download/promptguard-macos-arm64
# 3. Verifies checksum
# 4. Installs to /usr/local/bin/promptguard
```

**No authentication needed** - it's a public repo!

---

## Development Workflow

### Working on CLI

```bash
# 1. Clone public repo
git clone https://github.com/acebot712/promptguard-cli.git
cd promptguard-cli

# 2. Make changes
vim src/main.rs

# 3. Test locally
cargo build
./target/debug/promptguard --help

# 4. Run tests
cargo test

# 5. Build release
cargo build --release

# 6. Test release binary
./target/release/promptguard init --help

# 7. Commit and push
git add .
git commit -m "feat: Add new feature"
git push origin main

# 8. Create release (when ready)
git tag -a cli-v1.0.2 -m "Release v1.0.2"
git push origin cli-v1.0.2
```

### Working on Backend/Dashboard

```bash
# 1. Clone private repo (same as before)
git clone https://github.com/acebot712/promptguard.git
cd promptguard

# 2. Make changes (same as before)
vim backend/api/main.py

# 3. Test locally (same as before)
make local

# 4. Deploy (same as before)
git push origin staging
```

**Completely independent workflows!**

---

## Key Differences - At a Glance

| Aspect | Before (Monorepo) | After (Separate) |
|--------|------------------|------------------|
| **CLI Code Location** | `promptguard/promptguard-cli/` | `promptguard-cli/` (root) |
| **CLI Releases** | Tag in private repo | Tag in public repo |
| **Install Script URL** | Required GitHub token | Public URL |
| **Contributors** | Need backend access | Only need CLI access |
| **GitHub Actions** | `working-directory: promptguard-cli` | No subdirectory |
| **Version History** | Mixed backend+CLI | Pure CLI history |
| **Backend Deploys** | Might trigger CLI rebuild | Completely independent |
| **CLI Updates** | Update private repo | Update public repo |

---

## Common Questions

### Q: Where do I push CLI changes now?

**A**: To the public repo at `/Users/abhijoysarkar/projects/promptguard-cli`

```bash
cd /Users/abhijoysarkar/projects/promptguard-cli
git push origin main
```

### Q: How do I release a new CLI version?

**A**: Create and push a tag in the public repo:

```bash
cd /Users/abhijoysarkar/projects/promptguard-cli
git tag -a cli-v1.0.2 -m "Release v1.0.2"
git push origin cli-v1.0.2
```

### Q: Does the private repo have ANY CLI code now?

**A**: NO! Only documentation that links to the public repo.

```bash
cd /Users/abhijoysarkar/projects/promptguard
ls promptguard-cli/
# ls: promptguard-cli/: No such file or directory
```

### Q: Can users install without a GitHub token?

**A**: YES! It's a public repo:

```bash
curl -fsSL https://raw.githubusercontent.com/acebot712/promptguard-cli/main/install.sh | sh
# Works without authentication
```

### Q: What if I need to update CLI docs on the website?

**A**: Update the private repo's documentation:

```bash
cd /Users/abhijoysarkar/projects/promptguard
vim apps/docs/cli/installation.mdx
git push origin staging
```

This updates docs.promptguard.co, NOT the CLI itself.

### Q: What if backend and CLI need to stay in sync?

**A**: They don't need to!

- CLI talks to backend via REST API (`/api/v1/proxy`)
- API is versioned and backward-compatible
- CLI v1.0.0 can work with Backend v2.0.0
- If breaking API change needed, coordinate releases but they're still independent

### Q: How do I test CLI changes locally before releasing?

**A**: Build locally in the public repo:

```bash
cd /Users/abhijoysarkar/projects/promptguard-cli
cargo build --release
./target/release/promptguard --version
```

---

## Checklist for Future Releases

### Before Tagging

- [ ] All tests pass: `cargo test`
- [ ] Binary builds: `cargo build --release`
- [ ] Update version in `Cargo.toml`
- [ ] Update `CHANGELOG.md` (if you create one)
- [ ] Update README if features changed
- [ ] Commit all changes

### Creating Release

```bash
# 1. Update version
vim Cargo.toml  # version = "1.0.2"
git add Cargo.toml
git commit -m "chore: Bump version to 1.0.2"
git push origin main

# 2. Create tag
git tag -a cli-v1.0.2 -m "Release v1.0.2

Features:
- Add new command
- Improve error handling

Fixes:
- Fix ARM64 build issue"

# 3. Push tag
git push origin cli-v1.0.2

# 4. Wait for GitHub Actions
gh run watch  # Or check: https://github.com/acebot712/promptguard-cli/actions

# 5. Verify release
gh release view cli-v1.0.2

# 6. Test install
curl -fsSL https://raw.githubusercontent.com/acebot712/promptguard-cli/main/install.sh | sh
promptguard --version  # Should show 1.0.2
```

### After Release

- [ ] Announce on Twitter/LinkedIn
- [ ] Update docs.promptguard.co if needed
- [ ] Monitor GitHub issues for bug reports
- [ ] Check download counts

---

## Summary

**Yes, the workflow is exactly as you described:**

1. **Private repo (`promptguard`)**: No CLI code, only docs that link to public repo
2. **Public repo (`promptguard-cli`)**: All CLI code, completely independent
3. **Deployment**: Just push a tag from the CLI repo, GitHub Actions does the rest

```bash
# Release CLI (public repo)
cd /Users/abhijoysarkar/projects/promptguard-cli
git tag -a cli-v1.0.2 -m "Release"
git push origin cli-v1.0.2
# → Builds and releases automatically
```

**That's it!** No cross-repo coordination needed for normal CLI releases.

---

**Last Updated**: October 17, 2025
