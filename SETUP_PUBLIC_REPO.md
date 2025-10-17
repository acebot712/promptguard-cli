# Setting Up Public CLI Repository - Manual Approach

## Step 1: Create New Repository on GitHub

1. Go to https://github.com/new
2. Choose:
   - **Owner**: `acebot712` (your personal account) OR `promptguard` (organization)
   - **Repository name**: `promptguard-cli`
   - **Description**: "PromptGuard CLI - Drop-in LLM security for your applications"
   - **Visibility**: ✅ **Public**
3. **DO NOT** initialize with README, .gitignore, or license (we'll copy from existing)
4. Click "Create repository"

## Step 2: Clone and Setup Locally

```bash
# Navigate to a parent directory (NOT inside existing promptguard repo)
cd ~/projects  # or wherever you want the new repo

# Clone the new empty repo
git clone git@github.com:acebot712/promptguard-cli.git  # or promptguard/promptguard-cli
cd promptguard-cli

# Copy all CLI files from the private repo
cp -r ~/projects/promptguard/promptguard-cli/* .
cp ~/projects/promptguard/promptguard-cli/.gitignore .

# Initial commit
git add .
git commit -m "Initial commit: PromptGuard CLI v1.0.0"
git push origin main
```

## Step 3: Update Files in New Public Repo

### 3.1 Update GitHub Actions Workflow

Edit `.github/workflows/release-cli.yml`:

```yaml
# Change repository reference in upload step
- name: Upload Release Asset
  run: |
    gh release upload "${{ github.ref_name }}" "$BINARY_NAME" --repo acebot712/promptguard-cli
```

### 3.2 Update Install Script

Edit `install.sh` - change line 10:

```bash
# From:
REPO="promptguard/promptguard"

# To:
REPO="acebot712/promptguard-cli"  # or promptguard/promptguard-cli
```

### 3.3 Update README.md

Update installation command:

```bash
# From:
curl -fsSL https://raw.githubusercontent.com/promptguard/promptguard/main/promptguard-cli/install.sh | sh

# To:
curl -fsSL https://raw.githubusercontent.com/acebot712/promptguard-cli/main/install.sh | sh
```

### 3.4 Commit Updates

```bash
git add .github/workflows/release-cli.yml install.sh README.md
git commit -m "Update repository references for public CLI repo"
git push origin main
```

## Step 4: Create and Push Release Tag

```bash
# Tag the release
git tag -a cli-v1.0.0 -m "Release v1.0.0"

# Push the tag (this triggers GitHub Actions to build binaries)
git push origin cli-v1.0.0
```

## Step 5: Update Documentation in Private Repo

Back in the **private** promptguard repo, update these files:

### 5.1 Update `apps/docs/cli/installation.mdx`

```mdx
# Change line 11:
curl -fsSL https://raw.githubusercontent.com/acebot712/promptguard-cli/main/install.sh | sh
```

### 5.2 Update `apps/docs/cli/overview.mdx`

```mdx
# Update GitHub link (around line 15):
For the latest releases, visit our [GitHub releases page](https://github.com/acebot712/promptguard-cli/releases).
```

### 5.3 Update `apps/docs/docs.json` - Add GitHub link

```json
{
  "anchor": "CLI GitHub",
  "href": "https://github.com/acebot712/promptguard-cli",
  "icon": "code"
}
```

### 5.4 Commit documentation updates

```bash
cd ~/projects/promptguard
git add apps/docs/
git commit -m "docs: Update CLI installation URLs for public repository"
git push origin staging
```

## Step 6: Verify Everything Works

1. **Check GitHub Actions**: Go to https://github.com/acebot712/promptguard-cli/actions
   - Should see workflow running for cli-v1.0.0 tag
   - Wait for it to complete (builds 4 platform binaries)

2. **Check Release**: Go to https://github.com/acebot712/promptguard-cli/releases
   - Should see v1.0.0 release with 4 binary assets

3. **Test Install Script**:
   ```bash
   # In a clean directory
   curl -fsSL https://raw.githubusercontent.com/acebot712/promptguard-cli/main/install.sh | sh
   ./promptguard --version  # Should show 1.0.0
   ```

4. **Test Documentation**: Go to https://docs.promptguard.co/cli/installation
   - Verify install command works
   - Verify links point to correct repo

## Optional: Transfer to Organization

If you created under personal account but want it under organization:

```bash
# On GitHub:
# 1. Go to repo Settings
# 2. Scroll to "Danger Zone"
# 3. Click "Transfer ownership"
# 4. Transfer to "promptguard" organization
# 5. Update all URLs from acebot712 → promptguard
```

## Summary

After completion, you'll have:
- ✅ Public CLI repo at `github.com/acebot712/promptguard-cli`
- ✅ Automated binary builds on tag push
- ✅ One-line install script working
- ✅ Documentation updated and deployed
- ✅ Private backend repo unchanged

**Repository Structure:**
- **Private**: `acebot712/promptguard` (backend, dashboard, landing)
- **Public**: `acebot712/promptguard-cli` (CLI only)
