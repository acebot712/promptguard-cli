#!/bin/sh
# PromptGuard CLI uninstaller
# Usage: curl -fsSL https://raw.githubusercontent.com/acebot712/promptguard-cli/main/uninstall.sh | sh

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo "${BLUE}PromptGuard CLI Uninstaller${NC}"
echo "================================"
echo ""

# Check if PromptGuard is installed
INSTALL_DIR="/usr/local/bin"
BINARY_PATH="$INSTALL_DIR/promptguard"

if [ ! -f "$BINARY_PATH" ]; then
  echo "${YELLOW}PromptGuard is not installed at $BINARY_PATH${NC}"
  echo ""
  echo "If you installed via cargo, use:"
  echo "  cargo uninstall promptguard-cli"
  echo ""
  echo "Or run: make uninstall"
  exit 0
fi

# Show current version
if command -v promptguard >/dev/null 2>&1; then
  VERSION=$(promptguard --version 2>&1 | head -1 || echo "unknown")
  echo "Found: $VERSION"
  echo "Location: $BINARY_PATH"
  echo ""
fi

# Confirm uninstallation (unless --yes flag is passed)
if [ "$1" != "--yes" ] && [ "$1" != "-y" ]; then
  printf "Do you want to uninstall PromptGuard CLI? [y/N] "
  read -r response
  case "$response" in
    [yY][eE][sS]|[yY])
      ;;
    *)
      echo "${YELLOW}Uninstallation cancelled${NC}"
      exit 0
      ;;
  esac
fi

# Remove binary
echo ""
echo "Removing binary..."
if [ -w "$INSTALL_DIR" ]; then
  rm -f "$BINARY_PATH"
else
  echo "${YELLOW}Need sudo permission to remove from $INSTALL_DIR${NC}"
  sudo rm -f "$BINARY_PATH"
fi

# Check if removed successfully
if [ -f "$BINARY_PATH" ]; then
  echo "${RED}Error: Failed to remove $BINARY_PATH${NC}"
  exit 1
fi

echo "${GREEN}✓ Binary removed${NC}"

# Ask about configuration cleanup
CONFIG_DIR="$HOME/.promptguard"
if [ -d "$CONFIG_DIR" ]; then
  echo ""
  echo "Configuration directory found: $CONFIG_DIR"

  if [ "$1" != "--yes" ] && [ "$1" != "-y" ]; then
    printf "Do you want to remove configuration files? [y/N] "
    read -r config_response
    case "$config_response" in
      [yY][eE][sS]|[yY])
        rm -rf "$CONFIG_DIR"
        echo "${GREEN}✓ Configuration removed${NC}"
        ;;
      *)
        echo "${YELLOW}Configuration preserved${NC}"
        ;;
    esac
  else
    # In --yes mode, preserve config by default (safer)
    echo "${YELLOW}Configuration preserved (use manual removal if needed)${NC}"
  fi
fi

# Final confirmation
echo ""
echo "${GREEN}✓ PromptGuard CLI uninstalled successfully${NC}"
echo ""
echo "To reinstall, visit: https://github.com/acebot712/promptguard-cli"
echo ""
