# ADR 0003 — Trash-first deletes and safe cleanup

- **Status:** Accepted
- **Date:** 2026-10-30

## Context

Trust is the main weakness of existing cleanup tools, and data loss is the worst failure a
file manager can have.

## Decision

1. Every delete — manual or from cleanup — moves items to the OS Recycle Bin / Trash.
2. Permanent deletion is a separate, explicitly confirmed action and is never used by cleanup.
3. Every mutating file operation writes an undo-journal entry before executing.
4. Cleanup only produces **suggestions**. Nothing is removed without a review step, and the
   primary button states the exact outcome ("Move 9.9 GB to Recycle Bin").
5. Cleanup never touches a protected-path deny list (OS directories, Program Files, app
   bundles, user profile roots, cloud-only placeholders).
6. Recycle Bin emptying and empty-folder removal are unchecked by default.

## Consequences

- Freed space only appears after the user empties the Recycle Bin; the UI explains this.
- The operations engine needs a journal and integration tests before 0.2.
