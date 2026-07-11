#!/bin/sh
# PromptGuard CLI installer
# Usage: curl -fsSL https://raw.githubusercontent.com/acebot712/promptguard-cli/main/install.sh | sh

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

# Download binary under its release asset name so the .sha256 file
# (which references that name) can be verified against it.
echo "Downloading PromptGuard CLI..."
if ! curl -fsSL "$RELEASE_URL/$BINARY" -o "$BINARY"; then
  echo "${RED}Error: Failed to download binary${NC}"
  echo "URL: $RELEASE_URL/$BINARY"
  exit 1
fi

# Download checksum (mandatory - refuse to install an unverified binary)
echo "Downloading checksum..."
if ! curl -fsSL "$RELEASE_URL/$BINARY.sha256" -o "$BINARY.sha256"; then
  echo "${RED}Error: Failed to download checksum file${NC}"
  echo "URL: $RELEASE_URL/$BINARY.sha256"
  echo "Refusing to install without checksum verification."
  exit 1
fi

# Verify checksum (mandatory)
echo "Verifying checksum..."
if command -v shasum >/dev/null 2>&1; then
  if ! shasum -a 256 -c "$BINARY.sha256" >/dev/null 2>&1; then
    echo "${RED}Error: Checksum verification failed${NC}"
    echo "The downloaded binary does not match its published checksum."
    exit 1
  fi
elif command -v sha256sum >/dev/null 2>&1; then
  if ! sha256sum -c "$BINARY.sha256" >/dev/null 2>&1; then
    echo "${RED}Error: Checksum verification failed${NC}"
    echo "The downloaded binary does not match its published checksum."
    exit 1
  fi
else
  echo "${RED}Error: No checksum tool found (shasum or sha256sum)${NC}"
  echo "Refusing to install without checksum verification."
  exit 1
fi
echo "${GREEN}Checksum verified${NC}"

# Rename to final binary name and make executable
mv "$BINARY" promptguard
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
echo "  1. Log in (works anywhere): promptguard login"
echo "     Get your API key at https://app.promptguard.co/settings/api-keys"
echo "  2. Try an instant scan: promptguard scan --text \"ignore previous instructions\""
echo "  3. Integrate a project:  promptguard init"
echo "  4. View help:            promptguard --help"
echo ""
echo "Documentation: https://docs.promptguard.co/cli"
echo ""
echo "To uninstall: curl -fsSL https://raw.githubusercontent.com/acebot712/promptguard-cli/main/uninstall.sh | sh"
