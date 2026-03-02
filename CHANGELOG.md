# Changelog

## [0.8.0](https://github.com/raaymax/lazytail/compare/v0.7.0...v0.8.0) (2026-03-02)


### Features

* **bench:** add filter benchmarking CLI subcommand ([#50](https://github.com/raaymax/lazytail/issues/50)) ([a45ca20](https://github.com/raaymax/lazytail/commit/a45ca20da231ab8939cdc51cab9a5ff8d0852ba9))
* **bench:** compare generic and SIMD paths for plain text benchmarks ([#65](https://github.com/raaymax/lazytail/issues/65)) ([017188b](https://github.com/raaymax/lazytail/commit/017188b79b7abafb35677f8808eb101404e9e5d2))
* **clipboard:** add y keybinding to copy selected line ([#44](https://github.com/raaymax/lazytail/issues/44)) ([f7dd854](https://github.com/raaymax/lazytail/commit/f7dd8543e199083b03932b70f8ef0868d97b0609))
* **combined-view:** add merged chronological view for source categories ([#48](https://github.com/raaymax/lazytail/issues/48)) ([8d5ff18](https://github.com/raaymax/lazytail/commit/8d5ff1814a2bfa186572a974330e35332c72ebda))
* **combined-view:** per-category combined tabs with follow mode fix ([#49](https://github.com/raaymax/lazytail/issues/49)) ([40861db](https://github.com/raaymax/lazytail/commit/40861db78807c77429a699647a46958837a9cb0a))
* **filter:** add count-by aggregation with TUI view and MCP support ([#46](https://github.com/raaymax/lazytail/issues/46)) ([331e2ae](https://github.com/raaymax/lazytail/commit/331e2aefd5045c3bd8e5041fe410f87dc3f51daa))
* **filter:** add explicit Query filter mode via Tab cycling ([#47](https://github.com/raaymax/lazytail/issues/47)) ([136d779](https://github.com/raaymax/lazytail/commit/136d7799989a6fadff04e9cb5ffada82a823f912))
* **help:** add scrollable help overlay with j/k navigation ([#43](https://github.com/raaymax/lazytail/issues/43)) ([75ee3f4](https://github.com/raaymax/lazytail/commit/75ee3f45739533862134aac29d8813da65f15b75))
* **mcp:** add since_line parameter to get_tail for incremental polling ([bd88113](https://github.com/raaymax/lazytail/commit/bd88113d7f34a42cf0153d7ad5257bce03df1ee0))
* **mouse:** add mouse click support for side panel and log view ([#45](https://github.com/raaymax/lazytail/issues/45)) ([e459bfc](https://github.com/raaymax/lazytail/commit/e459bfc15b10e9a176aa89030775a5abb4362684))
* remember last opened source and fix tab shortcut mapping ([#62](https://github.com/raaymax/lazytail/issues/62)) ([22e822c](https://github.com/raaymax/lazytail/commit/22e822cf9622df166f54c2149627e6fa42aea7ea))
* **renderer:** add capture rendering, theme-aware styles, and external presets ([#61](https://github.com/raaymax/lazytail/issues/61)) ([84ff261](https://github.com/raaymax/lazytail/commit/84ff261d048889643deff930fb8d99514cbc36aa))
* **renderer:** add field paths, style maps, max width, and compound styles ([#55](https://github.com/raaymax/lazytail/issues/55)) ([2f12eee](https://github.com/raaymax/lazytail/commit/2f12eeea41559392c4c7e950edb43ac30e7518c7))
* **renderer:** add MCP rendering, field formatting, and conditional styling ([#59](https://github.com/raaymax/lazytail/issues/59)) ([2038433](https://github.com/raaymax/lazytail/commit/2038433adadb89c425d5e17edce8e36c75a5582c))
* **renderer:** add MCP rendering, field formatting, and conditional styling ([#59](https://github.com/raaymax/lazytail/issues/59)) ([6dd34ac](https://github.com/raaymax/lazytail/commit/6dd34acbe976778ca52002d4c79d16f083958606))
* **renderer:** add YAML-configurable rendering presets for structured log lines ([#52](https://github.com/raaymax/lazytail/issues/52)) ([960529b](https://github.com/raaymax/lazytail/commit/960529b750cc399659de906ab281bbc7ebc839a6))
* **renderer:** wire discovered sources to config renderers and add conversation preset ([f96224b](https://github.com/raaymax/lazytail/commit/f96224b20ac4eb5292708c2a8c310bfdaa7d8f78))
* **theme:** add {project_root}/themes/ to theme search paths ([#57](https://github.com/raaymax/lazytail/issues/57)) ([ed0e890](https://github.com/raaymax/lazytail/commit/ed0e890d40c9ce0fc06284fff6304b2cf3368ab6))
* **theme:** add configurable color scheme system with built-in themes ([#53](https://github.com/raaymax/lazytail/issues/53)) ([4b751cc](https://github.com/raaymax/lazytail/commit/4b751cc50095512a07f2bb38971106938a4fd16d))
* **theme:** add external theme file loading, inheritance, and CLI ([#54](https://github.com/raaymax/lazytail/issues/54)) ([ecee32c](https://github.com/raaymax/lazytail/commit/ecee32c4db8088e9ceccf0391d9b1c49daa4b776))
* **theme:** add multi-format import and remove project_root/themes/ path ([#63](https://github.com/raaymax/lazytail/issues/63)) ([46698fe](https://github.com/raaymax/lazytail/commit/46698fee2fb7baf2c1a94aac05df833a980f579b))
* **theme:** apply palette.background as TUI background color ([#58](https://github.com/raaymax/lazytail/issues/58)) ([6bd8127](https://github.com/raaymax/lazytail/commit/6bd8127a89bbd9703548cf9be23c6bcfba741722))
* **tui:** add raw mode toggle to bypass preset and ANSI rendering ([#66](https://github.com/raaymax/lazytail/issues/66)) ([9819e41](https://github.com/raaymax/lazytail/commit/9819e41e112ac1376612749a3c6865a3ee894ddc))


### Bug Fixes

* **index:** detect and reject stale columnar indexes on file replacement ([#56](https://github.com/raaymax/lazytail/issues/56)) ([d7f62f6](https://github.com/raaymax/lazytail/commit/d7f62f62d7b2ee723030c5e91cd2b0bd041c33d3))
* **index:** validate index freshness in `get_stats` before returning metadata ([#64](https://github.com/raaymax/lazytail/issues/64)) ([4ebc16e](https://github.com/raaymax/lazytail/commit/4ebc16e39e5bc48271219e3b4975483858aca099))
* install script fails due to GitHub API rate limiting ([#39](https://github.com/raaymax/lazytail/issues/39)) ([9cfd015](https://github.com/raaymax/lazytail/commit/9cfd015959647d9f54e96509e7599e9513b86da2))
* **renderer:** prevent empty lines from mismatched preset auto-detection ([4dd1703](https://github.com/raaymax/lazytail/commit/4dd1703b1d61503d6c9d09ac3e8adb9f8917960a))
* **tui:** prevent expanded lines from clipping at screen bottom ([#51](https://github.com/raaymax/lazytail/issues/51)) ([14f2bec](https://github.com/raaymax/lazytail/commit/14f2bec0b20c8beadbf52d29504f5acff0246ee7))
* **tui:** unify sidebar selection background color for overflow overlay ([bf6327b](https://github.com/raaymax/lazytail/commit/bf6327bc1216355799445f67aff8a0665e019d35))
* **tui:** use normal background for metadata in sidebar overflow overlay ([39b53b4](https://github.com/raaymax/lazytail/commit/39b53b4023973876b8ba934edb2e4e140faa152f))
* **watcher:** TUI not refreshing on macOS due to missed file events ([#41](https://github.com/raaymax/lazytail/issues/41)) ([f28c44d](https://github.com/raaymax/lazytail/commit/f28c44d4bb05e0b72ae43e154ffb4b47667e64de))

## [0.7.0](https://github.com/raaymax/lazytail/compare/v0.6.0...v0.7.0) (2026-02-23)


### Features

* **cli:** add a simple web client ([#30](https://github.com/raaymax/lazytail/issues/30)) ([26afdbe](https://github.com/raaymax/lazytail/commit/26afdbe42c22671ecc877727bbfa161882580dfe))
* severity line-number coloring, live stats, and ingestion rate ([9734a72](https://github.com/raaymax/lazytail/commit/9734a72222dd177cfe54f0dc39103a3bfba66d31))
* **update:** add self-update with CLI subcommand and background check ([#36](https://github.com/raaymax/lazytail/issues/36)) ([c0d4fd7](https://github.com/raaymax/lazytail/commit/c0d4fd7b653a7a69afbb529010ba7c91e250c137))


### Bug Fixes

* **ci:** auto-trigger release builds when release-please creates releases ([97ace90](https://github.com/raaymax/lazytail/commit/97ace90159397db27c0e8f3fad1b74047e5e7824))
* **examples:** add lz4_flex dev-dep and bench_compression example ([662ddd8](https://github.com/raaymax/lazytail/commit/662ddd84dc395dd4fe5c4d046106bc8cf3ab6577))
* harden atomicity, error handling, and safety across codebase ([b48b719](https://github.com/raaymax/lazytail/commit/b48b7199023c8cb42eb78e8e43273169a052d4c9))
* **index:** add writer lock and truncate orphaned entries on resume ([2a6c286](https://github.com/raaymax/lazytail/commit/2a6c286ec081d964f707b1e2685f701e4507c548))
* **perf:** resolve TUI freeze during active capture streams ([dd72e63](https://github.com/raaymax/lazytail/commit/dd72e6354a43a4a8f1ca2740cc5863242ee9ff43))
* **source:** show global sources alongside project sources ([d8fc3bd](https://github.com/raaymax/lazytail/commit/d8fc3bd4871ba2cb60eec6734900a0a887a490a6))

## [0.6.0](https://github.com/raaymax/lazytail/compare/v0.5.3...v0.6.0) (2026-02-19)


### Features

* **index:** add columnar index system ([#34](https://github.com/raaymax/lazytail/issues/34)) ([2d1d4e5](https://github.com/raaymax/lazytail/commit/2d1d4e5cd2195d38d54c4b39f2a0fb472135c1ec))
* **ui:** show line count and file size per source in side panel ([#33](https://github.com/raaymax/lazytail/issues/33)) ([15830dd](https://github.com/raaymax/lazytail/commit/15830ddfb9b46d69f4db3f5ec2b3d4b58ce84cea))


### Bug Fixes

* **ci:** trigger release build on tag push instead of workflow dispatch ([#26](https://github.com/raaymax/lazytail/issues/26)) ([a435292](https://github.com/raaymax/lazytail/commit/a43529209fc3a630b869aa32176cbda176b0ba98))
* **source:** check project-local markers for TUI source status ([#29](https://github.com/raaymax/lazytail/issues/29)) ([08a07bf](https://github.com/raaymax/lazytail/commit/08a07bfc2badbadf8d3f1223bdca9a1bf009d407))

## [0.5.3](https://github.com/raaymax/lazytail/compare/v0.5.2...v0.5.3) (2026-02-13)


### Bug Fixes

* **ci:** resolve empty PR number in release notes step ([8b66287](https://github.com/raaymax/lazytail/commit/8b662870cc3e925bf8131666a2e612e273afd42d))
* **source:** handle EPERM in macOS PID liveness check ([#25](https://github.com/raaymax/lazytail/issues/25)) ([cf87b07](https://github.com/raaymax/lazytail/commit/cf87b0787652cfde04c29ed201a2c1196fe92e9a))

## [0.5.2](https://github.com/raaymax/lazytail/compare/v0.5.1...v0.5.2) (2026-02-13)


### Bug Fixes

* **mcp:** resolve sources from project-local data directory ([#22](https://github.com/raaymax/lazytail/issues/22)) ([7e48e61](https://github.com/raaymax/lazytail/commit/7e48e61894fc3414076419e77af4cd36d3330c15))

## [0.5.1](https://github.com/raaymax/lazytail/compare/v0.5.0...v0.5.1) (2026-02-13)


### Bug Fixes

* **capture:** clean up stale markers from killed processes ([#20](https://github.com/raaymax/lazytail/issues/20)) ([09235f1](https://github.com/raaymax/lazytail/commit/09235f180f3ab49e88524184eeabe9957d70aa03))

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
