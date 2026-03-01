#!/usr/bin/env bash
#
# Import iTerm2 color schemes from BenJanecke/iTerm2-Color-Schemes
# and convert them to lazytail theme YAML files.
#
# Usage: ./scripts/import-iterm2-themes.sh
#
# Clones the repo (shallow), converts all xrdb schemes to themes/,
# and cleans up.

set -euo pipefail

REPO_URL="https://github.com/BenJanecke/iTerm2-Color-Schemes.git"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
OUTPUT_DIR="$PROJECT_DIR/themes"
TMP_DIR="$(mktemp -d)"

trap 'rm -rf "$TMP_DIR"' EXIT

echo "Cloning iTerm2-Color-Schemes..."
git clone --depth 1 --quiet "$REPO_URL" "$TMP_DIR/repo"

XRDB_DIR="$TMP_DIR/repo/xrdb"
if [ ! -d "$XRDB_DIR" ]; then
    echo "error: xrdb/ directory not found in repo" >&2
    exit 1
fi

mkdir -p "$OUTPUT_DIR"

count=0

for xrdb_file in "$XRDB_DIR"/*.xrdb; do
    basename="$(basename "$xrdb_file" .xrdb)"

    # Generate kebab-case filename: lowercase, spaces/underscores to hyphens
    slug="$(echo "$basename" | tr '[:upper:]' '[:lower:]' | tr ' _' '--' | sed 's/[^a-z0-9-]//g' | sed 's/--*/-/g' | sed 's/^-//;s/-$//')"

    out_file="$OUTPUT_DIR/${slug}.yaml"

    # Parse xrdb defines into associative array
    declare -A colors=()
    while IFS= read -r line; do
        if [[ "$line" =~ ^#define[[:space:]]+([A-Za-z_0-9]+)[[:space:]]+(#[0-9a-fA-F]{6}) ]]; then
            key="${BASH_REMATCH[1]}"
            val="${BASH_REMATCH[2]}"
            colors["$key"]="$val"
        fi
    done < "$xrdb_file"

    # Map xrdb keys to lazytail palette fields
    # Ansi 0-7 = normal colors, Ansi 8-15 = bright colors
    black="${colors[Ansi_0_Color]:-}"
    red="${colors[Ansi_1_Color]:-}"
    green="${colors[Ansi_2_Color]:-}"
    yellow="${colors[Ansi_3_Color]:-}"
    blue="${colors[Ansi_4_Color]:-}"
    magenta="${colors[Ansi_5_Color]:-}"
    cyan="${colors[Ansi_6_Color]:-}"
    white="${colors[Ansi_7_Color]:-}"
    bright_black="${colors[Ansi_8_Color]:-}"
    bright_red="${colors[Ansi_9_Color]:-}"
    bright_green="${colors[Ansi_10_Color]:-}"
    bright_yellow="${colors[Ansi_11_Color]:-}"
    bright_blue="${colors[Ansi_12_Color]:-}"
    bright_magenta="${colors[Ansi_13_Color]:-}"
    bright_cyan="${colors[Ansi_14_Color]:-}"
    bright_white="${colors[Ansi_15_Color]:-}"
    foreground="${colors[Foreground_Color]:-}"
    background="${colors[Background_Color]:-}"
    selection="${colors[Selection_Color]:-${colors[Cursor_Color]:-}}"

    # Skip if missing essential colors
    if [ -z "$black" ] || [ -z "$foreground" ] || [ -z "$background" ]; then
        echo "  skip: $basename (missing essential colors)"
        continue
    fi

    # Use bright_black as selection fallback if neither Selection_Color nor Cursor_Color
    if [ -z "$selection" ]; then
        selection="$bright_black"
    fi

    # Write YAML theme file
    cat > "$out_file" << YAML
# ${basename} — imported from iTerm2-Color-Schemes
# https://github.com/BenJanecke/iTerm2-Color-Schemes
base: dark
palette:
  black: "${black}"
  red: "${red}"
  green: "${green}"
  yellow: "${yellow}"
  blue: "${blue}"
  magenta: "${magenta}"
  cyan: "${cyan}"
  white: "${white}"
  bright_black: "${bright_black}"
  bright_red: "${bright_red}"
  bright_green: "${bright_green}"
  bright_yellow: "${bright_yellow}"
  bright_blue: "${bright_blue}"
  bright_magenta: "${bright_magenta}"
  bright_cyan: "${bright_cyan}"
  bright_white: "${bright_white}"
  foreground: "${foreground}"
  background: "${background}"
  selection: "${selection}"
YAML

    count=$((count + 1))
    echo "  ok: $basename -> ${slug}.yaml"
    unset colors
done

echo ""
echo "Imported $count themes to $OUTPUT_DIR/"
