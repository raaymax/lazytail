# Project Research Summary

**Project:** LazyTail — Project-scoped configuration and signal handling
**Domain:** CLI tool configuration and Unix signal integration
**Researched:** 2026-02-03
**Confidence:** HIGH

## Executive Summary

This research covers adding project-scoped configuration (lazytail.yaml) and robust signal handling to LazyTail, a terminal-based log viewer. The domain is well-established with clear patterns from tools like Git, Cargo, and ripgrep. The recommended approach uses YAML for configuration files (via serde-saphyr) with hierarchical precedence (project overrides global), and signal-hook's flag pattern for cleanup coordination.

The key technical challenge is integrating configuration loading into the existing event-driven architecture without disrupting the render-collect-process cycle. Configuration must load before the main event loop but after CLI argument parsing, creating a clear initialization phase. Signal handling requires careful attention to async-signal-safety — the existing capture.rs implementation uses the correct pattern but has one issue (calling process::exit from signal handler) that needs fixing.

The main risks are: (1) infinite loops during config file discovery across filesystem boundaries, (2) race conditions between signal handlers and normal cleanup paths, and (3) orphaned marker files when processes are killed with SIGKILL. All three have proven mitigation strategies from the ecosystem. The existing codebase already implements good patterns (atomic marker creation, PID validation) that reduce risk.

## Key Findings

### Recommended Stack

The existing Rust stack (ratatui, crossterm, signal-hook 0.3, serde, dirs) is solid. Two key additions are needed:

**Core technologies:**
- **serde-saphyr 0.0**: YAML parsing — actively maintained (Feb 2026 release), panic-free parsing critical for CLI tools, solves "Norway problem" with type-driven parsing, 834+ passing tests. Avoids deprecated serde_yaml (archived March 2024) and serde_yml (archived Sept 2025).
- **signal-hook 0.4.3**: Unix signal handling — already in use at 0.3.x, update recommended. Provides flag pattern for async-signal-safe cleanup, last release Jan 2026. Recommended by Rust CLI Book.
- **dirs 5.0**: Platform config paths — already in use, sufficient for both global and project-local path resolution.

**Version updates needed:**
- signal-hook: 0.3 → 0.4.3 (no breaking changes for current usage)

**Format decision:**
YAML chosen over TOML for cleaner nested structures and native multi-line strings (important for log filter patterns).

### Expected Features

**Must have (table stakes):**
- Config file auto-discovery — walk up directories until config found or root reached
- Hierarchical precedence — CLI args > env vars > project > global (standard pattern from Git, Cargo)
- YAML format support — human-readable, comment-friendly, already specified in PROJECT.md
- Graceful missing config — tool works without config file using defaults
- Clear error messages — include file path, line number, and what went wrong
- Global config location — XDG compliance (~/.config/lazytail/)
- Backwards compatibility — existing global streams in ~/.config/lazytail/data/ must keep working

**Should have (competitive):**
- Named source definitions — define sources by name in config, open with `lazytail open backend`
- Config validation command — `lazytail config validate` catches errors before runtime
- Config introspection — `lazytail config show` displays merged effective config
- Local override file — `lazytail.local.yaml` for personal overrides not committed to git
- Config init command — `lazytail init` generates starter config with comments

**Defer (v2+):**
- Source groups — group related sources to open multiple tabs with one command
- Watch paths for auto-discovery — complex feature requiring directory watcher integration
- JSON Schema for editor support — polish feature for autocomplete in VS Code
- Environment variable overrides — add when CI/CD use cases emerge

### Architecture Approach

Configuration loading must happen in a new initialization phase between CLI argument parsing and the main event loop. The key pattern is layered configuration with explicit precedence merging using Option<T> fields in serde structs.

**Major components:**
1. **Config struct** — holds all configurable values with Option<T> fields, implements merge() for precedence
2. **ConfigLoader** — finds config files (project-root discovery via parent walk), parses YAML, merges layers
3. **Project root discovery** — walks up from cwd looking for lazytail.yaml or .lazytail/ directory, stops at filesystem boundaries
4. **Integration with Args** — CLI arguments override config values after loading
5. **Signal handler coordination** — flag pattern with Arc<AtomicBool> for async-signal-safe cleanup

**Data flow:** Parse CLI args → Discover project root → Load config layers (defaults, global, project, env vars, CLI) → Resolve paths → Mode detection → TabState creation → Event loop

**Key patterns:**
- Stop at filesystem boundaries during discovery (compare st_dev, check for $HOME)
- Use Option<T>.or() chain for precedence merging
- Lock project root at startup, never change it mid-session
- Lazy project directory creation (only create .lazytail/ when first needed)

### Critical Pitfalls

1. **Async-signal-unsafe operations in signal handlers** — Never call functions that allocate, lock mutexes, or do I/O inside signal handlers. Signal can interrupt at any point including while holding locks. Prevention: Use signal_hook::flag::register() which only sets AtomicBool. Main loop checks flag and performs cleanup in normal code path. LazyTail's capture.rs already uses correct pattern but calls process::exit(0) which should be removed.

2. **Race condition between signal and normal exit** — Signal arrives after cleanup begins, or cleanup runs twice. Prevention: Single cleanup path with AtomicBool compare_exchange to ensure runs exactly once. Remove process::exit() from signal handler, let main loop exit after cleanup.

3. **Config file discovery infinite loop** — Naive parent walk can loop forever on symlink cycles, traverse slow network mounts, or find wrong config in parent directory. Prevention: Stop at filesystem boundaries (check stat().st_dev changes), stop at $HOME and /, maximum depth limit (32 levels), use canonical paths to resolve symlinks upfront.

4. **SIGKILL cannot be caught** — kill -9 terminates immediately without running any cleanup, leaving orphaned marker files. Prevention: Detect stale markers on startup (check if PID still running). LazyTail already has is_pid_running() in source.rs — expand this to all cleanup artifacts.

5. **Memory ordering too weak** — Using Ordering::Relaxed for AtomicBool in signal handler causes main thread to never see flag change on ARM architectures. Prevention: Use Ordering::SeqCst for both store and load, or Release/Acquire pair. signal_hook::flag::register() handles this correctly.

## Implications for Roadmap

Based on research, suggested phase structure follows dependency order and minimizes risk:

### Phase 1: Signal Infrastructure Foundation
**Rationale:** Signal handling is foundational for all cleanup operations. Must get this right before adding cleanup logic. Existing capture.rs has correct pattern but needs refinement.

**Delivers:**
- Robust signal handling in main viewing mode (not just capture mode)
- Flag pattern with Arc<AtomicBool> using correct memory ordering
- Double Ctrl+C support (first graceful, second force quit)
- Fix capture.rs to not call process::exit() from handler

**Addresses:**
- Critical pitfall 1 (async-signal-unsafe operations)
- Critical pitfall 2 (race conditions)
- Moderate pitfall 5 (memory ordering)
- Moderate pitfall 8 (double Ctrl+C)

**Stack elements:** signal-hook 0.4.3 upgrade

### Phase 2: Config Foundation
**Rationale:** Configuration loading is independent of signal handling and needed before any config-dependent features. Can be implemented and tested without disrupting existing functionality.

**Delivers:**
- Config struct with Option<T> fields for all settings
- ConfigLoader with YAML parsing via serde-saphyr
- Hierarchical precedence merging (defaults, global, project, CLI)
- Project root discovery with filesystem boundary checks
- Graceful degradation when config missing or unreadable

**Addresses:**
- Table stakes: config auto-discovery, hierarchical precedence, YAML support
- Critical pitfall 3 (infinite loop in discovery)
- Moderate pitfall 6 (deprecated YAML libraries)
- Moderate pitfall 7 (permission issues)

**Stack elements:** serde-saphyr 0.0, dirs 5.0 (existing)

**Avoids:** Integration with main.rs event loop until next phase

### Phase 3: Config Integration
**Rationale:** With config foundation stable, integrate into startup flow. This affects main.rs initialization but not the event loop itself.

**Delivers:**
- Config loading in main.rs between arg parsing and mode detection
- TabState construction uses config values
- Named source definitions loaded from config
- Environment variable overrides (LAZYTAIL_* prefix)

**Addresses:**
- Table stakes: backwards compatibility, clear error messages
- Differentiators: named source definitions

**Implements:** Architecture component integration (Config feeds into App/TabState construction)

**Uses:** All Phase 2 components plus existing Args parsing

### Phase 4: Project-Local Streams
**Rationale:** With config working, extend to project-local directories. This is where signal handling meets config (cleanup must know if marker is project or global).

**Delivers:**
- .lazytail/ directory creation with correct permissions
- Project-local stream storage
- source.rs extensions (project_data_dir, project_sources_dir)
- capture.rs project awareness (writes to project or global based on context)
- Discovery mode shows project-local sources

**Addresses:**
- Differentiators: project-local streams, separation from global
- Critical pitfall 4 (stale marker detection expanded to project scope)
- Moderate pitfall 7 (permissions with mode 0700)

**Coordinates:** Signal handling (Phase 1) with project-local cleanup

### Phase 5: Config Commands & Polish
**Rationale:** With core functionality working, add developer experience features. These are independent commands that don't affect runtime behavior.

**Delivers:**
- `lazytail init` — generate starter config with comments
- `lazytail config validate` — parse and report errors
- `lazytail config show` — display effective merged config
- Local override file support (lazytail.local.yaml)
- .gitignore recommendation on first .lazytail/ creation

**Addresses:**
- Differentiators: config validation, introspection, init command, local overrides
- Minor pitfall 10 (precedence confusion) via show command
- Minor pitfall 11 (.gitignore warning)

**Stack elements:** No new dependencies, uses existing Config/ConfigLoader

### Phase Ordering Rationale

- **Signal infrastructure first** because all cleanup operations depend on it. Getting async-signal-safety wrong causes hard-to-debug issues.
- **Config foundation before integration** allows thorough testing of discovery and parsing without touching the event loop.
- **Config integration before project-local** because project-local streams need config to know where the project root is.
- **Project-local before commands** because commands need stable runtime behavior to introspect.
- **Commands last** because they're independent, optional, and don't block adoption.

This ordering follows architectural dependencies discovered in research and avoids the critical pitfalls identified in PITFALLS.md.

### Research Flags

**Phases with standard patterns (skip research-phase):**
- **Phase 1:** Signal handling patterns well-documented in signal-hook docs and Rust CLI Book
- **Phase 2:** Config file patterns established by Cargo, ripgrep, bat
- **Phase 3:** Integration follows existing main.rs flow, documented in codebase
- **Phase 5:** Config commands are simple wrappers around existing ConfigLoader

**Phases likely needing deeper research during planning:**
- **Phase 4:** Project-local streams may need research on permission models and cleanup edge cases if implementation encounters filesystem-specific issues. However, strong foundation from existing source.rs reduces this risk.

## Confidence Assessment

| Area | Confidence | Notes |
|------|------------|-------|
| Stack | HIGH | serde-saphyr actively maintained (Feb 2026), signal-hook widely used, YAML vs TOML decision well-reasoned |
| Features | HIGH | Verified against official documentation from Cargo, Git, Docker Compose, ESLint migration guide |
| Architecture | HIGH | Based on existing codebase analysis plus ecosystem patterns from Rust CLI Book and real-world tools |
| Pitfalls | HIGH | Critical pitfalls verified with official docs (signal-hook, std::sync::atomic), existing code already implements some mitigations |

**Overall confidence:** HIGH

### Gaps to Address

- **Windows signal handling:** signal-hook works on Windows but lacks true SIGTERM. If cross-platform support matters, test on Windows. Current scope appears Unix-focused based on existing code.

- **YAML vs TOML final decision:** Research recommends YAML (better for nested structures, multi-line strings). If stakeholder prefers TOML, implementation should prototype both before committing. TOML is mature fallback (toml crate).

- **Config write-back:** Neither serde-saphyr nor alternatives support modifying existing YAML while preserving formatting/comments. If config generation/modification needed beyond init command, may need yaml-edit or manual string manipulation approaches.

- **Shared project directories:** Permission model (mode 0700) prevents other users reading .lazytail/. If multi-user project directories are use case, may need different approach (environment variable to override directory location, or XDG_RUNTIME_DIR for truly sensitive data).

All gaps are minor and can be addressed during implementation if they become relevant.

## Sources

### Primary (HIGH confidence)
- [signal-hook docs.rs v0.4.3](https://docs.rs/signal-hook) — async-signal-safety, flag pattern, memory ordering
- [Rust CLI Book: Signals](https://rust-cli.github.io/book/in-depth/signals.html) — best practices, official patterns
- [serde-saphyr lib.rs v0.0.17](https://lib.rs/crates/serde-saphyr) — features, version, adoption
- [Cargo Configuration Reference](https://doc.rust-lang.org/cargo/reference/config.html) — hierarchical config, precedence
- [std::sync::atomic::Ordering](https://doc.rust-lang.org/std/sync/atomic/enum.Ordering.html) — memory ordering semantics
- [XDG Base Directory Spec](https://specifications.freedesktop.org/basedir/latest/) — standard config locations
- [Git Config Documentation](https://git-scm.com/docs/git-config) — three-tier config model

### Secondary (MEDIUM confidence)
- [serde-saphyr GitHub](https://github.com/bourumir-wyngs/serde-saphyr) — panic-free claims, test coverage
- [Rain's Rust CLI Recommendations](https://rust-cli-recommendations.sunshowers.io/configuration.html) — directory conventions
- [ESLint Flat Config Migration](https://eslint.org/docs/latest/use/configure/migration-guide) — why cascading deprecated
- [Docker Compose Documentation](https://docs.docker.com/compose/intro/compose-application-model/) — file discovery patterns
- [Cargo umask security advisory](https://github.com/rust-lang/cargo/security/advisories/GHSA-j3xp-wfr4-hx87) — permission pitfalls
- [config-rs lib.rs v0.15.19](https://lib.rs/crates/config) — layered config patterns (not chosen)

### Tertiary (LOW confidence)
- [uv config discovery issue](https://github.com/astral-sh/uv/issues/7351) — filesystem boundary discussion
- Community forum discussions on YAML ecosystem state — relative adoption numbers may shift

---
*Research completed: 2026-02-03*
*Ready for roadmap: yes*
