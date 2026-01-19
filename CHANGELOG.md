# Changelog

## 0.1.0 (2026-01-19)


### Features

* add automated release PR workflow ([603ceed](https://github.com/raaymax/lazytail/commit/603ceedfae712233c55ec47b888ef7866fb76da5))


### Bug Fixes

* add tempfile dev dependency for tests ([a519f79](https://github.com/raaymax/lazytail/commit/a519f795802baa59d401f401abf871c66e6d655d))
* address high-priority code review issues ([bf6122b](https://github.com/raaymax/lazytail/commit/bf6122b28b607ea3b97f9b0c84ccef711f655e29))
* preserve selection when clearing filter ([aeb5445](https://github.com/raaymax/lazytail/commit/aeb5445aff1e4b315d6dc6ab87d7e6cd28f4d5c8))
* resolve all clippy warnings ([178dcee](https://github.com/raaymax/lazytail/commit/178dceeeae012a9a6a201ced3a428660ead83cbb))

## [0.1.0] - 2026-01-19

### Added
- Initial release of LazyTail - universal terminal-based log viewer
- Lazy file reading for efficient handling of large log files
- TUI interface with keyboard navigation
- Live filtering with instant results
- Background filtering (non-blocking regex and string matching)
- File watching with auto-reload
- Follow mode (auto-scroll to latest logs)
- ANSI color support for colored application logs
- Incremental filtering for improved performance
- Viewport-based rendering for low memory usage
- Works with any text-based log files (application, system, container, etc.)

### Features
- Navigation: Arrow keys, Page Up/Down, g/G for first/last line
- Filtering: Live filter mode with instant results
- Follow mode: Auto-scroll to new logs (like `tail -f`)
- Universal: Works with logs from any source (Docker, Kubernetes, web servers, applications)
- Colored logs: Full ANSI escape code support

[0.1.0]: https://github.com/raaymax/lazytail/releases/tag/v0.1.0
