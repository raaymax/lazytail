# Changelog

## [0.5.0](https://github.com/raaymax/lazytail/compare/v0.4.0...v0.5.0) (2026-02-11)


### Features

* add config system with discovery, loading, and CLI commands ([#11](https://github.com/raaymax/lazytail/issues/11)) ([6afaf66](https://github.com/raaymax/lazytail/commit/6afaf66ac701eb45f18f016c695c9eb023139f98))
* **mcp:** accept source names instead of file paths in tool API ([cd064b6](https://github.com/raaymax/lazytail/commit/cd064b640af874a92335a883a31f98a1f912a01d))
* **mcp:** accept source names instead of file paths in tool API ([2ace327](https://github.com/raaymax/lazytail/commit/2ace3270e7da649ff8dce9fad30015b3d2852c28))
* **mcp:** accept source names instead of file paths in tool API ([#18](https://github.com/raaymax/lazytail/issues/18)) ([cd064b6](https://github.com/raaymax/lazytail/commit/cd064b640af874a92335a883a31f98a1f912a01d))
* **mcp:** add plain text output format to reduce JSON escaping ([#13](https://github.com/raaymax/lazytail/issues/13)) ([2792865](https://github.com/raaymax/lazytail/commit/2792865a9b3379685c58c6061cf74b61f016c7ff))
* **mcp:** wire query language into search tool ([#19](https://github.com/raaymax/lazytail/issues/19)) ([727bb22](https://github.com/raaymax/lazytail/commit/727bb223bb0bc7c888832e38303ee17bbe44dd41))
* **ui:** add close confirmation dialog for tabs ([#15](https://github.com/raaymax/lazytail/issues/15)) ([d8f3dfd](https://github.com/raaymax/lazytail/commit/d8f3dfdde7551af269da93795f61e44290e4cc87))
* **ui:** display file path in header and add y to copy source path ([#17](https://github.com/raaymax/lazytail/issues/17)) ([c2c4995](https://github.com/raaymax/lazytail/commit/c2c4995d22d7f33bd3758e0a50211d60c0b0db94))


### Bug Fixes

* **config:** use ~/.config/ for storage on all platforms ([#16](https://github.com/raaymax/lazytail/issues/16)) ([bf8bbb1](https://github.com/raaymax/lazytail/commit/bf8bbb127f7f185e25bd92c90ad39ef110557dd3))

## [0.4.0](https://github.com/raaymax/lazytail/compare/v0.3.0...v0.4.0) (2026-01-31)


### Features

* add filter progress percentage and streaming filter ([0b2e1e3](https://github.com/raaymax/lazytail/commit/0b2e1e332fae4b99787f317004ff6d44eca375ec))
* add get_tail MCP tool and fix parameter schemas ([1fecd64](https://github.com/raaymax/lazytail/commit/1fecd642a4f88d411791da510f8a5d579860a95c))
* add list_sources MCP tool ([fb6aea0](https://github.com/raaymax/lazytail/commit/fb6aea079561b1c711bacc538106ea1bab3be6d9))
* add MCP server support ([a11401d](https://github.com/raaymax/lazytail/commit/a11401dca565ce341ce6e31bf7b58539264db106))
* add source discovery and capture mode ([9ab50c2](https://github.com/raaymax/lazytail/commit/9ab50c25dff3daf7a7334ad4517bf5beceb561bf))
* add source discovery and capture mode (Phase 2 & 3) ([cba0796](https://github.com/raaymax/lazytail/commit/cba07965b1af63d947a7c05c4b6a3e0536745fce))
* add tree-like source panel with category navigation ([5dd3903](https://github.com/raaymax/lazytail/commit/5dd390386860777698b3d6a0e2e86a9204cd1c2f))
* background loading for pipes and stdin ([b440ba1](https://github.com/raaymax/lazytail/commit/b440ba115f58482493d5abfe6cc1f51486596d5b))
* delete ended sources when closing tab ([d7896d0](https://github.com/raaymax/lazytail/commit/d7896d0aed54bae39e6cbbb8e38ebe0b137e8e72))
* enable MCP feature by default ([7d11c03](https://github.com/raaymax/lazytail/commit/7d11c038be3019d62618b02db419b7de3d7da039))
* highlight focused panel border ([1ecf14d](https://github.com/raaymax/lazytail/commit/1ecf14d8ca3b5afd7416be3abebfb6268c1c263d))
* **mcp:** add MCP server for AI assistant integration ([439383b](https://github.com/raaymax/lazytail/commit/439383b9405b3bf5eaf6df9fa3f4aa20a2641afb))
* optimize MCP search with streaming filter and fix lines_searched tracking ([4add1d4](https://github.com/raaymax/lazytail/commit/4add1d4e8e6cfafc5e882bce675e21b52512585a))
* tree-like source panel with Tab navigation ([d058405](https://github.com/raaymax/lazytail/commit/d05840559c9cbcedb2d1b8d3ba55805767fa5745))


### Bug Fixes

* allow dead_code for FilterProgress to fix CI without MCP feature ([861489a](https://github.com/raaymax/lazytail/commit/861489ad92ce489c9db8b8e9be5ff43e893d95c5))
* comprehensive bug fixes from code review ([a45926a](https://github.com/raaymax/lazytail/commit/a45926a9520b1638f45f19d8018f269bd00b2f68))
* handle edge cases in is_pid_running for macOS ([5dae000](https://github.com/raaymax/lazytail/commit/5dae0000e7781b14af8e7d8aee971ca240f4f8e0))
* make is_pid_running cross-platform for macOS ([ef48663](https://github.com/raaymax/lazytail/commit/ef4866369d3a9676b32f9235a9d5362b27e5a9f5))
* multiple filtering and input handling improvements ([628d36a](https://github.com/raaymax/lazytail/commit/628d36a16e1ad7e27bee730ae32af37598be716c))
* prevent results blink when changing filter ([d6a0e13](https://github.com/raaymax/lazytail/commit/d6a0e13bb3daf0749bccc2f0e2fdc9e3792080cd))
* process one key event per iteration for multi-key sequences ([cb8a2f6](https://github.com/raaymax/lazytail/commit/cb8a2f612cad1e04d0d7d625768cf0e2c529a145))
* refresh source status on each render cycle ([0d2301f](https://github.com/raaymax/lazytail/commit/0d2301fcd5c835ab1e57fe19eb928faab2ba69d6))
* remove redundant libc import ([9ff9144](https://github.com/raaymax/lazytail/commit/9ff9144ac131ab633d5b582fef46b4f20e720d48))
* resolve clippy warnings for newer Rust versions ([c7e8f2a](https://github.com/raaymax/lazytail/commit/c7e8f2afd6cd459d65b7e9de438c3227a120fa3d))

## [0.3.0](https://github.com/raaymax/lazytail/compare/0.2.0...v0.3.0) (2026-01-26)


### Features

* add advanced filter modes with regex support and cursor navigation ([bbb12c8](https://github.com/raaymax/lazytail/commit/bbb12c815cced10abecb6f2503cc149e41c5235d))
* add Ctrl+E/Ctrl+Y viewport scrolling and major refactor ([f14563f](https://github.com/raaymax/lazytail/commit/f14563f8aad79c19e9cf203633409f4d73b43dd0))
* add expandable log entries and default follow mode ([293fb9b](https://github.com/raaymax/lazytail/commit/293fb9b3e7f8963cce2b083f4bfe63b92629655e))
* add stats panel showing line counts in side panel ([d6fadbf](https://github.com/raaymax/lazytail/commit/d6fadbfda29d75fbedddd0f8bae26443df1a576d))
* persist filter history to disk ([d6eded9](https://github.com/raaymax/lazytail/commit/d6eded9aa6fdc7ad7196b622f5a97413239bc8c4))


### Bug Fixes

* change case sensitivity toggle to Alt+C ([ab82ca6](https://github.com/raaymax/lazytail/commit/ab82ca61d9e1f88cda28930676451595f391695c))
* remove src/ from gitignore ([b93d43f](https://github.com/raaymax/lazytail/commit/b93d43f8b086d9f4c1ad032273c357835ba7c0bd))

## [0.2.0](https://github.com/raaymax/lazytail/compare/v0.1.0...v0.2.0) (2026-01-23)


### Features

* add filter history with arrow key navigation ([0136352](https://github.com/raaymax/lazytail/commit/01363529da6da3406e923d7fa906cc828d403756))
* add help overlay with keyboard shortcuts ([eda6d1f](https://github.com/raaymax/lazytail/commit/eda6d1f9f311249437374ce5ca16c2192bff955c))
* add mouse scroll support ([b65f482](https://github.com/raaymax/lazytail/commit/b65f4823df3d83ccb60973d86e608d7716f22f30))
* add multi-tab support with side panel UI ([c20926c](https://github.com/raaymax/lazytail/commit/c20926ccb512ffb04e4570baa12ef28975048959))
* add stdin support with auto-detection ([b90ce34](https://github.com/raaymax/lazytail/commit/b90ce347fcec81801341e20d7bb7305ca3359c53))
* add viewport system for stable filter selection ([28e7837](https://github.com/raaymax/lazytail/commit/28e7837d998afefd3474eab9551da05e2d59cc57))
* add vim-style line jump with :number command ([4bfdb92](https://github.com/raaymax/lazytail/commit/4bfdb9237085ee33e2814962f835abaea7fb721a))
* add vim-style z commands for view positioning ([5883c41](https://github.com/raaymax/lazytail/commit/5883c41fbf791195057913720d5d4e8e0b969c2a))
* multi-tab support with stdin and improved filtering UX ([3678c7d](https://github.com/raaymax/lazytail/commit/3678c7d9d10ea7598dfa768ef89d07f06415ad62))


### Bug Fixes

* clear help overlay background to prevent text bleed-through ([c022e4c](https://github.com/raaymax/lazytail/commit/c022e4cbd2b996ea5da4f9a0796809a0e4d706cf))
* make mouse scroll follow selection like vim ([3aad03b](https://github.com/raaymax/lazytail/commit/3aad03b053efe9ca265031c6eeec798e5827d446))
* prevent adjust_scroll interference with mouse scrolling ([409d710](https://github.com/raaymax/lazytail/commit/409d710898b572fd723cdf5bdff28244ece90743))
* remap gray text colors for visibility on selection background ([6c588a0](https://github.com/raaymax/lazytail/commit/6c588a0f02e3ce174ca8f8470d7a022c946e8167))
* trigger live filter when navigating history ([5453e4e](https://github.com/raaymax/lazytail/commit/5453e4e340b09c844fd22fc3db29e51943d31f37))

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
