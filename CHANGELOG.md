# Changelog

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
