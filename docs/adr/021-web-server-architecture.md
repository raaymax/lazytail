# ADR-021: Web Server Architecture

## Status

Accepted

## Context

LazyTail needed a browser-based interface for users who prefer GUIs or need to share log views. The web server is a secondary interface — the TUI remains primary. Design constraints:

- Minimal additional dependencies (LazyTail is a CLI tool, not a web framework)
- Share domain state with the TUI (same `LogSource`, filters, indexes)
- Single binary deployment (no separate frontend build or static file serving)
- Sufficient for localhost usage, not a production web service

Alternatives considered:
- **actix-web / axum**: Full-featured async web frameworks. Would pull in a large dependency tree and require restructuring around async. Overkill for a simple REST API serving one user.
- **Separate frontend**: React/Vue SPA with a build pipeline. Adds toolchain complexity, CI steps, and a deployment concern. A single embedded HTML file is simpler.

## Decision

Use **tiny_http** for the HTTP server with an **embedded single-page application** (HTML inlined as a string constant).

Architecture:
- `lazytail web` starts a `tiny_http::Server` on `127.0.0.1:8421` (configurable via `--host` and `--port`)
- The SPA is served from a single `INDEX_HTML` string constant — no static file directory needed
- App state is shared via `Arc<Mutex<App>>` between the HTTP handler thread and the main event loop
- Long-polling for updates: clients poll `/api/events` which blocks until state changes or a 25-second timeout

Key API endpoints:
- `GET /api/sources` — list sources with severity counts and filter state
- `GET /api/lines` — paginated line content with per-line severity
- `POST /api/filter` — trigger filter via `FilterOrchestrator`
- `POST /api/filter/clear` — cancel and clear filter
- `POST /api/follow` — toggle follow mode

Request limits prevent abuse: `MAX_LINES_PER_REQUEST` (5,000), `MAX_REQUEST_BODY_SIZE` (1MB), `MAX_PENDING_EVENT_REQUESTS` (256).

## Consequences

**Benefits:**
- Single binary — no separate frontend build or static files to deploy
- Minimal dependencies — tiny_http is small and synchronous
- Shared state — web UI sees exactly the same data as TUI, no duplication
- Simple deployment — `lazytail web` just works

**Trade-offs:**
- Single-threaded request handling limits concurrent users (acceptable for localhost)
- Embedded HTML means frontend changes require a Rust rebuild
- No WebSocket support — long-polling has higher latency than push-based updates
- `Arc<Mutex<App>>` creates contention between HTTP handler and event loop under load
