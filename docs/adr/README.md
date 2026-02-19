# Architecture Decision Records

This directory contains Architecture Decision Records (ADRs) for LazyTail. Each ADR documents a significant architectural decision, its context, and consequences.

## Index

| ADR | Title | Status |
|-----|-------|--------|
| [001](001-event-driven-architecture.md) | Event-Driven Architecture | Accepted |
| [002](002-mmap-streaming-filter.md) | mmap-Based Streaming Filter | Accepted |
| [003](003-sparse-index-file-reader.md) | Sparse Indexing for File Reader | Accepted |
| [004](004-channel-based-filter-communication.md) | Channel-Based Filter Communication | Accepted |
| [005](005-vim-style-viewport.md) | Vim-Style Viewport Navigation | Accepted |
| [006](006-pid-source-tracking.md) | PID-Based Source Tracking | Accepted |
| [007](007-config-discovery.md) | Config Discovery via Parent Directory Walk | Accepted |
| [008](008-flag-based-signals.md) | Flag-Based Signal Handling | Accepted |
| [009](009-multi-tab-independent-state.md) | Multi-Tab Model with Independent State | Accepted |
| [010](010-mcp-server.md) | MCP Server Integration | Accepted |
| [011](011-project-scoped-storage.md) | Project-Scoped vs Global Source Storage | Accepted |
| [012](012-incremental-filtering.md) | Incremental Filtering on File Growth | Accepted |
| [013](013-live-filter-preview.md) | Live Filter Preview with Debouncing | Accepted |
| [014](014-hexagonal-log-source-state.md) | Hexagonal Architecture — LogSourceState Extraction | Accepted |

## Format

Each ADR follows the format:
- **Status**: Accepted, Superseded, or Deprecated
- **Context**: The problem and constraints that led to the decision
- **Decision**: What was decided and why
- **Consequences**: Benefits and trade-offs of the decision
