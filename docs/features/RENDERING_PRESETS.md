# Feature: Rendering Presets

## Problem

Log lines come in many structured formats (JSON, logfmt, nginx access logs, custom patterns) but are displayed as raw text. Users must mentally parse `{"level":"error","timestamp":"2024-01-01T00:00:00Z","message":"connection failed","service":"api","duration_ms":42}` every time they look at a log line.

Each interface (TUI, MCP, web) handles rendering independently. There's no shared "how should this log line look" layer — adding a new log format means implementing it in 3 places.

## Goal

A YAML-configurable preset system that:
- Extracts fields from structured log lines (JSON, logfmt, regex)
- Lays them out with styling: `ERROR connection failed service=api duration_ms=42`
- Works identically across TUI and MCP
- Compiles once at startup — per-line rendering cost is negligible vs JSON parsing
- Falls back gracefully to raw ANSI rendering for lines that don't match

## Requirements

### Must Have (Stage 1 — Core + TUI) — COMPLETE

- [x] **R1: StyledSegment IR** — `StyledSegment` with `text` + `SegmentStyle` enum (Default, Dim, Bold, Italic, Fg(SegmentColor)). `to_ratatui_style()` conversion. `resolve_severity_style()` and `resolve_status_code_style()` mapping functions.
- [x] **R2: YAML preset definition** — `RawRendererDef` in config with name, detect, regex, layout. Deserialized via serde under `renderers:` key.
- [x] **R3: Compile-at-load** — `compile()` converts `RawPreset` → `CompiledPreset` with pre-compiled regex, resolved layout entries, tracked `consumed_fields`.
- [x] **R4: Field extraction** — `FieldSource` enum with JSON (`serde_json`), logfmt (reuses `parse_logfmt()`), regex (named captures). Unified `extract_fields()` entry point.
- [x] **R4a: Auto parser** — `PresetParser::Auto` checks index flags per-line (JSON first, logfmt second). Without flags, tries JSON→logfmt sequentially. Auto-selected when detect.parser omitted and no regex.
- [x] **R5: `_rest` pseudo-field** — `get_rest_fields()` returns unconsumed fields sorted by key. Supports `RestFormat::KeyValue` ("k1=v1 k2=v2") and `RestFormat::Json` (JSON object).
- [x] **R6: Per-line renderer chain** — `PresetRegistry::render_line()` tries each renderer in order, returns first `Some`. Raw ANSI fallback is implicit in TUI.
- [x] **R6a: Index-based early reject** — `index_filter()` returns `(mask, want)` tuple. JSON preset requires `FLAG_FORMAT_JSON`, logfmt requires `FLAG_FORMAT_LOGFMT`. Regex/Auto return `None` (no shortcut). Checked before parsing.
- [x] **R7: Style functions** — `severity` maps level strings to colors (error→red, warn→yellow, info→green, debug→cyan, trace→gray). `status_code` maps HTTP codes (2xx→green, 3xx→cyan, 4xx→yellow, 5xx→red). Static: dim, bold, italic, color names. `StyleFn` enum dispatches.
- [x] **R8: PresetRegistry** — `new()` merges user + builtin (user shadows). `get_by_name()`, `render_line()` (explicit chain), `render_line_auto()` (auto-detection).
- [x] **R9: Auto-detection** — `detect_presets()` matches by filename globs (higher priority) then parser format from index flags.
- [x] **R10: Config — optional path** — `RawSource::path` is `Option<PathBuf>`. Metadata-only sources (name + renderers, no path) parsed correctly.
- [x] **R11: Config — renderers list** — `RawSource::renderers` is `Vec<String>` with `#[serde(default)]`. Passed through to `Source::renderer_names`.
- [x] **R12: Name matching** — Config metadata-only sources bind renderer_names to discovered sources by name. `App::source_renderer_map` built from config at startup, used in both initial discovery and runtime dir watcher.
- [x] **R12a: Combined view renderer resolution** — `CombinedReader::renderer_names()` returns per-source list. `render_log_view()` resolves renderer chain per-line via `source_id`. `ensure_combined_tabs()` wires renderer_names.
- [x] **R13: TUI integration** — `render_log_view()` tries preset before ANSI parsing. Converts `StyledSegment` → ratatui `Span` via `to_ratatui_style()`. Severity background and selection styling remain independent.
- [x] **R14: Built-in defaults** — `builtin_json()` (timestamp|level|message|_rest) and `builtin_logfmt()` (ts|level|msg|_rest). Both activate via auto-detection from index flags.
- [x] **R15: Width/alignment** — `apply_width()` truncates if over width, pads left-aligned if under.

### Should Have (Stage 2 — Field Paths + Styling) — COMPLETE

- [x] **R16: Array index in field paths** — `extract_json_field()` resolves numeric path segments as array indices. `message.content.0.text` traverses into `message` → `content` (array) → index 0 → `text`. When a path segment is a valid `usize` and the current value is an array, uses index access; otherwise falls back to object key lookup.
- [x] **R17: Value-to-style mapping (`style_map`)** — Layout entries have an optional `style_map` field: a YAML map from field value → style name. When the extracted field value matches a key, that style is applied; unmatched values get `Default`. Supports `_default` fallback key. `style_map` and `style` are mutually exclusive (compile error if both set). Resolved at compile time into a `StyleFn::Map(HashMap<String, SegmentStyle>)` variant.
- [x] **R18: `max_width` (truncate without padding)** — Layout entries have an optional `max_width` field. Unlike `width` (which pads short values with spaces to a fixed column), `max_width` only truncates values exceeding the limit. Short values are unchanged. `width` and `max_width` are mutually exclusive (compile error if both set).
- [x] **R19: Compound styles** — `style` accepts a list of style names. `style: [bold, cyan]` applies both BOLD modifier and cyan foreground. Resolved at compile time into `SegmentStyle::Compound { dim, bold, italic, fg }`. Modifiers (dim, bold, italic) combine with one Fg color. Compile error if two Fg colors specified.

### Should Have (Stage 3 — MCP + Formatting)

- [ ] **R20: MCP integration** — `LineInfo` gains optional `rendered` field with preset-formatted plain text. `content` stays as raw line (backward compatible). MCP `list_sources` exposes available renderer names per source.
- [ ] **R24: Field formatting** — Built-in formatters: `datetime` (relative/absolute), `duration` (humanize ms/ns), `bytes` (humanize). Applied via `format:` key on layout entries.
- [ ] **R25: Conditional styling** — Style based on field value comparison: e.g., highlight duration > 1000ms. Uses `style_when` with `{field, op, value, style}` conditions.

### Should Have (Stage 4 — Capture + Theme + External Presets)

- [ ] **R21: Capture passthrough rendering** — During `cmd | lazytail -n name`, apply the source's rendering preset to format log lines written to stdout. Resolves preset chain from config (by source name) or auto-detection. Lines that don't match any preset pass through unchanged.
- [ ] **R22: `--raw` flag for capture** — `lazytail -n name --raw` bypasses preset formatting and outputs unmodified log lines during capture. Default behavior (without `--raw`) applies preset formatting.
- [ ] **R23: Theme-aware preset styles** — Preset styles resolve colors from `theme.palette` instead of fixed ANSI color names. Changing the active theme also changes how rendering presets colorize log lines. `SegmentColor` gains a `Palette(String)` variant that looks up named colors from the theme. Fixed ANSI names (`red`, `cyan`, etc.) remain as fallback.
- [ ] **R26: External preset files** — Load preset definitions from standalone YAML files in search paths, following the same pattern as theme loading (`collect_themes_dirs`). Search order: `{project_root}/.lazytail/renderers/` > `{project_root}/renderers/` > `~/.config/lazytail/renderers/`. Each `.yaml` file defines one preset (filename stem = preset name). External presets merge with inline `renderers:` in `lazytail.yaml` (inline shadows external). `PresetRegistry::new()` discovers and compiles external presets alongside inline ones. Repo-bundled presets in `{project_root}/renderers/` let projects ship presets for their log formats without requiring each user to copy them into their config.

### Could Have (future)

- [ ] **R27: Preset inheritance** — A preset can extend another via `extends: base-preset`, overriding specific layout entries while inheriting the rest.
- [ ] **R28: Live preset reload** — Watch preset config for changes, recompile without restart.
- [ ] **R29: Preset sharing** — Import presets via URL or package.
- [ ] **R30: Conditional layout entries (`when`)** — Layout entries gain an optional `when` field that references a field + expected value. Entry is only rendered when condition matches. Example: `when: {field: type, eq: system}` would only render `subtype` for system events. Avoids blank gaps in mixed-type lines.
- [ ] **R31: Array join/iteration** — A `format: join(", ")` option for array fields that iterates array elements, extracts a sub-field from each, and joins them. Example: `field: message.content, format: join("; "), sub_field: type` → `"text; tool_use; thinking"`. Useful for summarizing content block types.
- [ ] **R32: Query `format` stage** — Inline preset-like formatting in query syntax: `json | format <severity> - <method> <url> - <status>`. Parses a format template after `format` keyword, extracts referenced fields, renders each line using the template. Bridges query language and rendering presets — lightweight alternative to defining a full YAML preset.

### Won't Have (out of scope)

- WASM/web integration (separate feature)
- Custom scripting/plugins for presets (Lua, WASM plugins)

## YAML Config Format

```yaml
# lazytail.yaml
renderers:
  - name: json-structured
    detect:
      parser: json                    # auto-detect from index flags
    layout:
      - field: timestamp
        style: dim
      - literal: " "
      - field: level
        style: severity              # maps value to color
        width: 5                     # fixed width, left-aligned
      - literal: " | "
        style: dim
      - field: message
      - literal: " "
      - field: _rest                 # remaining fields
        style: dim
        format: key=value            # or: json

  - name: nginx-access
    detect:
      filename: "access*.log"        # glob pattern
    regex: '(?P<ip>\S+) \S+ \S+ \[(?P<time>[^\]]+)\] "(?P<method>\S+) (?P<path>\S+) \S+" (?P<status>\d+)'
    layout:
      - field: time
        style: dim
      - literal: " "
      - field: method
        style: bold
      - literal: " "
      - field: path
      - literal: " "
      - field: status
        style: status_code           # maps HTTP codes to colors

  - name: claude-conversation
    detect:
      parser: json
    layout:
      - field: agentName
        style: cyan
        width: 18
      - literal: " "
      - field: type
        width: 9
        style_map:                       # R17: value → color
          user: green
          assistant: blue
          system: yellow
          progress: dim
      - literal: " | "
        style: dim
      - field: message.content.0.type    # R16: array index
        style: dim
        max_width: 12                    # R18: truncate, no pad
      - literal: " "
      - field: message.content.0.text    # R16: nested array path
        max_width: 120
      - field: message.content.0.name    # tool name for tool_use blocks
        style: magenta
      - field: subtype                   # system events
        style: yellow
      - field: data.type                 # progress events
        style: dim

sources:
  - name: backend
    renderers:
      - json-structured        # try JSON first
      - logfmt-basic           # try logfmt if not JSON
      # raw ANSI is always the implicit last resort
    # no path — metadata-only, matches discovered source by name

  - name: api
    path: /var/log/api.log
    renderers:
      - nginx-access

  - name: claude_log
    renderers:
      - claude-conversation    # conversation format for Claude logs

  - name: mixed-service
    renderers:
      - json-structured
      - nginx-access           # some lines are access logs
      - logfmt-basic           # some are logfmt
```

## Architecture

```
                         ┌─ renderer 1 → None (no match)
Raw Line → renderers[] ──┤─ renderer 2 → Some(Vec<StyledSegment>) → adapter
                         └─ ...
                         └─ (implicit) raw ANSI fallback

Combined ($all) view:
  line → MergedLine.source_id → source name → that source's renderers[]

Adapters:
  TUI: StyledSegment → ratatui Span (via to_ratatui_style)
  MCP: StyledSegment → plain text (segments concatenated)
  Web: (out of scope — separate WASM feature)
```

### Module Structure

```
src/renderer/
  mod.rs        — PresetRegistry, public API
  segment.rs    — StyledSegment IR (adapter-agnostic)
  preset.rs     — RawPreset (YAML), CompiledPreset, compilation, render()
  field.rs      — Unified field extraction (wraps query.rs + regex)
  detect.rs     — Auto-detection (index flags, filename globs)
  builtin.rs    — Built-in default presets (json, logfmt)
```

### Key Reuse Points

- `src/filter/query.rs` — `extract_json_field()`, `parse_logfmt()`, `Parser` enum
- `src/index/flags.rs` — `FLAG_FORMAT_JSON`, `FLAG_FORMAT_LOGFMT` for auto-detection
- `src/index/reader.rs` — `IndexReader::severity()` for line-number coloring (unchanged)

## Files to Create/Modify

| File | Action | Description |
|------|--------|-------------|
| `src/renderer/mod.rs` | Create | PresetRegistry, public API |
| `src/renderer/segment.rs` | Create | StyledSegment IR |
| `src/renderer/preset.rs` | Create | RawPreset, CompiledPreset, compile + render |
| `src/renderer/field.rs` | Create | Unified field extraction |
| `src/renderer/detect.rs` | Create | Auto-detection logic |
| `src/renderer/builtin.rs` | Create | Built-in default presets |
| `src/config/types.rs` | Modify | Optional path, `renderers` list on source, `renderers` preset defs on root |
| `src/config/loader.rs` | Modify | Handle optional path, pass through renderers |
| `src/config/error.rs` | Modify | Add new known fields for typo detection |
| `src/filter/query.rs` | Modify | Promote `extract_json_field` + `parse_logfmt` to `pub` |
| `src/log_source.rs` | Modify | Add `renderer_names: Vec<String>` field |
| `src/app/mod.rs` | Modify | Add `preset_registry` to App |
| `src/app/tab.rs` | Modify | Wire renderer names through tab creation |
| `src/reader/combined_reader.rs` | Modify | Add `renderer_names` to `SourceEntry` for per-line resolution |
| `src/tui/log_view.rs` | Modify | Integrate renderer chain before ANSI path |
| `src/main.rs` | Modify | Add `mod renderer`, load presets, name-match metadata sources |

## Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Preset format | YAML config | User-editable, no recompile needed. Compiled at load time for performance — per-line cost same as trait impls. |
| IR design | Flat `Vec<StyledSegment>` | Cache-friendly, trivially serializable, no adapter dependencies |
| Renderer resolution | Ordered list per source | Source defines `renderers: [a, b, c]`. Try in order, first `Some` wins. Raw ANSI is implicit last resort. Combined views resolve per-line via `source_id`. |
| Cache | None initially | JSON parsing ~50 visible lines/frame is microseconds. Profile later. |
| New dependencies | None | Reuse `serde_json`, `regex`, `serde_saphyr`. Simple wildcard matching instead of `glob` crate. |
| Early reject | Index flags `(mask, want)` | Check `FLAG_FORMAT_JSON`/`FLAG_FORMAT_LOGFMT` before parsing. Same pattern as `FilterQuery::index_mask()`. Two bitmask ops per preset skip — negligible. |
| ANSI in field values | Strip | Preset provides its own styling. Raw ANSI only for fallback lines. |
| Preset mutability | Immutable after load | `Arc<PresetRegistry>` shared across threads safely. |
| Array index syntax | Dot notation (`content.0.text`) | Consistent with existing dot-path syntax. No parser ambiguity. serde_json `Value::get(usize)` handles index access natively. |
| `style_map` vs `style` | Mutually exclusive | Compile error if both set. Keeps `StyleFn` dispatch simple — either static/dynamic or map lookup, never both. |
| `max_width` vs `width` | Mutually exclusive | `width` = fixed column (pad + truncate), `max_width` = truncate-only. No overlap in semantics. |
| Discovered source renderers | `App::source_renderer_map` | Built once from config at startup. Looked up by source name for both initial discovery and runtime dir watcher. Stored on App for event loop access. |

## Implementation Progress

### Stage 1: Core + Config + TUI — COMPLETE

- [x] StyledSegment IR (`src/renderer/segment.rs`) — `StyledSegment`, `SegmentStyle`, `SegmentColor`, `to_ratatui_style()`
- [x] Field extraction wrapper (`src/renderer/field.rs`) — JSON, logfmt, regex, auto-detect via `FieldSource` enum
- [x] CompiledPreset + render (`src/renderer/preset.rs`) — compile-at-load, `_rest` pseudo-field, width/alignment
- [x] Auto-detection (`src/renderer/detect.rs`) — filename globs (priority) + parser format from index flags
- [x] PresetRegistry (`src/renderer/mod.rs`) — `render_line()` chain + `render_line_auto()`, user presets shadow builtins
- [x] Built-in presets (`src/renderer/builtin.rs`) — `json` and `logfmt` with standard field layouts
- [x] Config: optional path + `renderers` list on source (`src/config/`)
- [x] Config: `renderers` preset definitions on root config
- [x] Config: error system updates for new fields
- [x] Promote `extract_json_field` / `parse_logfmt` to `pub`
- [x] `renderer_names: Vec<String>` on LogSource
- [x] `preset_registry: Arc<PresetRegistry>` on App
- [x] Tab creation wiring (renderer name from config/discovery)
- [x] `PresetParser::Auto` variant — per-line format detection via index flags, falls back to JSON→logfmt
- [x] `CompiledPreset::index_filter()` → `(mask, want)` for early reject without parsing
- [x] TUI: renderer chain in `render_log_view()` (early reject + try each, first match wins)
- [x] TUI: `to_ratatui_style()` conversion
- [x] TUI: combined view per-line renderer resolution via `source_id` → `renderer_names()`
- [x] Name matching: `App::source_renderer_map` built from config, passed to `from_discovered_source()` at startup and via dir watcher
- [x] Tests: preset compilation, rendering, field extraction, renderer chain, `_rest`, config loading

### Stage 2: Field Paths + Styling — COMPLETE

- [x] R16: Array index in field paths (`extract_json_field` numeric segment → array index, with object key fallback)
- [x] R17: `style_map` — value-to-style mapping on layout entries (`StyleFn::Map`, `_default` fallback key, mutual exclusivity with `style`)
- [x] R18: `max_width` — truncate without padding (mutual exclusivity with `width`)
- [x] R19: Compound styles (`style: [bold, cyan]` → `SegmentStyle::Compound`, single Fg color enforced)

### Stage 3: MCP + Formatting (next)

- [ ] R20: MCP `rendered` field in LineInfo + renderer names in `list_sources`
- [ ] R20: Preset registry in MCP server
- [ ] R24: Field formatters (datetime, duration, bytes)
- [ ] R25: Conditional styling (`style_when` conditions)

### Stage 4: Capture + Theme + External Presets

- [ ] R21: Capture passthrough rendering (`lazytail -n` applies preset to stdout)
- [ ] R22: `--raw` flag to bypass passthrough formatting
- [ ] R23: Theme-aware styles (`SegmentColor::Palette`, resolve from `theme.palette`)
- [ ] R26: External preset files (`.lazytail/renderers/`, `renderers/`, `~/.config/lazytail/renderers/`)

## Testing Strategy

1. **Unit tests** — Preset compilation, `CompiledPreset::render()`, field extraction, `_rest` handling, style resolution, auto-detection
2. **Config tests** — Optional path parsing, renderer field, renderers list, typo detection for new fields
3. **Integration test** — Create temp config with preset + source, verify end-to-end rendering
4. **Manual verification:**
   ```bash
   # 1. Create lazytail.yaml with renderer + source binding
   # 2. Pipe JSON logs:
   echo '{"level":"error","message":"fail","service":"api"}' | lazytail -n test
   # 3. View in TUI:
   lazytail
   # 4. Verify structured rendering, fallback for non-JSON lines
   ```

## Open Questions

1. ~~Should auto-detection built-in presets be opt-out?~~ Yes, they only activate when no explicit renderer is set.
2. ~~How should we handle field name aliases? (e.g., `msg` vs `message`, `ts` vs `timestamp`)~~ Built-in `json` preset consumes both `message` and `msg`; `logfmt` preset consumes `ts` and `msg`. Users define custom presets for other aliases.
3. Should presets be definable in a separate `~/.config/lazytail/presets.yaml` file? — Defer, start with inline `renderers:` in `lazytail.yaml`.
4. ~~Name matching for discovered sources~~ — Resolved: `App::source_renderer_map` built from config, used in `from_discovered_source()`.
5. R16 array index: should `message.content.0.text` syntax also support bracket notation (`message.content[0].text`)? Dot notation is simpler to parse and consistent with existing path syntax. Bracket notation is more familiar from jq/JSONPath. — **Decision: dot notation only** (`message.content.0.text`). Simpler implementation, no ambiguity with field names containing dots.
6. R17 style_map: should unmatched values fall back to a `_default` key or always to `SegmentStyle::Default`? — Allow optional `_default` key in the map for a fallback style.
7. R19 compound styles: should the style list be ordered (first color wins) or should color conflict be a compile error? — First Fg color wins, multiple modifiers combine. Compile error if two Fg colors specified.
8. R21 capture passthrough: should preset rendering be the default for `lazytail -n`, with `--raw` to disable? Or opt-in with `--format`/`--render`? Default-on is more useful but changes existing behavior.
9. R23 theme-aware: should `SegmentColor::Palette(name)` fall back to a hardcoded color if the palette key is missing, or fail at compile time? Runtime fallback is more resilient, compile-time is safer.
10. R32 query `format` stage: should the format template reuse YAML preset layout syntax or have its own inline syntax? Inline is more ergonomic for one-off use but adds a second format language.
11. R26 external preset files: should the directory be called `renderers/` or `presets/`? `renderers/` matches the YAML key name (`renderers:`). `presets/` is more intuitive for users. Theme system uses `themes/` matching the config key `theme:`.
