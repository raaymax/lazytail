---
phase: 03-config-loading
verified: 2026-02-03T23:30:00Z
status: gaps_found
score: 7/8 must-haves verified
gaps:
  - truth: "Config parse errors show in debug source when viewer opens"
    status: failed
    reason: "Errors logged to stderr, not shown in debug source tab"
    artifacts:
      - path: "src/main.rs"
        issue: "Uses eprintln for config errors instead of debug source"
    missing:
      - "Debug source tab implementation for showing config errors in UI"
      - "Integration to add config errors as a tab instead of stderr logging"
---

# Phase 3: Config Loading Verification Report

**Phase Goal:** Application parses YAML config and merges multiple configuration sources with clear precedence

**Verified:** 2026-02-03T23:30:00Z
**Status:** gaps_found
**Re-verification:** No — initial verification

## Goal Achievement

### Observable Truths

#### Plan 03-01: Config Loading Infrastructure

| #   | Truth                                                        | Status      | Evidence                                                                                    |
| --- | ------------------------------------------------------------ | ----------- | ------------------------------------------------------------------------------------------- |
| 1   | YAML config with name and sources fields parses correctly   | ✓ VERIFIED  | types.rs has RawConfig with name/sources, 13 loader tests pass                             |
| 2   | Unknown fields cause parse errors with suggestions          | ✓ VERIFIED  | deny_unknown_fields on structs, test_load_unknown_field_error passes, jaro_winkler used    |
| 3   | Tilde paths expand to home directory                        | ✓ VERIFIED  | expand_path() function in loader.rs, test_expand_path_tilde passes                         |
| 4   | Parse errors include file path, line number, and suggestion | ✓ VERIFIED  | ConfigError::Parse has path/line/column/suggestion, format_cargo_style() formats correctly |

#### Plan 03-02: Main Integration

| #   | Truth                                                         | Status      | Evidence                                                                       |
| --- | ------------------------------------------------------------- | ----------- | ------------------------------------------------------------------------------ |
| 5   | Config sources appear in side panel under separate categories | ✓ VERIFIED  | ui/mod.rs lines 130-131 show "Project Sources" and "Global Sources" headers   |
| 6   | Sources with missing files appear grayed out                 | ✓ VERIFIED  | tab.disabled field, ui/mod.rs line 183-185 renders disabled in DarkGray       |
| 7   | Config parse errors show in debug source when viewer opens   | ✗ FAILED    | Errors logged to stderr (lines 309-311 main.rs), no debug source tab          |
| 8   | Viewer opens normally even when config has errors            | ✓ VERIFIED  | Config errors captured in vec, doesn't block app creation (lines 144-150)     |

**Score:** 7/8 truths verified

### Required Artifacts

| Artifact              | Expected                                  | Status      | Details                                                      |
| --------------------- | ----------------------------------------- | ----------- | ------------------------------------------------------------ |
| `src/config/types.rs` | Config structs with deny_unknown_fields   | ✓ VERIFIED  | 68 lines, RawConfig/RawSource have deny_unknown_fields       |
| `src/config/loader.rs`| YAML parsing with serde_saphyr            | ✓ VERIFIED  | 418 lines, load_file() uses serde_saphyr::from_str           |
| `src/config/error.rs` | ConfigError with jaro_winkler suggestions | ✓ VERIFIED  | 309 lines, find_suggestion() uses jaro_winkler with 0.8 threshold |
| `src/app.rs`          | SourceType with ProjectSource/GlobalSource| ✓ VERIFIED  | Lines 22-33, enum has 5 variants including new config types  |
| `src/main.rs`         | Config loading integration                | ✓ VERIFIED  | Lines 143-161, calls config::load() and creates tabs         |
| `src/ui/mod.rs`       | Separate sections for config sources      | ✓ VERIFIED  | Lines 130-131, distinct category names rendered             |
| `src/tab.rs`          | from_config_source() method               | ✓ VERIFIED  | Lines 281-322, creates tabs or disabled placeholders         |

### Key Link Verification

| From                    | To                      | Via                           | Status     | Details                                                   |
| ----------------------- | ----------------------- | ----------------------------- | ---------- | --------------------------------------------------------- |
| src/config/loader.rs    | src/config/types.rs     | imports Config structs        | ✓ WIRED    | Line 10: `use crate::config::types::`                     |
| src/config/loader.rs    | src/config/error.rs     | returns ConfigError           | ✓ WIRED    | Line 9 imports, line 47 returns ConfigError              |
| src/main.rs             | src/config/loader.rs    | calls load()                  | ✓ WIRED    | Line 143: `config::load(&discovery)`                      |
| src/main.rs             | src/tab.rs              | creates tabs from sources     | ✓ WIRED    | Lines 197, 205: `TabState::from_config_source()`          |
| src/ui/mod.rs           | src/app.rs              | renders SourceType variants   | ✓ WIRED    | Lines 130-131 match on SourceType enum                    |

### Requirements Coverage

| Requirement | Description                                       | Status        | Blocking Issue                                |
| ----------- | ------------------------------------------------- | ------------- | --------------------------------------------- |
| LOAD-01     | YAML format support with serde-saphyr             | ✓ SATISFIED   | serde-saphyr in Cargo.toml, loader parses YAML|
| LOAD-02     | Hierarchical precedence (project over global)     | ✓ SATISFIED   | loader.rs line 88 comment, project name takes precedence |
| LOAD-03     | Clear error messages with file/line               | ✓ SATISFIED   | ConfigError::format_cargo_style() includes location |
| OPT-01      | `name` option                                     | ✓ SATISFIED   | types.rs RawConfig has name field, used in UI |
| OPT-02      | `sources` option                                  | ✓ SATISFIED   | types.rs RawConfig has sources vec            |
| OPT-03      | `follow` option                                   | N/A           | Deferred per ROADMAP.md note                  |
| OPT-04      | `filter` option                                   | N/A           | Deferred per ROADMAP.md note                  |
| OPT-05      | `streams_dir` option                              | N/A           | Deferred per ROADMAP.md note                  |

**Note:** ROADMAP.md explicitly states OPT-03, OPT-04, OPT-05 are deferred. Phase 3 only implements name and sources.

### Anti-Patterns Found

| File              | Line | Pattern                    | Severity | Impact                                                   |
| ----------------- | ---- | -------------------------- | -------- | -------------------------------------------------------- |
| src/main.rs       | 309  | eprintln for config errors | ⚠️ WARNING | Config errors go to stderr, not visible in TUI          |

**Note:** The "placeholder" mentions in tab.rs (lines 279, 286, 326) are documentation describing intentional disabled tab behavior, not stub code. The implementation is complete.

### Human Verification Required

#### 1. Config Sources Display in Side Panel

**Test:** 
1. Create `/tmp/test-lazytail/lazytail.yaml`:
   ```yaml
   name: "Test Project"
   sources:
     - name: syslog
       path: /var/log/syslog
     - name: missing
       path: /nonexistent/file.log
   ```
2. Run `cd /tmp/test-lazytail && lazytail`
3. Check side panel for "Project Sources" section
4. Verify "syslog" source appears normal
5. Verify "missing" source appears grayed out

**Expected:** 
- "Project Sources" header visible
- "syslog" selectable in normal color
- "missing" shown in dark gray and not selectable

**Why human:** Visual appearance (colors, layout) can't be verified programmatically

#### 2. Config Error Messages Quality

**Test:**
1. Create `/tmp/test-config.yaml`:
   ```yaml
   nam: "Typo"
   sources: []
   ```
2. Create `/tmp/test-dir/lazytail.yaml` with above content
3. Run `cd /tmp/test-dir && lazytail --verbose`
4. Check error message format

**Expected:**
- Error shows file path `/tmp/test-dir/lazytail.yaml`
- Error includes line/column location
- Error suggests "did you mean `name`?"
- Cargo-style formatting with `-->` and `|` markers

**Why human:** Error message formatting quality needs human judgment

#### 3. Config Merging Behavior

**Test:**
1. Create global config at `~/.config/lazytail/config.yaml`:
   ```yaml
   name: "Global Name"
   sources:
     - name: global-log
       path: /var/log/syslog
   ```
2. Create project config `/tmp/project/lazytail.yaml`:
   ```yaml
   name: "Project Name"
   sources:
     - name: project-log
       path: /var/log/auth.log
   ```
3. Run `cd /tmp/project && lazytail --verbose`
4. Check verbose output and side panel

**Expected:**
- Verbose shows project name "Project Name" (not "Global Name")
- Side panel has "Project Sources" with "project-log"
- Side panel has "Global Sources" with "global-log"
- Both source groups visible and distinct

**Why human:** Multi-source integration behavior needs end-to-end validation

### Gaps Summary

Phase 3 goal is **mostly achieved** with one gap:

**Gap: Config Error Display (Truth 7)**

Config errors are logged to stderr instead of shown in a debug source tab within the UI. The summaries claim "Config errors logged to stderr (debug source deferred)" which matches the code, but the plan's must_haves specify "Config parse errors show in debug source when viewer opens."

**What's working:**
- Config errors are captured (lines 144-150 in main.rs)
- Errors don't block app creation (graceful degradation works)
- Errors are logged to stderr (lines 309-311 in main.rs)

**What's missing:**
- Debug source tab showing config errors in TUI
- Integration to create a special tab for error messages

**Impact:** Medium — config errors are visible but require checking terminal, not discoverable in the UI itself. Users might miss parse errors if they don't notice stderr output.

**Recommendation:** This appears to be an intentional deferral based on summary notes. If this is acceptable as Phase 5 polish work, mark truth 7 as "Partial - deferred to Phase 5" rather than "Failed." Otherwise, needs a quick plan to add debug source tab.

---

_Verified: 2026-02-03T23:30:00Z_
_Verifier: Claude (gsd-verifier)_
