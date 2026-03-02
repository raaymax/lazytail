# ADR-022: Session Persistence

## Status

Accepted

## Context

Users working on multi-service projects frequently return to the same log source across sessions. Without session persistence, every launch starts with no source selected, requiring manual navigation to the desired tab.

The session system needed to:
- Remember which source was last active per project
- Distinguish between different project contexts
- Handle missing or corrupted session data gracefully
- Not interfere with test execution

## Decision

Store per-project session state in a single JSON file at `~/.config/lazytail/session.json`.

**Scope:** The session file maps **project context keys** to **last-active source names**. The context key is the project root path (from `lazytail.yaml` discovery) or `"__global__"` for non-project usage.

**Format:**
```json
{
  "contexts": {
    "/home/user/project-a": { "last_source": "API" },
    "/home/user/project-b": { "last_source": "Worker" },
    "__global__": { "last_source": "syslog" }
  }
}
```

**Bounded storage:** The file caps at 100 context entries (`MAX_CONTEXTS`) to prevent unbounded growth from users switching between many projects.

**Graceful degradation:** All session operations use `Option` chains — if the file is missing, corrupted, or unreadable, the app starts normally without a last-source preference. No error dialogs or warnings.

**Test isolation:** Session I/O functions are conditionally compiled (`#[cfg(not(test))]`) to prevent tests from reading or writing the user's real session file.

## Consequences

**Benefits:**
- Seamless workflow continuity — open LazyTail and immediately see the source you were last viewing
- Project-scoped: different projects remember different sources
- Zero configuration required — works automatically after first use
- Graceful degradation — corrupted or missing session data is silently ignored

**Trade-offs:**
- Only stores last-active source, not full session state (open tabs, filter history, scroll position)
- Single JSON file may have write conflicts if multiple LazyTail instances save simultaneously (unlikely in practice)
- Conditional compilation for test isolation means test code can't verify session persistence behavior directly
