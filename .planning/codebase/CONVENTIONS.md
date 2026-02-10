# Coding Conventions

**Analysis Date:** 2026-02-03

## Naming Patterns

**Files:**
- Lowercase with underscores: `main.rs`, `filter_engine.rs`, `string_filter.rs`
- Module directories mirror file structure: `src/filter/mod.rs` exports submodules
- Trait implementations in separate files: `src/filter/string_filter.rs`, `src/filter/regex_filter.rs`

**Functions:**
- Snake_case for all functions: `run_app()`, `collect_file_events()`, `resolve_with_options()`
- Helper functions prefixed with verb: `handle_`, `process_`, `trigger_`, `collect_`
- Private helper functions explicitly documented with triple-slash comments
- Accessor methods use get_ prefix: `get_line()`, `get_screen_offset()`, `get_input()`
- Boolean checks use is_ or has_ prefix: `is_match()`, `is_loading()`, `is_case_sensitive()`

**Variables:**
- Snake_case for all variables: `active_tab`, `line_indices`, `scroll_position`, `filter_mode`
- Mutable collections suffix with _mut: Not observed; uses `mut` keyword instead
- Configuration constants in UPPER_SNAKE_CASE at module top: `const MAX_HISTORY_ENTRIES: usize = 50;`, `const DEFAULT_EDGE_PADDING: usize = 3;`
- Field names descriptive but concise: `anchor_line`, `scroll_position`, `edge_padding`, `input_buffer`

**Types:**
- PascalCase for all structs: `App`, `TabState`, `FileWatcher`, `StringFilter`, `RegexFilter`
- PascalCase for enums: `FilterMode`, `InputMode`, `ViewMode`, `FileEvent`, `AppEvent`
- Enum variants PascalCase: `FilterMode::Plain`, `InputMode::EnteringFilter`, `FileEvent::Modified`
- Type aliases match struct style: Not extensively used

## Code Style

**Formatting:**
- Standard Rust style via `cargo fmt`
- 4-space indentation (enforced by rustfmt)
- Maximum line length not explicitly enforced but generally ~80-100 chars
- Blank lines separate logical sections within functions
- Comments on own line for clarity

**Linting:**
- `cargo clippy` for lint checking
- Dead code marked with `#[allow(dead_code)]` with explanatory comment:
  ```rust
  #[allow(dead_code)] // Public API for external use and tests
  pub fn plain() -> Self {
  ```
- Unused imports removed, but `use` statements organized in groups
- Compiler warnings treated seriously (few in codebase)

## Import Organization

**Order:**
1. Standard library imports: `use std::path::PathBuf;`
2. External crate imports: `use ratatui::{...}; use regex::Regex;`
3. Internal crate imports: `use crate::filter::{...}; use crate::app::App;`
4. Conditional imports: `#[cfg(test)] use std::path::PathBuf;`

**Path Aliases:**
- Not heavily used; explicit full paths preferred
- Some convenience re-exports in mod.rs: `pub use self::engine::FilterEngine;`
- Glob imports avoided except in tests: `use super::*;`

## Error Handling

**Patterns:**
- Uniform error type: `anyhow::Result<T>` throughout codebase
- Error context added at call site: `.context("Failed to reload file")?`
- Internal errors propagated via `?` operator rather than `.unwrap()`
- Critical invariants use `assert!()` macro: `assert!(!tabs.is_empty(), "App must be created with at least one tab")`
- Graceful degradation: Invalid filters logged but don't crash, reader lock poisoning handled with explicit error message:
  ```rust
  reader_guard.reload()
      .expect("Reader lock poisoned - filter thread panicked")
  ```
- File operation errors include path in message for debugging
- Match on `Ok(x)` for expected success paths, ignore errors with `let _ = tx.send()`

**Error Recovery:**
- Errors in filter threads result in `FilterError` events, not panics
- File watcher errors logged to stderr but app continues: `FileEvent::Error(String)`
- Stream read errors marked as complete rather than crashing: `tab.mark_stream_complete()`

## Logging

**Framework:** Standard `println!()` and `eprintln!()` macros

**Patterns:**
- Debug info typically sent to stdout during normal operation
- Errors sent to stderr: `eprintln!("Failed to reload file for tab {}: {}", tab_idx, e);`
- Non-critical failures logged with context: `eprintln!("Warning: Failed to parse filter history: {}", e);`
- File modification tracking uses comments rather than logs: important state changes documented in code
- Tab index included in multi-tab operations for debugging:
  ```rust
  eprintln!("Filter error for tab {}: {}", tab_idx, err);
  ```

## Comments

**When to Comment:**
- Algorithm rationale: "During filtering, we used to force view to end. But this causes jumping when user navigates..."
- Non-obvious logic: Explain why something is done, not what it does
- SAFETY blocks for unsafe code: Required for all unsafe { } blocks
- Public APIs documented with `///` comments
- Complex state transitions explained: "First pass: reload files and handle inactive tabs"
- Configuration constants documented: "Debounce delay for live filter preview (milliseconds)"

**JSDoc/TSDoc:**
- Uses `///` doc comments for public items:
  ```rust
  /// Create a new viewport anchored to the given line
  pub fn new(initial_line: usize) -> Self {
  ```
- Parameter documentation: `/// The file line number that is selected (stable across filter changes)`
- Module-level documentation with `//!`: See `viewport.rs` and `source.rs`
- Examples in doc comments: Not extensively used
- Private function documentation sparse but present for complex logic

**Special Comments:**
- SAFETY comments required for all unsafe blocks (see `src/source.rs` line 74)
- TODOs minimal - only for actual future work, not for ideas
- FIXME comments indicate known issues but none currently present
- Comments explain "why" not "what": `// Pick closer of insert_pos-1 or insert_pos`

## Function Design

**Size:**
- Functions generally 20-100 lines
- Some handlers can be 200+ lines (main.rs handlers) but split into logical sections with comments
- Event processing split across multiple functions for clarity: `collect_file_events()`, `apply_filter_events_to_tab()`, etc.
- Small focused functions preferred for testability

**Parameters:**
- Ownership: Pass `&mut self` for state mutation, `&self` for reads, `&[T]` for slices
- Generic functions: Used where abstraction valuable (Filter trait), not overused
- Default parameters: Not used (Rust limitation); builder pattern or Options used instead
- Maximum ~5-6 parameters before considering refactoring
- Context passed explicitly rather than global state

**Return Values:**
- `Result<T>` for fallible operations: File I/O, parsing, system calls
- `Option<T>` for value-present/absent: `Option<TabState>`, `Option<FilterProgress>`
- Enums for state variants: `FilterState`, `InputMode`, `FileEvent`
- Tuples for related returns: `(new_total, old_total)` in `ActiveTabFileModification`
- No implicit conversions - match on return values at call site

## Module Design

**Exports:**
- Public traits and structs exported from mod.rs
- Implementation details kept private (e.g., `contains_ascii_ignore_case` in string_filter.rs)
- Submodule re-exports for convenience:
  ```rust
  // filter/mod.rs
  pub use self::engine::FilterEngine;
  pub use self::string_filter::StringFilter;
  ```
- Private module members accessed via `self::` or crate path

**Barrel Files:**
- Minimal barrel files; mod.rs re-exports key public items
- Test modules declared inline with `#[cfg(test)] mod tests { }`
- Feature-gated modules: `#[cfg(feature = "mcp")] mod mcp;`

## Event-Driven Architecture

**Patterns:**
- All state mutations happen in `App::apply_event()` (centralized dispatch)
- Handlers return events instead of mutating state:
  ```rust
  fn process_event(app: &mut App, event: event::AppEvent, has_start_filter: bool) {
      match &event {
          // ...
          _ => app.apply_event(event)
      }
  }
  ```
- Background operations emit events via channels (filter threads, file watchers)
- Non-blocking event collection with `try_recv()` calls

## Unsafe Code

**Guidelines:**
- Unsafe used only for POSIX kill() check (process existence on non-Linux)
- All unsafe blocks wrapped with explicit SAFETY comment
- Alternative safe implementations preferred when available
- Unsafe blocks kept minimal and well-justified

---

*Convention analysis: 2026-02-03*
