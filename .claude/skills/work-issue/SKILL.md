---
name: work-issue
description: Take a Folio GitHub issue from the Folio Roadmap project to a ready-to-ship change, delegating tests and reviews to the project's specialist agents by area label. Use when asked to work on, implement or pick up an issue.
argument-hint: <issue-number>
---

Work issue #$ARGUMENTS following `docs/AGENTS.md`. You are the lead: you own the branch,
the project status, commits and the PR. Specialists only report back.

## 1. Read

```
gh issue view $ARGUMENTS --json number,title,body,labels,milestone,state
gh api repos/naz013/folio-file-manager/issues/$ARGUMENTS/dependencies/blocked_by --jq '.[] | {number,state}'
```

- If any blocker is open, stop and say which.
- Extract the acceptance criteria checkboxes; they are the contract for this issue.
- Read the docs the issue links (PRODUCT, ROADMAP) only where the criteria need them.

## 2. Plan

- Pick specialists from the routing table in `docs/AGENTS.md` by the `area:` labels.
  `reviewer` always runs.
- For a `spike`, skip specialists: time-box it and end with an ADR draft in
  `docs/decisions/`.
- Move the project item to In Progress:

```
ITEM=$(gh project item-list 3 --owner naz013 --format json --limit 200 --jq ".items[] | select(.content.number==$ARGUMENTS) | .id")
gh project item-edit --project-id PVT_kwHOABgows4Bpgn8 --id "$ITEM" --field-id PVTSSF_lAHOABgows4Bpgn8zhn79BA --single-select-option-id 47fc9ee4
```

## 3. Build

- Branch `feature/<milestone number>-<slug>` from `main` unless already on the issue's branch.
- Implement the feature yourself. Once the interfaces exist, brief `test-writer` with the
  issue number, the criteria to cover and the module paths; it can run while you finish.

## 4. Review

- Brief the routed specialists in parallel (they only read). Each brief: issue number,
  criteria in scope, files changed.
- Fix what they find, then send the fixes to the same agent with `SendMessage` instead
  of starting a new one.

## 5. Report

Give a table of every acceptance criterion: met / not met / not verified, with evidence
(test name, file:line or measurement). A specialist that failed leaves its criteria as
not verified — say so; don't drop them.

## 6. Ship — only on the user's go-ahead

- No PRs, and never push: the user pushes `main` themselves.
- Commit on the feature branch with `Closes #$ARGUMENTS` in the message.
- Rebase onto `main` if it moved, re-run `cargo test` and clippy, then fast-forward
  `main` (`git fetch . <branch>:main` works without checking it out).
- Delete the merged branch (and its worktree, if you made one).
- Leave the project status to the push: GitHub closes the issue from the commit
  message and the project moves closed issues to Done.
