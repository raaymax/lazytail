# ADR-001: Event-Driven Architecture

## Status

Accepted

## Context

A TUI log viewer needs to handle multiple concurrent concerns: user input, file changes, background filter progress, and stream data from pipes. These must be coordinated without blocking the render loop or causing race conditions.

Common approaches include:
1. **Direct mutation** - handlers directly modify application state
2. **Event-driven** - handlers return events, a central loop processes them
3. **Actor model** - separate threads for each concern with message passing

## Decision

We use an **event-driven render-collect-process cycle**:

1. **Render** current state with ratatui
2. **Collect** events from all sources (file watchers, filter channels, input)
3. **Process** events via `App::apply_event()` to update state
4. Repeat until quit

Input handlers (`handlers/input.rs`, `handlers/filter.rs`, `handlers/file_events.rs`) never mutate `App` directly. They return `Vec<AppEvent>`, which are processed sequentially in the main loop.

Side effects that require access to both `App` and external systems (e.g., triggering a filter which needs to spawn a thread) are handled in `process_event()` in `main.rs`, outside of `App::apply_event()`.

## Consequences

**Benefits:**
- All state transitions go through `App::apply_event()`, making the system predictable and testable
- No borrow checker conflicts from handlers trying to mutate state while reading it
- Side effects are explicit and contained in one place (`process_event()`)
- Easy to add new event types without restructuring existing code

**Trade-offs:**
- Some indirection: input handlers can't see the immediate effect of their events
- `process_event()` in main.rs has some complexity from handling filter debouncing, cancellation, and follow mode
- Mouse scroll events are handled as a special case (coalesced before event processing)
