# ADR-005: Vim-Style Viewport Navigation

## Status

Accepted

## Context

A log viewer needs a scrolling model that feels familiar to developers. The viewport must handle content changes gracefully (file growth, filter application/removal) without losing the user's position.

Options considered:
1. **Scroll-position based** - track scroll offset, derive selection
2. **Selection-based** - track selected line, derive scroll position
3. **Anchor-based** - track a file line number (anchor), resolve against current content

## Decision

We use an **anchor-based viewport** (`Viewport` struct) with vim-style scrolling:

- **Anchor line**: a file line number that is the current selection. Stable across filter changes because it refers to the actual file line, not an index into the filtered view.
- **Edge padding** (scrolloff): 3 lines of padding at top/bottom. Selection moves freely within the comfort zone; viewport scrolls only when selection hits the padding boundary.
- **Binary search resolution**: when content changes (filter applied/removed), `resolve()` uses binary search to find the anchor in the new `line_indices`. If not found, snaps to the nearest line.

Supported commands mirror vim:
- `j/k` - move selection (viewport scrolls at edges)
- `Ctrl+E/Y` - scroll viewport, selection moves with it (same screen position)
- `zz/zt/zb` - center/top/bottom selection on screen
- `G/gg` - jump to end/start
- `PageDown/PageUp` - page navigation
- Mouse scroll - moves both selection and viewport together

## Consequences

**Benefits:**
- Users on line 500 stay on line 500 when a filter is applied (if that line matches)
- When a filter is cleared, the user returns to the same file line they were viewing
- vim-like feel is natural for the target audience (developers viewing logs)
- Edge padding prevents the jarring experience of selection at the very edge of the screen

**Trade-offs:**
- More complex than simple scroll-position tracking
- `resolve()` must be called after any content change to sync the viewport
- Some edge cases when the anchor line is removed by filtering (snaps to nearest, which may not be the ideal choice in all cases)
