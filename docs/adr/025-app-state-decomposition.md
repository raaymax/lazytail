# ADR-025: App State Decomposition into Sub-Controllers

## Status

Accepted

## Context

The `App` struct in `src/app/mod.rs` accumulated many flat fields over time: `tabs`, `active_tab`, `combined_tabs`, `input_mode`, `input_buffer`, `cursor_position`, `filter_history`, `pending_filter_at`, `regex_error`, source panel selection, panel width, category expansion state, and more. All of these lived at the same level inside `App`, making `apply_event()` increasingly complex and making it difficult to reason about which fields belonged to which concern.

For example, filter debouncing (`pending_filter_at`), regex validation (`regex_error`), query validation (`query_error`), filter mode (`current_mode`), and filter history were all separate fields on `App` despite being a single cohesive concern. Similarly, input buffer, cursor position, and input mode were independent fields that always changed together. This violated the Single Responsibility Principle and made it easy to forget updating a related field when modifying another.

## Decision

Decompose `App` into four sub-controllers, each owning a coherent slice of state:

### 1. `TabManager` (`app/tab_manager.rs`)

Owns the tab collection and combined view lifecycle:

- `tabs: Vec<TabState>` -- all open tabs
- `active: usize` -- currently active tab index
- `combined: [Option<TabState>; 5]` -- per-category combined (`$all`) tabs, indexed by `SourceType`
- `active_combined: Option<SourceType>` -- which combined tab is active (None = regular tab)

Provides: `active_tab()` / `active_tab_mut()` (resolves combined vs regular), `select_tab()`, `close_tab()`, `tabs_by_category()`, `find_tab_index()`, `ensure_combined_tabs()`, `refresh_combined_tab()`, `select_combined_tab()`.

### 2. `InputController` (`app/input_controller.rs`)

Owns text input mechanics and the `InputMode` enum:

- `buffer: String` -- input buffer for filter/line-jump entry
- `cursor: usize` -- cursor position (byte offset)
- `mode: InputMode` -- current input mode

`InputMode` moved here from `app/mod.rs`. Variants: `Normal`, `EnteringFilter`, `EnteringLineJump`, `ZPending`, `SourcePanel`, `ConfirmClose`.

Provides: `input_char()`, `input_backspace()`, `cursor_left()` / `cursor_right()` / `cursor_home()` / `cursor_end()`, `set_content()`, `clear()`, and mode query helpers (`is_entering_filter()`, `is_entering_line_jump()`).

### 3. `FilterController` (`app/filter_controller.rs`)

Owns filter validation, debouncing, and history navigation:

- `current_mode: FilterMode` -- plain, regex, or query (with case sensitivity)
- `regex_error: Option<String>` -- regex validation error
- `query_error: Option<String>` -- query syntax validation error
- `pending_at: Option<Instant>` -- debounce deadline for live filter preview
- `history: Vec<FilterHistoryEntry>` -- filter history (max 50 entries)
- `history_index: Option<usize>` -- position during history navigation

Provides: `validate_regex()`, `validate_query()`, `is_valid()`, `schedule_debounce()`, `add_to_history()`, `history_up()` / `history_down()`, `reset_history_index()`. The debounce delay is a module-level constant (`FILTER_DEBOUNCE_MS = 500`).

### 4. `SourcePanelController` (`app/source_panel.rs`)

Owns source tree navigation state:

- `state: SourcePanelState` -- selection (`Option<TreeSelection>`) and per-category expansion flags (`[bool; 5]`)
- `width: u16` -- side panel width (default 32)

Provides: `navigate()` (delta-based movement over a flat item list), `toggle_category_expand()`, `fix_selection_after_close()`.

### What remains on `App`

`App` retains fields that are either cross-cutting or don't belong to any single controller:

- `preset_registry: Arc<PresetRegistry>` -- rendering presets
- `theme: Theme` -- color scheme
- `source_renderer_map: HashMap<String, Vec<String>>` -- config-driven renderer assignments
- `should_quit`, `help_scroll_offset`, `pending_close_tab`, `confirm_return_mode`
- `status_message`, `has_start_filter_in_batch`, `startup_time`, `first_render_elapsed`
- `verbose`, `layout: LayoutAreas`, `warning_popup`

`App::apply_event()` remains the single entry point for state transitions. It delegates to controllers but mediates any cross-controller coordination (e.g., filter submission reads from `input.buffer`, validates via `filter`, then triggers orchestration on `tab_mgr.active_tab_mut()`).

### Key design points

- **Plain structs, not traits.** Each controller is a concrete struct with methods. There is no `Controller` trait -- the controllers are not interchangeable and adding a trait would be unnecessary abstraction.
- **Controllers don't reference each other.** `App` mediates all cross-controller interactions. For example, `FilterController::history_up()` returns `Option<(String, FilterMode)>`, and `App::apply_event()` feeds that into `InputController::set_content()`. The controllers have no knowledge of each other.
- **`apply_event()` stays as the single entry point.** The decomposition does not distribute event handling across controllers. All side-effects (debounce scheduling, cancellation, follow-mode jumps, tab switching) still flow through `App::apply_event()`.
- **Backward-compatible delegation.** `App` provides thin delegation methods like `active_tab()` and `select_tab()` that forward to `TabManager`, avoiding a large blast radius across callers.

## Consequences

**Benefits:**

- Each controller is independently understandable -- `FilterController` is ~180 lines with a clear scope, compared to the filter-related fields being scattered across a much larger `App`
- Related fields are grouped and encapsulated -- it is no longer possible to update `pending_at` without having `FilterController` in scope
- Adding new state to a concern (e.g., a new filter validation check) requires changes only in the relevant controller
- The `App` struct definition is readable at a glance: four named controllers plus a handful of cross-cutting fields

**Trade-offs:**

- Access to controller state requires an extra level of indirection (`app.input.buffer` instead of `app.input_buffer`), which affects all handler code
- `App::apply_event()` still contains the coordination logic and remains a large function -- the decomposition reduces field sprawl but does not reduce event-handling complexity
- Cross-controller operations (e.g., filter submission touching input, filter, and tab state) require `apply_event()` to read from one controller and write to another, which means the method must hold mutable access to `App` as a whole
