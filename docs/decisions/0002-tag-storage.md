# ADR 0002 — Tag storage

- **Status:** Accepted
- **Date:** 2026-10-30

## Context

Windows has no system-wide file tags. macOS has Finder tags (xattr
`com.apple.metadata:_kMDItemUserTags`); Linux has `user.xdg.tags` xattrs (filesystem-dependent).
Tags must survive renames and moves, and work identically on every OS.

## Options considered

| Option                                  | Verdict                                                             |
| --------------------------------------- | ------------------------------------------------------------------- |
| NTFS Alternate Data Streams             | Lost when copying to FAT/exFAT, cloud folders, zip, email — fragile |
| Sidecar files (`.folio` per folder)     | Clutters folders, breaks with moves                                 |
| Path-keyed database                     | Breaks on every rename/move                                         |
| **SQLite index keyed by file identity** | Survives rename/move within a volume; consistent on all OSes        |

## Decision

A SQLite database in the app data directory stores tags keyed by **file identity**:

- Windows: volume serial number + NTFS file ID
- macOS / Linux: device + inode

The last known path is stored too, used to re-link entries when identity changes
(e.g. after a copy to another volume or a restore from backup).

From 1.1, tags are also **mirrored to native tags** where the OS supports them
(two-way sync on macOS and Linux).

## Consequences

- Tags are local to the machine; export/import (P1) covers migration.
- Copying a file to another volume does not carry its tags (same as Finder across
  non-supporting filesystems); "move" within a volume keeps them.
- Requires a file-identity helper in `platform/` for each OS.
