#!/bin/bash
# Setup public CLI repository
# This script creates a new public repo and pushes the CLI code there

set -e

echo "🚀 Setting up public promptguard-cli repository"
echo ""

# Step 1: Create public repo (requires gh CLI)
echo "📦 Step 1: Creating public repository..."
if ! command -v gh &> /dev/null; then
    echo "❌ GitHub CLI (gh) not found. Install it first:"
    echo "   brew install gh"
    exit 1
fi

# Check if logged in
if ! gh auth status &> /dev/null; then
    echo "🔐 Logging into GitHub..."
    gh auth login
fi

# Create the repo (choose the organization or your personal account)
echo ""
echo "Where should we create the public repo?"
echo "1. Your personal account (acebot712/promptguard-cli)"
echo "2. Organization account (promptguard/promptguard-cli)"
read -p "Choose (1 or 2): " choice

if [ "$choice" = "1" ]; then
    REPO_OWNER="acebot712"
    REPO_NAME="promptguard-cli"
    REPO_FULL="$REPO_OWNER/$REPO_NAME"
elif [ "$choice" = "2" ]; then
    REPO_OWNER="promptguard"
    REPO_NAME="promptguard-cli"
    REPO_FULL="$REPO_OWNER/$REPO_NAME"
else
    echo "❌ Invalid choice"
    exit 1
fi

echo ""
echo "Creating: https://github.com/$REPO_FULL"
read -p "Continue? (y/n): " confirm

if [ "$confirm" != "y" ]; then
    echo "❌ Cancelled"
    exit 0
fi

# Create the repo
gh repo create "$REPO_FULL" --public --description "PromptGuard CLI - Drop-in LLM security for your applications" || echo "Repo might already exist, continuing..."

# Step 2: Extract CLI directory and push
echo ""
echo "📤 Step 2: Extracting CLI code and pushing to public repo..."

cd /Users/abhijoysarkar/projects/promptguard

# Create a temporary branch with just the CLI
git subtree split --prefix=promptguard-cli -b cli-public-branch

# Add new remote
git remote add cli-public "https://github.com/$REPO_FULL.git" 2>/dev/null || git remote set-url cli-public "https://github.com/$REPO_FULL.git"

# Push to new repo
echo "Pushing to $REPO_FULL..."
git push cli-public cli-public-branch:main --force

# Clean up
git branch -D cli-public-branch

echo ""
echo "✅ Success! Public repo created at:"
echo "   https://github.com/$REPO_FULL"
echo ""
echo "📋 Next steps:"
echo "1. Update install script URL in all docs to:"
echo "   curl -fsSL https://raw.githubusercontent.com/$REPO_FULL/main/install.sh | sh"
echo ""
echo "2. Update GitHub Actions workflow to use:"
echo "   Repository: $REPO_FULL"
echo ""
echo "3. Push a release tag:"
echo "   cd /Users/abhijoysarkar/projects/promptguard"
echo "   git tag -a cli-v1.0.0 -m 'Release v1.0.0'"
echo "   git push cli-public cli-v1.0.0"
echo ""
echo "4. GitHub Actions will build binaries and create release"
