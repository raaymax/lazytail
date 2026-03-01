# Feature: Color Schemes & Theme Configuration

## Problem

All UI colors in LazyTail are hardcoded across 6 TUI files (~100+ call sites). Users cannot customize the appearance, and the dark-only color scheme doesn't adapt to light terminal backgrounds. Severity line backgrounds use fixed RGB values that clash with non-dark terminals.

## Goal

A theme system that:
- Ships built-in dark and light schemes (dark = current look, refined)
- Loads external theme files from `~/.config/lazytail/themes/` or `.lazytail/themes/`
- Supports inheritance (`base: dark`) with per-field overrides
- Imports iTerm2/Windows Terminal color schemes via CLI
- Uses a two-layer model: **palette** (16 ANSI colors + bg/fg/selection) → **semantic UI colors** (derived from palette, individually overridable)
- Configurable via `theme:` field in global `~/.config/lazytail/config.yaml` (primary), with project-level override in `lazytail.yaml`

## Requirements

### Must Have (Stage 1 — Core + Built-in Themes) — COMPLETE

- [x] **R1: Theme struct** — `Palette` (16 ANSI + foreground/background/selection) and `UiColors` (~25 semantic roles + source color array). All fields are `ratatui::style::Color`.
- [x] **R2: Palette → UiColors derivation** — `Palette::derive_ui_colors()` produces sensible defaults. Auto-detects light/dark via `is_background_light()` for severity background adjustment.
- [x] **R3: Built-in dark theme** — Matches original hardcoded look exactly. Default when no config.
- [x] **R4: Built-in light theme** — Adjusted colors: darker green/yellow/cyan for visibility, light severity backgrounds (lavender, light yellow, light red, light magenta).
- [x] **R5: Color parsing** — `parse_color()` handles 16 named colors (case-insensitive), hex 3/6-digit (`#f50`, `#ff5500`), and `"default"` → `Color::Reset`. Serde via `ThemeColor` newtype.
- [x] **R6: Config field** — `theme: Option<RawThemeConfig>` in `RawConfig`. `#[serde(untagged)]` enum accepts string or struct. Primary home is global `config.yaml`; project `lazytail.yaml` overrides.
- [x] **R7: Theme on App** — `pub theme: Theme` on `App` struct. Set from resolved config in `main.rs`.
- [x] **R8: Replace hardcoded colors** — All `Color::` constants removed. All TUI modules use `app.theme.ui.*`. `help.rs` and `aggregation_view.rs` receive `&UiColors` parameter.
- [x] **R9: Configurable source colors** — `CombinedReader::source_info()` accepts `&[Color]` from `theme.ui.source_colors`.
- [x] **R10: Error handling** — `resolve_named()` suggests built-in names via Jaro-Winkler similarity. `"theme"` added to `ROOT_FIELDS` in `config/error.rs`.

### Should Have (Stage 2 — External Files + Import) — COMPLETE

- [x] **R11: External theme files** — YAML files in `~/.config/lazytail/themes/` (global) or `.lazytail/themes/` (project). `collect_themes_dirs()` discovers both directories; `discover_themes()` scans for `.yaml` files.
- [x] **R12: Theme inheritance** — `base:` field works for built-in and external theme names. Recursive `load_theme_file()` resolves external-to-external inheritance with cycle detection.
- [x] **R13: Inline overrides in config** — `theme: { base: dark, ui: { primary: cyan } }` works. Palette overrides re-derive UI colors, then UI overrides applied on top.
- [x] **R14: iTerm2 import CLI** — `lazytail theme import <file> --name <name>` parses Windows Terminal JSON, validates required keys, maps "purple" → "magenta", writes YAML to `~/.config/lazytail/themes/`.
- [x] **R15: Theme list CLI** — `lazytail theme list` shows built-in themes (dark, light) and discovered external themes from themes directories.

### Could Have (Stage 3 — Polish)

- [ ] **R16: Theme hot-reload** — Watch theme file for changes, re-apply without restart.
- [ ] **R17: Web viewer theming** — Pass theme to web SPA via API.
- [ ] **R18: MCP theme info** — Expose current theme palette in `get_stats` or a new tool.
- [ ] **R19: 256-color palette** — Extended palette beyond 16 ANSI for more granular themes.
- [ ] **R20: `NO_COLOR` support** — Respect `NO_COLOR` env var convention.

### Won't Have (out of scope)

- Per-line dynamic color rules (that's rendering presets territory)
- Lua/scripting-based theme logic
- Terminal profile detection (user picks their theme explicitly)

## Current Color Inventory

All hardcoded colors that will be replaced:

| File | Constants/Inline | Colors Used |
|------|-----------------|-------------|
| `src/tui/log_view.rs` | 5 consts + ~15 inline | DarkGray, White, Yellow, Rgb(30,30,40), Rgb(50,40,0), Rgb(55,10,10), Rgb(75,0,15) |
| `src/tui/side_panel.rs` | ~20 inline | Magenta, Cyan, Green, DarkGray, Yellow, White, Black, Red |
| `src/tui/status_bar.rs` | ~8 inline | Magenta, DarkGray, Green, Yellow, Red, Cyan, White |
| `src/tui/help.rs` | ~12 inline | Yellow, Cyan, Green, DarkGray, Magenta, Red, Black, White |
| `src/tui/aggregation_view.rs` | ~6 inline | Magenta, DarkGray, Cyan, White, Yellow |
| `src/reader/combined_reader.rs` | 1 const array | Cyan, Green, Yellow, Magenta, Blue, Red, LightCyan, LightGreen |

## YAML Config Format

### Global config (`~/.config/lazytail/config.yaml`) — primary location

```yaml
# Simple: select built-in
theme: dark

# Or: select external theme file
theme: dracula         # looks up dracula.yaml in themes dirs

# Or: inline overrides
theme:
  base: dark
  palette:
    red: "#ff6666"
  ui:
    selection_bg: "#505050"
    primary: cyan
```

### Project config (`lazytail.yaml`) — optional override

```yaml
# Override for this project (takes precedence over global)
theme: light

# Or with overrides on top of global
theme:
  base: dark
  ui:
    severity_error_bg: "#550000"
```

### External theme file (`~/.config/lazytail/themes/dracula.yaml`)

```yaml
name: "Dracula"
base: dark                        # inherit, override below

palette:
  black: "#21222c"
  red: "#ff5555"
  green: "#50fa7b"
  yellow: "#f1fa8c"
  blue: "#bd93f9"
  magenta: "#ff79c6"
  cyan: "#8be9fd"
  white: "#f8f8f2"
  bright_black: "#6272a4"
  bright_red: "#ff6e6e"
  bright_green: "#69ff94"
  bright_yellow: "#ffffa5"
  bright_blue: "#d6acff"
  bright_magenta: "#ff92df"
  bright_cyan: "#a4ffff"
  bright_white: "#ffffff"
  foreground: "#f8f8f2"
  background: "#282a36"
  selection: "#44475a"

ui:                               # optional overrides
  expanded_bg: "#1e1e2e"
```

## Semantic Color Roles

Each role has a default derived from palette. Users override only what they want to change.

| Role | Default (dark) | Used for |
|------|---------------|----------|
| **Core abstract** | | |
| `fg` | palette.white | Default text, tab names, aggregation values |
| `muted` | palette.bright_black | Hints, disabled items, ended source, trace severity |
| `accent` | palette.cyan | Headers, regex border, aggregation headers, line jump |
| `highlight` | palette.magenta | Loading, query border, combined tab, aggregation bar |
| `primary` | palette.yellow | Focus, active tab, filter text, help title |
| `positive` | palette.green | Success, active source, follow mode, confirm yes |
| `negative` | palette.red | Errors, confirm no, error border |
| **Selection** | | |
| `selection_bg` | palette.bright_black | Selected line background |
| `selection_fg` | palette.bright_white | Selected line foreground (readability) |
| **Log line backgrounds** | | |
| `expanded_bg` | `#1e1e28` | Expanded line background |
| `severity_warn_bg` | `#322800` | Warning line background |
| `severity_error_bg` | `#370a0a` | Error line background |
| `severity_fatal_bg` | `#4b000f` | Fatal line background |
| **Severity chart** | | |
| `severity_fatal` | palette.magenta | Fatal bars/indicators |
| `severity_error` | palette.red | Error bars/indicators |
| `severity_warn` | palette.yellow | Warning bars/indicators |
| `severity_info` | palette.green | Info bars/indicators |
| `severity_debug` | palette.cyan | Debug bars/indicators |
| `severity_trace` | palette.bright_black | Trace bars/indicators |
| **Filter input border** | | |
| `filter_plain` | palette.white | Plain text mode |
| `filter_regex` | palette.cyan | Regex mode |
| `filter_query` | palette.magenta | Query mode |
| `filter_error` | palette.red | Error state |
| **Popup** | | |
| `popup_bg` | palette.black | Popup/overlay background |
| **Source colors** | | |
| `source_colors` | [cyan, green, yellow, magenta, blue, red, light_cyan, light_green] | Combined view source cycling |

## Architecture

```
Config loading (resolution order):
  1. Built-in default (dark)
  2. Global config (~/.config/lazytail/config.yaml) theme: field
  3. Project config (lazytail.yaml) theme: field (overrides global)
  → resolve base → load palette → derive UiColors → apply overrides → Theme

Theme resolution:
  "dark"              → built-in dark Theme
  "dracula"           → load from themes dirs (~/.config/lazytail/themes/dracula.yaml)
  { base: dark, ... } → built-in dark → apply palette overrides → re-derive UI → apply UI overrides

Rendering:
  app.theme.ui.primary   →  Color (used directly in ratatui Style)
  app.theme.ui.muted     →  Color
  ...
```

### Module Structure

```
src/theme/
  mod.rs          — Theme, Palette, UiColors structs, built-in themes, public API
  loader.rs       — Theme file loading, resolution, inheritance
```

### Key Reuse Points

- `src/config/types.rs` — Existing `RawConfig`/`Config` structs, `deny_unknown_fields` pattern
- `src/config/error.rs` — Typo detection with Jaro-Winkler similarity, known fields arrays
- `src/config/discovery.rs` — `DiscoveryResult` for locating themes dirs alongside data dirs

## Files to Create/Modify

| File | Action | Description |
|------|--------|-------------|
| `src/theme/mod.rs` | Create | Theme, Palette, UiColors, built-in dark/light, `parse_color()` |
| `src/theme/loader.rs` | Create | `load_theme()`, file discovery, inheritance resolution |
| `src/config/types.rs` | Modify | Add `theme: Option<RawThemeConfig>` to `RawConfig` |
| `src/config/loader.rs` | Modify | Load theme from global config, apply project config override |
| `src/config/error.rs` | Modify | Add `"theme"` to `ROOT_FIELDS` for typo detection |
| `src/app/mod.rs` | Modify | Add `pub theme: Theme` to `App` |
| `src/tui/mod.rs` | Modify | Pass `&app.theme.ui` to `render_help_overlay` and `render_aggregation_view` |
| `src/tui/log_view.rs` | Modify | Remove 5 consts, use `theme.ui.*` throughout |
| `src/tui/side_panel.rs` | Modify | Replace ~20 inline `Color::` with `theme.ui.*` |
| `src/tui/status_bar.rs` | Modify | Replace ~8 inline `Color::` with `theme.ui.*` |
| `src/tui/help.rs` | Modify | Replace ~12 inline `Color::`, add `theme` parameter |
| `src/tui/aggregation_view.rs` | Modify | Replace ~6 inline `Color::`, add `theme` parameter |
| `src/reader/combined_reader.rs` | Modify | Accept `source_colors` instead of hardcoded `SOURCE_COLORS` |
| `src/cli/mod.rs` | Modify | Add `Theme` subcommand with `Import` and `List` actions |
| `src/cli/theme.rs` | Create | iTerm2 import and theme list CLI |
| `src/main.rs` | Modify | Register `theme` module, pass theme from config to App |

## Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Two-layer model | Palette + semantic UiColors | Palette enables trivial iTerm2 import (16 colors). Semantic layer gives UI-specific control. Users override either layer. |
| Palette → UI derivation | Automatic via `derive_ui_colors()` | Importing an iTerm2 scheme sets only palette; UI colors automatically adapt. Overrides are additive. |
| Theme on App | `pub theme: Theme` field | All render functions already receive `&App`. No signature changes for most functions. |
| Color format | Named + hex + "default" | Named for simplicity, hex for precision, "default" for terminal passthrough. Matches ratatui's `Color` enum. |
| Config format | `#[serde(untagged)]` enum | `theme: "dark"` (string) and `theme: { base: dark, ... }` (struct) both work cleanly. |
| Config precedence | Global → project override | Theme lives primarily in global `config.yaml` (user preference). Project `lazytail.yaml` can override for project-specific needs. Follows existing two-tier config pattern. |
| File discovery | `.lazytail/themes/` + `~/.config/lazytail/themes/` | Follows existing project/global scoping pattern from config discovery. |
| Built-in themes | Rust functions, not files | No filesystem dependency for defaults. Always available. |
| Import format | Windows Terminal JSON | Flat 20-key JSON, easy to parse with `serde_json`. Most common cross-format from iTerm2-Color-Schemes repo. |

## Implementation Progress

### Stage 1: Core + Built-in Themes — COMPLETE

- [x] `Theme`, `Palette`, `UiColors` structs (`src/theme/mod.rs`)
- [x] `parse_color()` — named/hex/default parsing (16 named colors + hex 3/6-digit + "default")
- [x] `Palette::derive_ui_colors()` — auto-derive semantics from palette, light/dark aware
- [x] Built-in `dark()` and `light()` themes (light has adjusted severity backgrounds)
- [x] `RawThemeConfig` serde type (string or struct with base/palette/ui via `#[serde(untagged)]`)
- [x] `theme` field on `RawConfig` and `Config`
- [x] Theme loading during config load: global config → project override (`src/config/loader.rs`)
- [x] Add `"theme"` to known root fields (`src/config/error.rs`)
- [x] `pub theme: Theme` on `App` struct
- [x] Replace hardcoded colors in `log_view.rs`
- [x] Replace hardcoded colors in `side_panel.rs`
- [x] Replace hardcoded colors in `status_bar.rs`
- [x] Replace hardcoded colors in `help.rs` (receives `&UiColors` param)
- [x] Replace hardcoded colors in `aggregation_view.rs` (receives `&UiColors` param)
- [x] Configurable `source_colors` in `combined_reader.rs` (passed via `source_info()`)
- [x] Inline overrides in config (`theme: { base: ..., ui: { ... } }`)
- [x] Tests: 18 tests covering color parsing, palette derivation, theme loading, overrides

### Stage 2: External Files + Import — COMPLETE

- [x] Theme file discovery (project + global themes dirs) — `collect_themes_dirs()` and `discover_themes()`
- [x] Theme file loading with inheritance (`base:` field) — recursive resolution with cycle detection, supports external-to-external inheritance
- [x] `lazytail theme import` CLI (Windows Terminal JSON → theme YAML) — validates keys, maps "purple" → "magenta"
- [x] `lazytail theme list` CLI (built-ins + discovered files)
- [x] Tests: file loading, inheritance, import, cycle detection, roundtrip validation

### Stage 3: Polish (future)

- [ ] Theme hot-reload on file change
- [ ] Web viewer theme support
- [ ] MCP theme info exposure
- [ ] `NO_COLOR` env var support

## Testing Strategy

1. **Unit tests** — Color parsing (named, hex, invalid), palette derivation, theme merge/override
2. **Config tests** — `theme: "dark"`, `theme: { base: dark, ui: { ... } }`, unknown theme error, typo suggestions
3. **Integration test** — Create temp theme file, load via config, verify resolved colors
4. **Manual verification:**
   ```bash
   # Default dark theme (should look identical to current):
   lazytail

   # Light theme (global config):
   # Add `theme: light` to ~/.config/lazytail/config.yaml, verify on light terminal

   # Project override:
   # Set `theme: dark` in ~/.config/lazytail/config.yaml
   # Set `theme: light` in lazytail.yaml → project wins, shows light

   # Custom override:
   # Add `theme: { base: dark, ui: { primary: cyan } }` to config.yaml
   # Verify focused borders are cyan

   # External theme file:
   # Create ~/.config/lazytail/themes/custom.yaml, set `theme: custom`

   # iTerm2 import:
   lazytail theme import Dracula.json --name dracula
   # Set `theme: dracula`, verify colors match Dracula scheme
   ```

## Known Issues

### Light theme: text nearly invisible, background still dark

When using `base: light`, the light palette's `foreground` and `background` colors are set on the `Palette` struct and flow into `UiColors` via `derive_ui_colors()`, but ratatui renders text against the **terminal's own background**, not the palette's `background`. The palette `background` field is never actually applied to any widget — it only influences `is_background_light()` for severity background derivation and `popup_bg` selection.

Result: `base: light` sets `fg` to black and `muted` to dark_gray, but the terminal background stays dark → black-on-dark text is unreadable. The palette `foreground`/`background` create an illusion of control that doesn't match what the terminal actually shows.

**Root cause:** LazyTail doesn't paint its own background — it relies on the terminal's background color. The light palette assumes a light terminal background but can't enforce it.

**Possible fixes:**
1. Paint explicit `bg()` on all rendered widgets using `palette.background` (invasive, affects every render call)
2. Remove `background` from palette — don't pretend we control it. Instead, document that `base: light` is for terminals with light backgrounds
3. Add a `force_background: true` option that paints `palette.background` on the root widget only (ratatui `Clear` with bg color)

## Open Questions

1. Should the light theme auto-detect terminal background color, or always require explicit `theme: light`? — Start with explicit selection, auto-detection is unreliable across terminals.
2. Should `NO_COLOR` env var force a "no color" theme (all defaults)? — Yes, respect the convention.
3. Should theme affect ANSI colors within log content, or only LazyTail's own UI chrome? — UI chrome only. Log content ANSI colors are passthrough from the source.
