# Folio — Working with agents

How Claude Code splits work on Folio between a **lead** (the main session) and
**specialist subagents**. Adapted from Anthropic's
[multiagent orchestration](https://platform.claude.com/docs/en/managed-agents/multiagent-orchestration)
guide; the unit of work is always a GitHub issue on the
[Folio Roadmap](https://github.com/users/naz013/projects/3) project.

## Roles

| Role | Who | Owns |
|---|---|---|
| Lead | main session | the issue, branch, project status, commits, local merge to `main`, talking to you |
| Specialist | `.claude/agents/*.md` | one scoped task; reports back, never touches GitHub or git history |

Rules taken from the guide:

- **One level deep.** Specialists don't spawn agents. Only the lead delegates.
- **Predefined over improvised.** Use the specialists below before inventing an ad-hoc
  agent; they carry the project rules and the right tools and model.
- **Isolated context, shared workspace.** A specialist sees the repo but not the lead's
  conversation, so every brief states the issue number, the acceptance criteria it covers
  and the files in scope.
- **Follow up, don't respawn.** After fixing what a reviewer found, send the fixes to the
  same reviewer (`SendMessage`) so it checks against its earlier findings.
- **Escalate the risky subset.** Anything that can lose data goes to `safety-reviewer`
  (Opus), not to the general reviewer.

## Specialists

| Agent | Model | Writes code | Use for |
|---|---|---|---|
| `reviewer` | Sonnet | no | Final review of every change against CLAUDE.md rules and the issue's criteria |
| `safety-reviewer` | Opus | no | Deletes, moves, overwrites, cleanup, undo journal — ADR 0003 |
| `test-writer` | Sonnet | tests only | Rust unit/integration tests for acceptance criteria |
| `perf-auditor` | Sonnet | no | Checks a change against the PRODUCT.md performance budgets |

## Routing by label

The issue's `area:` labels pick the specialists. `reviewer` runs on every issue.

| Label | Also call |
|---|---|
| `area: file-ops` | `test-writer`, `safety-reviewer` |
| `area: storage` | `safety-reviewer`; `perf-auditor` for scanner work |
| `area: browsing` | `perf-auditor` |
| `area: tags` | `test-writer` |
| `area: platform` | — (`reviewer` checks `unsafe` / FFI) |
| `area: ux` | — (`reviewer` checks tokens and accessibility) |
| `chore` | — |
| `spike` | none; the lead time-boxes it and ends with an ADR draft in `docs/decisions/` |

## Issue lifecycle

Run it with `/work-issue <number>`.

1. **Read** the issue: labels, milestone, acceptance criteria, `blocked-by`, sub-issues.
   Stop if a blocker is still open.
2. **Plan** which criteria the lead implements and which specialists run. Move the project
   item to **In Progress**.
3. **Build** on a `feature/<milestone>-<slug>` branch. The lead writes the feature code;
   `test-writer` covers the criteria in parallel once interfaces exist.
4. **Review** with the routed specialists. They run in parallel — they only read.
5. **Report** each acceptance criterion as met, not met, or not verified. A specialist that
   fails doesn't fail the issue; its criteria are listed as not verified.
6. **Ship** only on your go-ahead: commit with `Closes #<n>` and fast-forward `main`
   locally. No PRs; you push `main` yourself, which closes the issue and moves the
   item to **Done**.

## When to fan out

- **One issue** → the lead does it, with specialists for tests and review. No fan-out.
- **Several issues, read-only** (audit a milestone's exit criteria, triage, review open
  PRs) → one subagent per issue in parallel, then the lead combines the results.
- **Several issues, writing code** → separate sessions in their own worktrees, one per
  issue, only for issues with no `blocked-by` between them and touching different modules
  (`fs/`, `ops/`, `tags/`, `scan/`, `platform/`, UI). Never two writers on one module.
- **Parent issue with sub-issues** → the sub-issues are the phases; run them in
  dependency order and parallelize only within a phase.
- **Before any fan-out**, the lead lists the phases and how many agents each runs, then
  waits for your go-ahead. Every agent costs tokens; don't fan out what one agent can do.
