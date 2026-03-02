# ADR-017: Theme System

## Status

Accepted

## Context

LazyTail's UI colors were hardcoded, making it difficult to adapt to different terminal color schemes and user preferences. Users on light-background terminals had poor contrast, and there was no way to match the tool's appearance to their terminal emulator theme.

The theme system needed to:
- Support both light and dark terminal backgrounds
- Allow users to reuse color schemes from their terminal emulator
- Provide sensible defaults without configuration
- Support partial overrides (change one color without redefining everything)

## Decision

Implement a **two-level color system** with YAML-based themes:

1. **Palette layer**: 16 ANSI-style colors plus foreground, background, and selection colors. This maps to the terminal emulator's color model.
2. **UI colors layer**: Semantic colors derived from the palette (e.g., `severity_error`, `filter_query`, `selection_bg`). Automatically computed from palette via `derive_ui_colors()`.

Theme resolution follows a priority chain: project theme (from `lazytail.yaml`) > global theme (from `~/.config/lazytail/themes/`) > built-in default.

Light/dark detection: `Palette.is_background_light()` checks background luminance and adjusts derived UI colors accordingly.

**Multi-format import** via `lazytail theme import`: converts color schemes from Windows Terminal (.json), Alacritty (.toml), Ghostty (.conf), and iTerm2 (.itermcolors) into LazyTail's YAML format. This lets users match their log viewer to their terminal without manual color copying.

Override composition: palette overrides trigger re-derivation of UI colors, then explicit UI overrides are applied on top — enabling both automatic adaptation and manual fine-tuning.

## Consequences

**Benefits:**
- Automatic light/dark adaptation without user configuration
- One-command import from popular terminal emulators
- Partial overrides: change one palette color and all dependent UI colors update automatically
- YAML themes are human-readable and shareable

**Trade-offs:**
- Two-level indirection (palette → UI colors) adds complexity to the color resolution path
- Import from external formats may lose nuance (e.g., iTerm2 has more than 16 colors)
- Theme files must be discovered from multiple directories, adding startup I/O
