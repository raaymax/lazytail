# `/nibble` — Batch Implementation Orchestrator

## Context

You have lists of features to implement (e.g., web viewer parity items in ROADMAP2.md). Today, each item requires manually: implementing, testing, committing, creating a PR, fixing CI, and merging. You want to automate this by spawning autonomous agents — each handles one task end-to-end with a full pr:full-like workflow (implement → self-review → fix → test → PR → CI → merge), creating individual PRs that all merge into a single integration branch. A final PR rolls everything into master.

**Key constraint:** Skills can't nest — an agent spawned via `Task` can't call `/pr:fast` or `/commit`. Solution: inline the full workflow directly in the agent's prompt.

**Key decisions:**
- Agents handle the **full pr:full-like flow** autonomously (implement, self-review, fix, test, commit, PR, CI, merge)
- Agents handle **their own merge conflicts** — if push/merge fails, agent merges base into their branch, resolves conflicts, and retries
- Parallel mode supported — agents work concurrently, each is fully self-sufficient
- Self-review step included in every agent's workflow

## Architecture

```
You (running /nibble)
  │
  ├─ Phase 1: Parse task list file, confirm with user
  ├─ Phase 2: Create integration branch (nibble/master-integration)
  │
  ├─ Phase 3: Spawn agents (sequential or parallel batch)
  │    └─ Each Agent (in isolated worktree):
  │         1. Fetch + branch from integration branch
  │         2. Read codebase, implement feature
  │         3. cargo fmt + clippy + test (fix until green)
  │         4. Self-review: re-read own changes, find bugs/quality issues
  │         5. Fix review findings, re-run checks
  │         6. Commit (conventional commits, no AI attribution)
  │         7. Push, create PR → integration branch
  │         8. Monitor CI, fix failures (up to 3x)
  │         9. Merge PR (squash) — if conflict: merge base, resolve, re-push, retry
  │        10. Return structured summary
  │
  ├─ Phase 4: Create final PR (integration → master)
  │    Wait for CI, fix if needed, ask user to merge
  │
  └─ Clean up branches and worktrees
```

### Why agents handle their own conflicts

In parallel mode, multiple agents may try to merge into the integration branch simultaneously. When one succeeds first, others get conflicts. Rather than requiring leader intervention, each agent is instructed to:
1. `git fetch origin && git merge origin/{integration_branch}`
2. Resolve conflicts intelligently (their changes take priority for their files)
3. Re-run checks to ensure nothing broke
4. Push and retry merge

This makes agents fully autonomous — the leader just monitors progress.

### CI gap

`.github/workflows/ci.yml` only triggers on PRs targeting `master`. Sub-PRs target the integration branch, so CI won't run on them. Local checks (`cargo fmt/clippy/test`) are the primary gate per agent. The final PR to master triggers full CI.

## What to create

### File: `~/.claude/commands/nibble.md`

Single global skill file following the same structure as `pr/fast.md` and `pr/team.md`.

### Skill structure:

```
---
name: nibble
description: "Batch implementation - spawn agents to implement a task list, each creating a reviewed PR against an integration branch"
allowed-tools: [Read, Write, Edit, Bash, Grep, Glob, Task, AskUserQuestion, Skill, TaskCreate, TaskUpdate, TaskList]
---
<objective> 4 phases overview </objective>
<critical_rules> state management, failure handling </critical_rules>
<process>
  <step name="init"> resume support </step>
  <step name="phase-1-parse"> read file, parse tasks, confirm </step>
  <step name="phase-2-setup"> create integration branch, STATE.json </step>
  <step name="phase-3-execute"> spawn agents, monitor, handle results </step>
  <step name="phase-4-final-pr"> create PR to master, CI, merge </step>
</process>
<agent_prompt_template> the full inlined workflow (the critical piece) </agent_prompt_template>
<state_schema> STATE.json structure </state_schema>
<recovery> resume, reset, cleanup instructions </recovery>
```

### Phase 1 — Parse tasks

- Read file from `$ARGUMENTS` (supports `- [ ]` checkboxes, numbered lists, dash lists)
- If file has sections (like ROADMAP2.md), let user pick which section(s)
- Build ordered task list with id, title, description, slug
- Ask user to confirm list and choose mode (sequential / parallel with batch size N)

### Phase 2 — Setup

- Create `nibble/{base}-integration` branch from current branch, push to origin
- Initialize `.planning/nibble/STATE.json` with all tasks as pending
- Create TaskCreate entries for visibility

### Phase 3 — Execute

**Sequential mode (default):**
- For each pending task:
  - Spawn agent: `Task(subagent_type: "general-purpose", isolation: "worktree", mode: "bypassPermissions", max_turns: 75)`
  - Agent prompt filled from template (task description, integration branch, conventions)
  - Wait for completion
  - On success: update STATE.json, log result
  - On failure: ask user → retry (max 3) / skip / abort

**Parallel mode (opt-in, `--parallel N`):**
- Spawn N agents simultaneously, each with `run_in_background: true, isolation: "worktree"`
- Each agent runs the full workflow independently including merge
  - If merge conflicts: agent fetches latest integration branch, merges into their branch, resolves, re-runs checks, pushes, retries
- Leader polls `TaskOutput` to monitor progress
- After batch completes, process results (update STATE, handle failures)
- Next batch starts from updated integration branch

### Phase 4 — Final PR

- `gh pr create --base {base} --head {integration_branch}` with summary table of all sub-PRs
- Wait for CI (this actually triggers CI since it targets master)
- Fix CI failures if needed (up to 3 attempts)
- Ask user to confirm merge
- Clean up integration branch

## Agent prompt template (the critical piece)

Each agent gets a detailed workflow prompt with these steps:

1. **Setup** — `git fetch origin && git checkout -b {feature_branch} origin/{integration_branch}`
2. **Understand** — Read CLAUDE.md, explore codebase with Grep/Glob/Read, understand related code
3. **Implement** — Make minimal focused changes for the single task
4. **Verify** — `cargo fmt && cargo clippy -- -D warnings && cargo test`, fix until all pass
5. **Self-review** — Re-read all changed files, look for: bugs, edge cases, missing error handling, test coverage gaps, convention violations. Fix any issues found.
6. **Re-verify** — Run checks again after review fixes
7. **Commit** — Stage specific files, conventional commit (`feat(web): add ANSI color rendering`), no AI attribution
8. **Push + PR** — `git push -u origin HEAD && gh pr create --base {integration_branch}` with summary body
9. **CI + fix** — `gh pr checks --watch`, fix up to 3x. If no CI runs (expected for non-master target), treat as pass.
10. **Merge** — `gh pr merge --squash --delete-branch`. **If merge fails (conflict):**
    - `git fetch origin && git merge origin/{integration_branch}`
    - Resolve conflicts (keep own changes for own files, accept theirs for unrelated files)
    - Re-run all checks
    - `git push`
    - Retry merge (up to 3 attempts)
11. **Report** — Return `STATUS: SUCCESS/FAILED`, PR number, summary, files changed

The prompt also includes:
- Project conventions from CLAUDE.md (build commands, commit format, no AI mention)
- Explicit rules: don't call slash commands, don't modify unrelated files, don't commit .planning/

## State schema

```json
{
  "base_branch": "master",
  "integration_branch": "nibble/master-integration",
  "mode": "sequential|parallel",
  "batch_size": 1,
  "phases": {
    "1_parse": "completed|in_progress|pending",
    "2_setup": "...",
    "3_execute": "...",
    "4_final_pr": "..."
  },
  "tasks": [
    {
      "id": 1,
      "title": "ANSI color rendering",
      "slug": "ansi-color-rendering",
      "status": "pending|in_progress|completed|failed|skipped",
      "branch": "nibble/1-ansi-color-rendering",
      "pr_number": 47,
      "pr_url": "...",
      "attempts": 1,
      "summary": "Added ansi_to_html conversion in web SPA"
    }
  ],
  "final_pr": { "number": null, "status": "pending" }
}
```

## Existing patterns to reuse

| Pattern | Source file | How we use it |
|---------|------------|---------------|
| State management + resume | `~/.claude/commands/pr/fast.md` | Same `.planning/` dir, STATE.json, phase tracking |
| Progress banners | `~/.claude/commands/pr/fast.md` | `═══ PHASE X: NAME ═══` format |
| Task tracking | `~/.claude/commands/pr/fast.md` | TaskCreate/TaskUpdate for visibility |
| Agent spawning with isolation | Task tool's `isolation: "worktree"` | Built-in worktree per agent |
| CI fix loop | `~/.claude/commands/pr/ci-fix.md` | Steps inlined in agent prompt |
| Conventional commits | `~/.claude/commands/commit.md` | Rules inlined in agent prompt |
| Parallel agent monitoring | `~/.claude/commands/pr/team.md` | `run_in_background` + `TaskOutput` polling |

## Verification

1. Create a test file with 2-3 trivial tasks (e.g., "add comment to src/web/mod.rs")
2. Run `/nibble test-tasks.md` in sequential mode
3. Verify: integration branch created, agents complete, PRs merged, final PR created
4. Test resume: interrupt mid-run, re-run, confirm it picks up correctly
5. Test failure: include an impossible task, verify skip/retry/abort works
6. Test parallel: run with `--parallel 2` on independent tasks, verify conflict resolution
