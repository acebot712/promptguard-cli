#!/bin/sh
# PromptGuard CLI installer
# Usage: curl -fsSL https://raw.githubusercontent.com/promptguard/promptguard/main/promptguard-cli/install.sh | sh

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Repository
REPO="acebot712/promptguard-cli"
RELEASE_URL="https://github.com/$REPO/releases/latest/download"

echo "${GREEN}PromptGuard CLI Installer${NC}"
echo "================================"
echo ""

# Detect OS
OS="$(uname -s)"
case "$OS" in
  Darwin)
    OS_TYPE="macos"
    ;;
  Linux)
    OS_TYPE="linux"
    ;;
  *)
    echo "${RED}Error: Unsupported operating system: $OS${NC}"
    echo "Supported: macOS, Linux"
    exit 1
    ;;
esac

# Detect architecture
ARCH="$(uname -m)"
case "$ARCH" in
  x86_64|amd64)
    ARCH_TYPE="x86_64"
    ;;
  aarch64|arm64)
    ARCH_TYPE="arm64"
    ;;
  *)
    echo "${RED}Error: Unsupported architecture: $ARCH${NC}"
    echo "Supported: x86_64, arm64"
    exit 1
    ;;
esac

# Construct binary name
BINARY="promptguard-${OS_TYPE}-${ARCH_TYPE}"

echo "Detected: $OS_TYPE ($ARCH_TYPE)"
echo "Binary: $BINARY"
echo ""

# Check for required tools
if ! command -v curl >/dev/null 2>&1; then
  echo "${RED}Error: curl is required but not installed${NC}"
  exit 1
fi

# Create temp directory
TMP_DIR=$(mktemp -d)
trap "rm -rf $TMP_DIR" EXIT

cd "$TMP_DIR"

# Download binary
echo "Downloading PromptGuard CLI..."
if ! curl -fsSL "$RELEASE_URL/$BINARY" -o promptguard; then
  echo "${RED}Error: Failed to download binary${NC}"
  echo "URL: $RELEASE_URL/$BINARY"
  exit 1
fi

# Download checksum
echo "Downloading checksum..."
if ! curl -fsSL "$RELEASE_URL/$BINARY.sha256" -o promptguard.sha256; then
  echo "${YELLOW}Warning: Could not download checksum file${NC}"
else
  # Verify checksum
  echo "Verifying checksum..."
  if command -v shasum >/dev/null 2>&1; then
    if ! shasum -a 256 -c promptguard.sha256 >/dev/null 2>&1; then
      echo "${RED}Error: Checksum verification failed${NC}"
      exit 1
    fi
  elif command -v sha256sum >/dev/null 2>&1; then
    if ! sha256sum -c promptguard.sha256 >/dev/null 2>&1; then
      echo "${RED}Error: Checksum verification failed${NC}"
      exit 1
    fi
  else
    echo "${YELLOW}Warning: No checksum tool found (shasum or sha256sum)${NC}"
  fi
fi

# Make executable
chmod +x promptguard

# Install
INSTALL_DIR="/usr/local/bin"
echo ""
echo "Installing to $INSTALL_DIR/promptguard..."

if [ -w "$INSTALL_DIR" ]; then
  mv promptguard "$INSTALL_DIR/promptguard"
else
  echo "${YELLOW}Need sudo permission to install to $INSTALL_DIR${NC}"
  sudo mv promptguard "$INSTALL_DIR/promptguard"
fi

# Verify installation
if ! command -v promptguard >/dev/null 2>&1; then
  echo "${RED}Error: Installation failed - promptguard not in PATH${NC}"
  exit 1
fi

VERSION=$(promptguard --version 2>&1 | head -1 || echo "unknown")

echo ""
echo "${GREEN}✓ PromptGuard CLI installed successfully!${NC}"
echo ""
echo "Installed: $INSTALL_DIR/promptguard"
echo "Version: $VERSION"
echo ""
echo "Next steps:"
echo "  1. Get your API key: https://app.promptguard.co/settings/api-keys"
echo "  2. Initialize in your project: promptguard init --api-key pg_sk_xxx"
echo "  3. View help: promptguard --help"
echo ""
echo "Documentation: https://docs.promptguard.co/cli"
echo ""
echo "To uninstall: curl -fsSL https://raw.githubusercontent.com/acebot712/promptguard-cli/main/uninstall.sh | sh"
