# ADR-004: Channel-Based Filter Communication

## Status

Accepted

## Context

Background filter threads need to communicate results back to the UI thread without blocking rendering. The filter may take seconds on large files, and users expect to see results appear progressively.

Options considered:
1. **Shared mutable state** - filter writes to shared Vec behind a Mutex
2. **Channels** (`std::sync::mpsc`) - filter sends progress messages
3. **Async streams** - tokio/async channels with await

## Decision

We use **`std::sync::mpsc` channels** for filter-to-UI communication:

```
FilterEngine/StreamingFilter
  -> spawns std::thread
  -> sends FilterProgress variants via Sender<FilterProgress>

Main loop
  -> calls try_recv() on Receiver (non-blocking)
  -> processes PartialResults, Complete, Error
```

`FilterProgress` variants:
- `Processing(usize)` - lines processed so far
- `PartialResults { matches, lines_processed }` - batch of matching line indices
- `Complete { matches, lines_processed }` - final results
- `Error(String)` - filter error

The `CancelToken` (an `Arc<AtomicBool>`) allows the main thread to signal cancellation to the filter thread when the user changes the pattern or cancels the filter.

## Consequences

**Benefits:**
- Non-blocking: `try_recv()` never blocks the render loop
- Progressive results: `PartialResults` let the UI show matches as they're found
- Clean cancellation: `CancelToken` checked periodically (every 10,000 lines)
- No tokio dependency for the core TUI (async only needed for MCP server)

**Trade-offs:**
- One channel per active filter per tab (but only one filter runs per tab at a time)
- Need to manually drain the channel and handle each variant
- `PartialResults` batching (50,000 lines) is a fixed trade-off between responsiveness and overhead
