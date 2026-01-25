#!/usr/bin/env bash

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# GitHub repository
REPO="raaymax/lazytail"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"

echo -e "${GREEN}LazyTail Installer${NC}"
echo ""

# Detect distribution
detect_distro() {
    if [ -f /etc/arch-release ]; then
        echo "arch"
    elif [ -f /etc/debian_version ]; then
        echo "debian"
    elif [ -f /etc/fedora-release ]; then
        echo "fedora"
    else
        echo "unknown"
    fi
}

# Check if binary is already installed
check_existing_install() {
    if command -v lazytail &> /dev/null; then
        EXISTING_PATH=$(command -v lazytail)
        echo -e "${YELLOW}Found existing installation: $EXISTING_PATH${NC}"
        return 0
    fi
    return 1
}

# Detect OS and architecture
OS="$(uname -s)"
ARCH="$(uname -m)"
DISTRO=$(detect_distro)

case "$OS" in
    Linux*)
        PLATFORM="linux"
        ;;
    Darwin*)
        PLATFORM="macos"
        ;;
    *)
        echo -e "${RED}Unsupported operating system: $OS${NC}"
        exit 1
        ;;
esac

case "$ARCH" in
    x86_64|amd64)
        ARCH_SUFFIX="x86_64"
        ;;
    aarch64|arm64)
        ARCH_SUFFIX="aarch64"
        ;;
    *)
        echo -e "${RED}Unsupported architecture: $ARCH${NC}"
        exit 1
        ;;
esac

# Handle Arch Linux
if [ "$DISTRO" = "arch" ]; then
    echo -e "${BLUE}Arch Linux detected!${NC}"
    echo ""

    # Check for AUR helpers
    AUR_HELPER=""
    if command -v yay &> /dev/null; then
        AUR_HELPER="yay"
    elif command -v paru &> /dev/null; then
        AUR_HELPER="paru"
    fi

    if [ -n "$AUR_HELPER" ]; then
        echo "Recommended installation method for Arch Linux is via AUR."
        echo ""
        echo "Benefits of AUR installation:"
        echo "  • Integrated with pacman"
        echo "  • Automatic updates with system upgrades"
        echo "  • Easy removal and dependency tracking"
        echo ""

        if check_existing_install; then
            echo -e "${YELLOW}Found existing binary installation: $EXISTING_PATH${NC}"
            echo "You can remove it after AUR installation completes."
            echo ""
        fi

        echo "Do you want to install via AUR now using $AUR_HELPER? [Y/n]"
        read -r response

        case "$response" in
            [nN][oO]|[nN])
                echo "Continuing with binary install..."
                echo ""
                ;;
            *)
                echo ""
                echo "Running: $AUR_HELPER -S lazytail"
                echo ""
                $AUR_HELPER -S lazytail

                if [ $? -eq 0 ]; then
                    echo ""
                    echo -e "${GREEN}✓ LazyTail installed successfully via AUR!${NC}"

                    if check_existing_install && [[ "$EXISTING_PATH" != "/usr/bin/lazytail" ]]; then
                        echo ""
                        echo -e "${YELLOW}Cleanup: Remove old binary installation${NC}"
                        echo "  rm $EXISTING_PATH"
                    fi

                    exit 0
                else
                    echo -e "${RED}AUR installation failed. Falling back to binary install...${NC}"
                    echo ""
                fi
                ;;
        esac
    else
        echo "Recommended installation method for Arch Linux is via AUR:"
        echo ""
        echo "  Install an AUR helper first:"
        echo "    ${GREEN}sudo pacman -S --needed git base-devel && git clone https://aur.archlinux.org/yay.git && cd yay && makepkg -si${NC}"
        echo ""
        echo "  Then install lazytail:"
        echo "    ${GREEN}yay -S lazytail${NC}"
        echo ""
        echo "Or continue with binary install..."
        echo "Press Ctrl+C to cancel, or Enter to continue..."
        read -r
        echo ""
    fi
fi

# Check for existing installation (for non-Arch or if user chose to continue)
if check_existing_install; then
    echo ""
    echo "Do you want to:"
    echo "  1) Update existing installation"
    echo "  2) Cancel"
    echo ""
    read -p "Choice [1-2]: " choice

    case $choice in
        1)
            echo "Proceeding with update..."
            ;;
        *)
            echo "Installation cancelled."
            exit 0
            ;;
    esac
    echo ""
fi

# Get latest release version
echo "Fetching latest release..."
LATEST_VERSION=$(curl -s "https://api.github.com/repos/$REPO/releases/latest" | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/')

if [ -z "$LATEST_VERSION" ]; then
    echo -e "${RED}Failed to fetch latest version${NC}"
    exit 1
fi

echo "Latest version: $LATEST_VERSION"

# Construct download URL
BINARY_NAME="lazytail-${PLATFORM}-${ARCH_SUFFIX}.tar.gz"
DOWNLOAD_URL="https://github.com/$REPO/releases/download/$LATEST_VERSION/$BINARY_NAME"

echo "Downloading from: $DOWNLOAD_URL"

# Create temporary directory
TMP_DIR=$(mktemp -d)
trap 'rm -rf "$TMP_DIR"' EXIT

# Download and extract
cd "$TMP_DIR"
if ! curl -L -o "$BINARY_NAME" "$DOWNLOAD_URL"; then
    echo -e "${RED}Failed to download binary${NC}"
    exit 1
fi

echo "Extracting..."
tar xzf "$BINARY_NAME"

# Install binary
mkdir -p "$INSTALL_DIR"
mv lazytail "$INSTALL_DIR/lazytail"
chmod +x "$INSTALL_DIR/lazytail"

echo ""
echo -e "${GREEN}✓ LazyTail installed successfully!${NC}"
echo ""
echo "Installed to: $INSTALL_DIR/lazytail"
echo "Version: $LATEST_VERSION"
echo ""

# Check if install directory is in PATH
if [[ ":$PATH:" != *":$INSTALL_DIR:"* ]]; then
    echo -e "${YELLOW}Warning: $INSTALL_DIR is not in your PATH${NC}"
    echo ""
    echo "Add this to your shell profile (~/.bashrc, ~/.zshrc, etc.):"
    echo ""
    echo "    export PATH=\"\$PATH:$INSTALL_DIR\""
    echo ""
else
    echo "Run 'lazytail --help' to get started"
fi

# Reminder about updates for binary installs
echo ""
echo -e "${BLUE}Note:${NC} To update in the future, re-run this script."
if [ "$DISTRO" = "arch" ]; then
    echo "Consider switching to AUR for automatic updates: ${GREEN}yay -S lazytail${NC}"
fi
