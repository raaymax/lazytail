# Testing Patterns

**Analysis Date:** 2026-02-03

## Test Framework

**Runner:**
- Built-in `cargo test` via standard Rust test framework
- No external test framework dependency
- Config: Default Rust test harness

**Assertion Library:**
- Standard Rust macros: `assert!()`, `assert_eq!()`, `assert_ne!()`
- Custom matching: `matches!()` macro for enum pattern matching

**Run Commands:**
```bash
cargo test                       # Run fast tests only
cargo test -- --include-ignored  # Run all tests including slow ones
cargo test -- --ignored          # Run only slow/integration tests
cargo test -- --nocapture        # Show println! output from tests
```

## Test File Organization

**Location:**
- Co-located with implementation: Tests in same file as code
- Test module declared at end of each file with `#[cfg(test)]`

**Naming:**
- Test function names describe behavior: `test_case_insensitive_matching()`, `test_detects_file_modification()`
- Modules always named `tests` (convention)
- No separate test directory; all tests in `src/`

**Structure:**
```
src/
├── filter/
│   ├── string_filter.rs        # Implementation + tests
│   ├── regex_filter.rs         # Implementation + tests
│   └── engine.rs               # Implementation + tests
├── watcher.rs                  # Implementation + tests
├── viewport.rs                 # Implementation + tests
└── source.rs                   # Implementation + tests
```

## Test Structure

**Suite Organization:**
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_name() {
        // Arrange
        let input = setup();

        // Act
        let result = function_under_test(input);

        // Assert
        assert_eq!(result, expected);
    }
}
```

**Patterns:**
- Arrange-Act-Assert structure implicit but widely followed
- Setup in test function body or via helper functions
- Teardown automatic (Rust Drop trait or function scope)
- One logical assertion per test (though may use multiple assert!() calls for related conditions)

**Examples from codebase:**

Filter tests (src/filter/string_filter.rs):
```rust
#[test]
fn test_case_insensitive_matching() {
    let filter = StringFilter::new("error", false);

    assert!(filter.matches("ERROR: Something went wrong"));
    assert!(filter.matches("error: Something went wrong"));
    assert!(!filter.matches("INFO: Everything is fine"));
}

#[test]
fn test_empty_pattern() {
    let filter = StringFilter::new("", false);

    assert!(filter.matches("Any line"));
    assert!(filter.matches(""));
}
```

Viewport tests (src/viewport.rs):
```rust
fn make_lines(lines: &[usize]) -> Vec<usize> {
    lines.to_vec()
}

#[test]
fn test_resolve_basic() {
    let mut vp = Viewport::new(5);
    let lines = make_lines(&[0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);

    let view = vp.resolve(&lines, 5);
    // assertions follow
}
```

## Mocking

**Framework:**
- Manual mocking via test-specific implementations
- No external mock library (tempfile used for file system tests)
- Trait-based design enables easy testing with stub implementations

**Patterns:**
```rust
// Example: StringFilter tests use direct instantiation
let filter = StringFilter::new("pattern", false);
assert!(filter.matches("sample line"));

// Example: FileWatcher uses real tempfile for integration tests
let temp_file = NamedTempFile::new().unwrap();
let watcher = FileWatcher::new(temp_file.path()).unwrap();
```

**What to Mock:**
- File system: Use `tempfile::NamedTempFile` for integration tests
- External crate behavior: Create wrapper types if needed (not observed in current codebase)

**What NOT to Mock:**
- Filter trait implementations: Test real StringFilter and RegexFilter
- Core data structures: Test real Viewport, TabState, App
- Logic algorithms: Always test actual implementation

## Fixtures and Factories

**Test Data:**
- Inline in test functions: `let filter = StringFilter::new("error", false);`
- Helper functions for repeated setup:
  ```rust
  fn make_lines(lines: &[usize]) -> Vec<usize> {
      lines.to_vec()
  }
  ```
- Poll helper for async/timing tests:
  ```rust
  fn poll_for_event(watcher: &FileWatcher, max_attempts: u32, interval_ms: u64) -> Option<FileEvent> {
      for _ in 0..max_attempts {
          if let Some(event) = watcher.try_recv() {
              return Some(event);
          }
          thread::sleep(Duration::from_millis(interval_ms));
      }
      None
  }
  ```

**Location:**
- Helper functions in same `#[cfg(test)] mod tests { }` block
- Or defined as private functions in main module if needed by multiple test modules

## Coverage

**Requirements:**
- Not enforced by CI/build system
- Observed coverage patterns:
  - Filter implementations: ~95%+ (comprehensive test cases)
  - Core structures (Viewport, App): ~80%+ (major paths tested)
  - Event handlers: ~70%+ (some paths tested, edge cases may lack coverage)
  - File I/O modules: Integration tests focus on happy path

**View Coverage:**
```bash
cargo tarpaulin --out Html  # Generate HTML coverage report (if tarpaulin installed)
```

## Test Types

**Unit Tests:**
- Scope: Single function or small component
- Approach: Test behavior in isolation
- Examples:
  - `test_case_insensitive_matching()` in string_filter.rs
  - `test_pattern_anchors()` in regex_filter.rs
  - `test_resolve_basic()` in viewport.rs
- Characteristics: Fast (< 1ms each), many per module, no file system access

**Integration Tests:**
- Scope: Component interaction with file system or multiple modules
- Approach: Test real file modifications, watch events, filter progress
- Examples:
  - `test_detects_file_modification()` in watcher.rs
  - `test_multiple_modifications()` in watcher.rs
- Characteristics: Slower (50-200ms), marked with `#[ignore]`, use tempfile crate
- Run with: `cargo test -- --ignored`

**Acceptance Tests:**
- Not present: LazyTail is a UI application without end-to-end test infrastructure
- UI testing would require terminal emulator mocking (too complex for current setup)

## Slow Tests

**Marking:**
```rust
#[test]
#[ignore]  // Slow test - requires temp dir setup
fn test_rapid_modifications_stress() {
    // Test code
}
```

**Rationale:**
- File system operations add delay (inotify/notify latency ~10-50ms)
- Polling introduces intentional sleeps for event detection
- Tests marked `#[ignore]` run in separate CI pipeline if desired

**Run Separately:**
```bash
cargo test                      # Fast tests only (skips #[ignore])
cargo test -- --ignored         # Just slow tests
cargo test -- --include-ignored # All tests (fast + slow)
```

## Common Patterns

**Async Testing (Timing-Based):**
```rust
// Example from watcher.rs - poll-based event detection
fn poll_for_event(watcher: &FileWatcher, max_attempts: u32, interval_ms: u64) -> Option<FileEvent> {
    for _ in 0..max_attempts {
        if let Some(event) = watcher.try_recv() {
            return Some(event);
        }
        thread::sleep(Duration::from_millis(interval_ms));
    }
    None
}

#[test]
fn test_detects_file_modification() {
    let temp_file = NamedTempFile::new().unwrap();
    let path = temp_file.path().to_path_buf();
    let watcher = FileWatcher::new(&path).unwrap();

    thread::sleep(Duration::from_millis(50)); // Allow watcher init

    // Modify file
    let mut file = OpenOptions::new().append(true).open(&path).unwrap();
    writeln!(file, "New line").unwrap();

    // Poll for event (10 attempts x 10ms = 100ms max)
    let event = poll_for_event(&watcher, 10, 10);
    assert!(matches!(event, Some(FileEvent::Modified)));
}
```

**Error Testing:**
```rust
// Example from regex_filter.rs
#[test]
fn test_invalid_regex() {
    let result = RegexFilter::new(r"[invalid(", false);
    assert!(result.is_err());
}

// Example from watcher.rs
#[test]
fn test_watcher_creation_fails_for_nonexistent_file() {
    let result = FileWatcher::new("/path/that/definitely/does/not/exist/file.log");
    assert!(result.is_err());
}
```

**Boundary Testing:**
```rust
// From string_filter.rs - empty pattern should match everything
#[test]
fn test_empty_pattern() {
    let filter = StringFilter::new("", false);
    assert!(filter.matches("Any line"));
    assert!(filter.matches(""));
}

// From viewport.rs - saturating arithmetic prevents panics
#[test]
fn test_resolve_basic() {
    let mut vp = Viewport::new(5);
    let lines = make_lines(&[0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
    let view = vp.resolve(&lines, 5);
    assert_eq!(view.selected_index, 5);
}
```

## Test Dependencies

**External Crates (dev only):**
- `tempfile = "3.0"` - Temporary file creation for integration tests

**Standard Library:**
- `std::fs` - File operations in tests
- `std::thread` - Timing/polling in async tests
- `std::time::Duration` - Sleep intervals

## Test Organization Rules

**Assertions per test:**
- Typically 2-5 assertions per test function
- Related conditions tested together (e.g., multiple case sensitivity variations)
- When many assertions needed, split into multiple tests

**Test names:**
- Start with `test_` prefix (required by Rust test framework)
- Describe input or condition: `test_case_insensitive_matching()`
- Describe expected behavior: `test_empty_pattern()`
- Avoid generic names like `test_basic()` or `test_works()`

**Test grouping:**
- Fast tests at top of module
- Slow tests marked `#[ignore]` at bottom
- Related tests adjacent (all filter tests together, all navigation tests together)

---

*Testing analysis: 2026-02-03*
