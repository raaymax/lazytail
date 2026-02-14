# ADR-013: Live Filter Preview with Debouncing

## Status

Accepted

## Context

Users expect to see filter results as they type, not just after pressing Enter. But triggering a full filter on every keystroke would cause excessive work and UI lag, especially on large files.

Options considered:
1. **Filter on submit only** - no preview, filter runs on Enter
2. **Filter on every keystroke** - immediate but potentially laggy
3. **Debounced preview** - wait for typing pause, then filter

## Decision

We use **debounced live filter preview** with a 500ms delay:

1. On each keystroke (`FilterInputChar`, `FilterInputBackspace`, `HistoryUp/Down`, `ToggleFilterMode`):
   - Immediately cancel any in-progress filter via `CancelToken`
   - Schedule a filter to run at `now + 500ms` (`pending_filter_at`)
2. On each main loop iteration, check if `pending_filter_at` has elapsed
3. If elapsed: run `trigger_live_filter_preview()` which starts the filter
4. On submit (Enter): bypass debounce, trigger filter immediately
5. On cancel (Esc): clear `pending_filter_at`, cancel any in-progress filter, restore original position via `filter.origin_line`

Partial results arrive via `FilterProgress::PartialResults` during the filter, so the UI updates incrementally even for large files.

To prevent visual "blink" when a new filter starts, clearing old results is **deferred** until the first batch of new results arrives (`tab.filter.needs_clear` flag).

## Consequences

**Benefits:**
- Users see results while typing without pressing Enter
- 500ms debounce prevents excessive work during fast typing
- Immediate cancellation on new keystroke avoids wasted computation
- Deferred clear prevents empty-screen blink between filters
- Esc restores the original viewport position (via `origin_line`)

**Trade-offs:**
- 500ms delay before preview appears (acceptable for interactive use)
- Complex interaction between debounce timer, cancellation, and partial results
- `pending_filter_at` adds temporal state to the main loop
