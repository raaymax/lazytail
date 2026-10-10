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

# Parse flags
NIGHTLY=false
FORCE_MUSL=false
INSTALL_METHOD=""  # "aur" or "binary"; empty means ask on Arch
for arg in "$@"; do
    case "$arg" in
        --nightly) NIGHTLY=true ;;
        --musl) FORCE_MUSL=true ;;
        --aur) INSTALL_METHOD="aur" ;;
        --no-aur) INSTALL_METHOD="binary" ;;
    esac
done

echo -e "${GREEN}LazyTail Installer${NC}"
if [ "$NIGHTLY" = true ]; then
    echo -e "${YELLOW}(nightly build)${NC}"
fi
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

# Detect whether the system C library is musl (e.g. Alpine, Void musl)
is_musl() {
    if ls /lib/ld-musl-*.so.1 &> /dev/null; then
        return 0
    fi
    if command -v ldd &> /dev/null && ldd --version 2>&1 | grep -qi musl; then
        return 0
    fi
    return 1
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

# Pick glibc or musl build on Linux
LIBC_SUFFIX=""
if [ "$PLATFORM" = "linux" ]; then
    if [ "$FORCE_MUSL" = true ] || is_musl; then
        if [ "$ARCH_SUFFIX" != "x86_64" ]; then
            echo -e "${RED}musl builds are only available for x86_64 (detected: $ARCH)${NC}"
            exit 1
        fi
        LIBC_SUFFIX="-musl"
        echo -e "${BLUE}Using musl (statically linked) build${NC}"
        echo ""
    fi
fi

# Handle Arch Linux: let the user choose between AUR and a direct binary download
if [ "$DISTRO" = "arch" ] && [ "$INSTALL_METHOD" != "binary" ]; then
    echo -e "${BLUE}Arch Linux detected!${NC}"
    echo ""

    # Check for AUR helpers
    AUR_HELPER=""
    if command -v yay &> /dev/null; then
        AUR_HELPER="yay"
    elif command -v paru &> /dev/null; then
        AUR_HELPER="paru"
    fi

    if [ -z "$INSTALL_METHOD" ]; then
        echo "How do you want to install LazyTail?"
        echo ""
        echo "  1) Download prebuilt binary from GitHub releases (default)"
        if [ -n "$AUR_HELPER" ]; then
            echo "  2) Install from AUR using $AUR_HELPER (pacman-managed updates)"
        else
            echo "  2) Install from AUR (requires an AUR helper such as yay or paru)"
        fi
        echo ""
        echo "Note: AUR packages are user-submitted. Review the PKGBUILD before installing."
        echo ""
        echo -n "Choice [1-2]: "
        read -r choice < /dev/tty
        case "$choice" in
            2) INSTALL_METHOD="aur" ;;
            *) INSTALL_METHOD="binary" ;;
        esac
        echo ""
    fi

    if [ "$INSTALL_METHOD" = "aur" ]; then
        if [ -z "$AUR_HELPER" ]; then
            echo -e "${YELLOW}No AUR helper found (yay or paru).${NC}"
            echo "Install one, then run:"
            echo -e "    ${GREEN}yay -S lazytail${NC}"
            echo ""
            echo "Or re-run this script and choose the binary download."
            exit 1
        fi

        echo "Running: $AUR_HELPER -S lazytail"
        echo ""
        if $AUR_HELPER -S lazytail; then
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
    fi
fi

# Check for existing installation (for non-Arch or if user chose to continue)
if check_existing_install; then
    echo ""
    echo "Do you want to:"
    echo "  1) Update existing installation"
    echo "  2) Cancel"
    echo ""
    echo -n "Choice [1-2]: "
    read -r choice < /dev/tty

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

# Get release version
if [ "$NIGHTLY" = true ]; then
    echo "Fetching nightly build..."
    LATEST_VERSION="nightly"
else
    echo "Fetching latest release..."
    LATEST_VERSION=$(curl -sI "https://github.com/$REPO/releases/latest" | grep -i '^location:' | sed -E 's|.*/tag/||' | tr -d '[:space:]')

    if [ -z "$LATEST_VERSION" ]; then
        echo -e "${RED}Failed to fetch latest version${NC}"
        exit 1
    fi
fi

echo "Version: $LATEST_VERSION"

# Construct download URL
BINARY_NAME="lazytail-${PLATFORM}-${ARCH_SUFFIX}${LIBC_SUFFIX}.tar.gz"
DOWNLOAD_URL="https://github.com/$REPO/releases/download/$LATEST_VERSION/$BINARY_NAME"

echo "Downloading from: $DOWNLOAD_URL"

# Create temporary directory
TMP_DIR=$(mktemp -d)
trap 'rm -rf "$TMP_DIR"' EXIT

# Download and extract
cd "$TMP_DIR"
if ! curl -fL -o "$BINARY_NAME" "$DOWNLOAD_URL"; then
    echo -e "${RED}Failed to download binary${NC}"
    if [ -n "$LIBC_SUFFIX" ] && [ "$NIGHTLY" != true ]; then
        echo "This release may not include a musl build yet. Try: --nightly"
    fi
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
echo -e "${BLUE}Note:${NC} To update in the future, run 'lazytail update' or re-run this script."

# MCP Configuration Section
echo ""
echo -e "${GREEN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${GREEN}AI Assistant Integration (MCP)${NC}"
echo -e "${GREEN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo ""
echo "LazyTail can integrate with AI assistants via MCP (Model Context Protocol)."
echo "This allows AI assistants to search and analyze your log files."
echo ""

# Get the installed binary path
LAZYTAIL_BIN="$INSTALL_DIR/lazytail"

# Helper function to add JSON entry to mcpServers
add_mcp_json_config() {
    local config_file="$1"
    local config_dir=$(dirname "$config_file")

    mkdir -p "$config_dir"

    if [ -f "$config_file" ]; then
        # File exists - check if lazytail already configured
        if grep -q '"lazytail"' "$config_file" 2>/dev/null; then
            echo -e "  ${YELLOW}⚠ lazytail already configured in $config_file${NC}"
            return 0
        fi

        # Use jq for reliable cross-platform JSON manipulation
        # (avoids sed -i portability issues between macOS and Linux)
        if command -v jq &> /dev/null; then
            jq --arg bin "$LAZYTAIL_BIN" '.mcpServers.lazytail = {"command": $bin, "args": ["--mcp"]}' \
                "$config_file" > "$config_file.tmp" && mv "$config_file.tmp" "$config_file"
        else
            echo -e "  ${YELLOW}⚠ jq not found. Please add manually to $config_file:${NC}"
            echo '    "mcpServers": { "lazytail": { "command": "'"$LAZYTAIL_BIN"'", "args": ["--mcp"] } }'
            return 0
        fi
    else
        # Create new file
        cat > "$config_file" << EOF
{
  "mcpServers": {
    "lazytail": {
      "command": "$LAZYTAIL_BIN",
      "args": ["--mcp"]
    }
  }
}
EOF
    fi
    echo -e "  ${GREEN}✓ Configured in $config_file${NC}"
}

# Helper function for TOML config (Codex)
add_mcp_toml_config() {
    local config_file="$1"
    local config_dir=$(dirname "$config_file")

    mkdir -p "$config_dir"

    if [ -f "$config_file" ]; then
        if grep -q '\[mcp_servers\.lazytail\]' "$config_file" 2>/dev/null; then
            echo -e "  ${YELLOW}⚠ lazytail already configured in $config_file${NC}"
            return 0
        fi
        # Append to existing file
        echo "" >> "$config_file"
        cat >> "$config_file" << EOF
[mcp_servers.lazytail]
command = ["$LAZYTAIL_BIN", "--mcp"]
EOF
    else
        cat > "$config_file" << EOF
[mcp_servers.lazytail]
command = ["$LAZYTAIL_BIN", "--mcp"]
EOF
    fi
    echo -e "  ${GREEN}✓ Configured in $config_file${NC}"
}

# Detect available AI assistants
ASSISTANTS_FOUND=()

# Claude Code
if command -v claude &> /dev/null; then
    ASSISTANTS_FOUND+=("claude_code")
fi

# Claude Desktop
if [ -d "$HOME/.config/Claude" ] || [ -d "$HOME/Library/Application Support/Claude" ]; then
    ASSISTANTS_FOUND+=("claude_desktop")
fi

# OpenAI Codex
if command -v codex &> /dev/null || [ -d "$HOME/.codex" ]; then
    ASSISTANTS_FOUND+=("codex")
fi

# Gemini CLI
if command -v gemini &> /dev/null || [ -d "$HOME/.gemini" ]; then
    ASSISTANTS_FOUND+=("gemini")
fi

# Cursor
if [ -d "$HOME/.cursor" ] || command -v cursor &> /dev/null; then
    ASSISTANTS_FOUND+=("cursor")
fi

# Windsurf
if [ -d "$HOME/.codeium" ] || command -v windsurf &> /dev/null; then
    ASSISTANTS_FOUND+=("windsurf")
fi

# Zed
if [ -d "$HOME/.config/zed" ] || command -v zed &> /dev/null; then
    ASSISTANTS_FOUND+=("zed")
fi

if [ ${#ASSISTANTS_FOUND[@]} -eq 0 ]; then
    echo "No supported AI assistants detected."
    echo ""
    echo "To manually configure MCP later, see: https://github.com/$REPO#ai-assistant-integration-mcp"
else
    echo "Detected AI assistants: ${ASSISTANTS_FOUND[*]}"
    echo ""

    # Claude Code
    if [[ " ${ASSISTANTS_FOUND[*]} " =~ " claude_code " ]]; then
        echo -e "${BLUE}Claude Code${NC}"
        echo "  Configure MCP for Claude Code? [Y/n]"
        read -r response < /dev/tty
        case "$response" in
            [nN][oO]|[nN])
                echo "  Skipped."
                ;;
            *)
                if claude mcp add lazytail --scope user -- "$LAZYTAIL_BIN" --mcp 2>/dev/null; then
                    echo -e "  ${GREEN}✓ Added via 'claude mcp add'${NC}"
                else
                    echo -e "  ${RED}✗ Failed to add via CLI. Try manually: claude mcp add lazytail -- $LAZYTAIL_BIN --mcp${NC}"
                fi
                ;;
        esac
        echo ""
    fi

    # Claude Desktop
    if [[ " ${ASSISTANTS_FOUND[*]} " =~ " claude_desktop " ]]; then
        echo -e "${BLUE}Claude Desktop${NC}"
        echo "  Configure MCP for Claude Desktop? [Y/n]"
        read -r response < /dev/tty
        case "$response" in
            [nN][oO]|[nN])
                echo "  Skipped."
                ;;
            *)
                if [ "$PLATFORM" = "macos" ]; then
                    CONFIG_FILE="$HOME/Library/Application Support/Claude/claude_desktop_config.json"
                else
                    CONFIG_FILE="$HOME/.config/Claude/claude_desktop_config.json"
                fi
                add_mcp_json_config "$CONFIG_FILE"
                ;;
        esac
        echo ""
    fi

    # OpenAI Codex
    if [[ " ${ASSISTANTS_FOUND[*]} " =~ " codex " ]]; then
        echo -e "${BLUE}OpenAI Codex${NC}"
        echo "  Configure MCP for Codex? [Y/n]"
        read -r response < /dev/tty
        case "$response" in
            [nN][oO]|[nN])
                echo "  Skipped."
                ;;
            *)
                add_mcp_toml_config "$HOME/.codex/config.toml"
                ;;
        esac
        echo ""
    fi

    # Gemini CLI
    if [[ " ${ASSISTANTS_FOUND[*]} " =~ " gemini " ]]; then
        echo -e "${BLUE}Gemini CLI${NC}"
        echo "  Configure MCP for Gemini CLI? [Y/n]"
        read -r response < /dev/tty
        case "$response" in
            [nN][oO]|[nN])
                echo "  Skipped."
                ;;
            *)
                if command -v gemini &> /dev/null; then
                    if gemini mcp add lazytail -- "$LAZYTAIL_BIN" --mcp 2>/dev/null; then
                        echo -e "  ${GREEN}✓ Added via 'gemini mcp add'${NC}"
                    else
                        add_mcp_json_config "$HOME/.gemini/settings.json"
                    fi
                else
                    add_mcp_json_config "$HOME/.gemini/settings.json"
                fi
                ;;
        esac
        echo ""
    fi

    # Cursor
    if [[ " ${ASSISTANTS_FOUND[*]} " =~ " cursor " ]]; then
        echo -e "${BLUE}Cursor${NC}"
        echo "  Configure MCP for Cursor (global)? [Y/n]"
        read -r response < /dev/tty
        case "$response" in
            [nN][oO]|[nN])
                echo "  Skipped."
                ;;
            *)
                add_mcp_json_config "$HOME/.cursor/mcp.json"
                ;;
        esac
        echo ""
    fi

    # Windsurf
    if [[ " ${ASSISTANTS_FOUND[*]} " =~ " windsurf " ]]; then
        echo -e "${BLUE}Windsurf${NC}"
        echo "  Configure MCP for Windsurf? [Y/n]"
        read -r response < /dev/tty
        case "$response" in
            [nN][oO]|[nN])
                echo "  Skipped."
                ;;
            *)
                add_mcp_json_config "$HOME/.codeium/windsurf/mcp_config.json"
                ;;
        esac
        echo ""
    fi

    # Zed
    if [[ " ${ASSISTANTS_FOUND[*]} " =~ " zed " ]]; then
        echo -e "${BLUE}Zed${NC}"
        echo "  Configure MCP for Zed? [Y/n]"
        read -r response < /dev/tty
        case "$response" in
            [nN][oO]|[nN])
                echo "  Skipped."
                ;;
            *)
                ZED_SETTINGS="$HOME/.config/zed/settings.json"
                if [ -f "$ZED_SETTINGS" ]; then
                    if grep -q '"lazytail"' "$ZED_SETTINGS" 2>/dev/null; then
                        echo -e "  ${YELLOW}⚠ lazytail already configured in $ZED_SETTINGS${NC}"
                    else
                        echo -e "  ${YELLOW}⚠ Zed requires manual configuration.${NC}"
                        echo "  Add to $ZED_SETTINGS under 'context_servers':"
                        echo ""
                        echo "    \"context_servers\": {"
                        echo "      \"lazytail\": {"
                        echo "        \"command\": { \"path\": \"$LAZYTAIL_BIN\", \"args\": [\"--mcp\"] }"
                        echo "      }"
                        echo "    }"
                    fi
                else
                    echo -e "  ${YELLOW}⚠ Zed settings not found. Add to $ZED_SETTINGS:${NC}"
                    echo ""
                    echo "    \"context_servers\": {"
                    echo "      \"lazytail\": {"
                    echo "        \"command\": { \"path\": \"$LAZYTAIL_BIN\", \"args\": [\"--mcp\"] }"
                    echo "      }"
                    echo "    }"
                fi
                ;;
        esac
        echo ""
    fi
fi

echo -e "${GREEN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo ""
echo "Installation complete! Run 'lazytail --help' to get started."
