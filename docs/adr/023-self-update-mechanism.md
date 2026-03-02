# ADR-023: Self-Update Mechanism

## Status

Accepted

## Context

LazyTail distributes pre-built binaries via GitHub releases. Users who installed via the install script (rather than a package manager) had no built-in way to check for or install updates. The update mechanism needed to:

- Check for new releases without disrupting the user's workflow
- Respect users who installed via package managers (pacman, dpkg, brew)
- Avoid network requests on every launch
- Be opt-out for users who don't want update checks
- Not bloat the binary for users who don't need it

## Decision

Implement self-update as a **feature-gated module** (`self-update` feature flag), disabled by default in development builds and enabled in release builds.

**Update check strategy:**
- Background check on startup: queries the GitHub releases API for the latest version
- **24-hour cache**: results are cached in `~/.config/lazytail/update-cache.json` to avoid repeated network requests
- If a newer version is available, a non-intrusive notice is shown
- Users can disable checks via the `update_check` config field in `lazytail.yaml`

**Update installation (`lazytail update`):**
- Downloads the appropriate binary for the current OS/architecture from GitHub releases
- Replaces the running binary with the downloaded one
- `--check` flag: only check for updates without installing (exit code 0 = up-to-date, 1 = available)

**Package manager detection:**
- Before offering self-update, checks if the binary was installed via a package manager (pacman, dpkg, brew) by inspecting package databases
- If a package manager owns the binary, advises the user to update through their package manager instead
- Falls back to path-based detection if no package manager is found

**Feature gating:**
- The entire `update/` module and CLI flags (`update` subcommand, `--no-update-check`) are behind `#[cfg(feature = "self-update")]`
- This keeps the binary small and dependency-free for users who don't need it
- The `self_update` crate is only compiled when the feature is enabled

## Consequences

**Benefits:**
- Users get notified of updates without manual checking
- 24-hour cache prevents excessive API calls
- Package manager awareness prevents conflicts with system package management
- Feature gating keeps the default binary lean

**Trade-offs:**
- Feature-gated code paths increase conditional compilation complexity
- GitHub API rate limits could affect users behind shared IPs (mitigated by caching)
- Binary replacement is Unix-specific (not tested on Windows)
- Config-based opt-out (`update_check` field) requires users to create a config file to disable checks
