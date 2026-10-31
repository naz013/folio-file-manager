# Folio — Product Definition

> Working name. A beautiful, lightweight, cross-platform file manager.
> Windows first, then macOS and Linux. Finder-inspired, but calmer and more capable.

## Vision

A file manager that feels instant, looks the same everywhere, and helps people keep their files
organized (tags) and their disks healthy (space breakdown + safe cleanup) — without the clutter of
power-user tools.

## Target users

- **Primary:** people who live in their files every day (designers, developers, students, office
  workers) and find Windows Explorer slow, dated or cluttered.
- **Secondary:** people switching between macOS and Windows who want one consistent tool.
- **Not targeted (for now):** sysadmins needing FTP/SFTP, scripting, or two-pane "commander" workflows.

## Product principles

These are the tie-breakers for every decision.

1. **Simple by default, powerful on demand.** Advanced features live in menus, shortcuts and the
   Ctrl K palette — never in the default toolbar.
2. **Never lose data.** Deletes go to the Recycle Bin/Trash. File operations are undoable.
   Conflicts always ask.
3. **Feels instant.** Performance budgets (see below) are release blockers, not nice-to-haves.
4. **Keyboard-first, mouse-friendly.** Every action has a shortcut; drag & drop works everywhere.
5. **Consistent across OSes, native where it matters.** Same UI everywhere; OS conventions for
   shortcuts, menus, paths and trash.

## Performance budgets

| Metric                                    | Budget                  |
| ----------------------------------------- | ----------------------- |
| Open a 10,000-item folder (warm)          | < 150 ms to first paint |
| Scroll a 50,000-item folder               | 60 fps                  |
| Idle memory, Folio process                | < 80 MB                 |
| Idle memory, total private incl. WebView2 | < 150 MB                |
| Cold start                                | < 1 s                   |
| Installer size                            | < 15 MB                 |

## Feature catalog

Priorities: **P0** = required for 1.0, **P1** = shortly after 1.0, **P2** = later.

### Browsing

| Feature                                | Priority | Notes                                                                             |
| -------------------------------------- | -------- | --------------------------------------------------------------------------------- |
| Grid & list views                      | P0       | Grid with size slider; list with sortable, resizable columns                      |
| Column view (Finder-style)             | P1       | High value for Mac switchers; must not block 1.0                                  |
| Gallery view                           | P2       | Large preview + filmstrip for photo folders                                       |
| Tabs                                   | P0       | Middle-click opens in new tab; drag tab out → new window; tabs restored on launch |
| Sidebar                                | P0       | Favorites (drag to reorder/add), drives with usage bars, tags                     |
| Breadcrumb path bar                    | P0       | Click a segment to jump; chevron shows siblings; double-click to type a path      |
| Quick Look (Space)                     | P0       | Images, PDF, text/code, audio/video; Windows preview handlers later               |
| Inspector panel                        | P1       | Preview, tags, space breakdown, metadata                                          |
| Hidden files, sort, group by kind/date | P0       |                                                                                   |
| Dual pane                              | P2       | Opt-in for power users                                                            |

### File operations

| Feature                           | Priority     | Notes                                                                           |
| --------------------------------- | ------------ | ------------------------------------------------------------------------------- |
| Copy / move / rename / new folder | P0           | Background operations queue with progress, pause, cancel                        |
| Conflict dialog                   | P0           | Replace / Skip / Keep both, "apply to all", side-by-side compare                |
| Undo / redo (Ctrl Z)              | P0           | Move, rename, delete, new folder. Key differentiator vs Explorer                |
| Drag & drop inside the app        | P0           | Tabs, sidebar, folders; spring-loaded tabs                                      |
| Drag & drop to/from other apps    | P0 (Windows) | Native OLE drag source/target; must work with Explorer, browsers, mail          |
| Context menu                      | P0           | Own clean menu + "More options" opening the native shell menu (7-Zip, VS Code…) |
| Batch rename                      | P1           | Find/replace, numbering, dates, live preview                                    |
| Compress / extract zip            | P1           | Other formats later                                                             |

### Organization (tags)

| Feature                        | Priority | Notes                                                                         |
| ------------------------------ | -------- | ----------------------------------------------------------------------------- |
| 7 color tags + named tags      | P0       | Keys 1–7, context menu, inspector                                             |
| Tag filters                    | P0       | Sidebar + filter chips in the folder header                                   |
| Tag storage                    | P0       | SQLite index keyed by file ID — see [ADR 0002](decisions/0002-tag-storage.md) |
| Native tag sync                | P1       | macOS Finder tags, Linux `user.xdg.tags`; Windows is Folio-only               |
| Tag export / backup            | P1       | Protects tags when moving to a new PC                                         |
| Smart folders (saved searches) | P2       | e.g. "Tag: Work AND modified this week"                                       |

### Search & navigation

| Feature                       | Priority | Notes                                          |
| ----------------------------- | -------- | ---------------------------------------------- |
| Type-to-filter current folder | P0       |                                                |
| Ctrl K command palette        | P0       | Jump to folders, run actions, recent locations |
| Recursive search (name/size)  | P1       | Use Windows Search index when available        |
| Full-text content search      | P2       | Only via OS index, never our own indexer       |

### Storage & cleanup (differentiator)

| Feature                           | Priority | Notes                                                               |
| --------------------------------- | -------- | ------------------------------------------------------------------- |
| Space breakdown (donut + treemap) | P0       | Background scan, cached, incremental; drill-down                    |
| Folder sizes in list view         | P1       | Free from the scan cache                                            |
| Large & old files                 | P0       | Configurable thresholds                                             |
| Old downloads                     | P0       | Installers/archives older than N days                               |
| Duplicates                        | P0       | Size → partial hash → full hash; review UI chooses the copy to keep |
| Temp files & caches               | P1       | Allow-list of known-safe locations only                             |
| Empty folders, Recycle Bin        | P1       | Unchecked by default                                                |
| MFT fast scan (Windows)           | P2       | WizTree-like speed; requires admin, opt-in                          |

Cleanup safety rules — see [ADR 0003](decisions/0003-trash-first.md):
suggestions only, always a review step, always to Recycle Bin/Trash, never system-critical paths.

### Platform & polish

| Feature                               | Priority | Notes                                                            |
| ------------------------------------- | -------- | ---------------------------------------------------------------- |
| Light / dark / system theme           | P0       | Driven by design tokens                                          |
| Auto-update + signed installer        | P0       | Unsigned builds get blocked by SmartScreen                       |
| Minimal settings                      | P0       | Start location, hidden files, theme, default view                |
| Accessibility                         | P1       | Screen-reader labels, focus rings, reduced motion, high contrast |
| Network drives, OneDrive placeholders | P1       | Never hydrate cloud-only files for thumbnails                    |
| Set as default file manager           | P2       | Opt-in, off by default (fragile on Windows)                      |
| macOS & Linux builds                  | 1.1      | See [ROADMAP](ROADMAP.md)                                        |

## Explicitly out of scope

- Built-in cloud sync
- FTP / SFTP (maybe later via plugins)
- Plugin system before 2.0
- "PC booster" features: registry cleaners, RAM optimizers, startup managers

## Competitive landscape

| Product                        | Platform      | Gap Folio fills                        |
| ------------------------------ | ------------- | -------------------------------------- |
| Files (files.community)        | Windows       | Beautiful but heavy/slow; Windows only |
| OneCommander                   | Windows       | Modern but busy; Windows only          |
| Directory Opus                 | Windows       | Extremely powerful, complex, paid      |
| XYplorer                       | Windows       | Tags & power features, dated UI        |
| Double Commander               | Win/Linux/Mac | Cross-platform but old two-pane UX     |
| Finder / Marta / ForkLift      | macOS         | Mac only                               |
| WizTree / TreeSize / DaisyDisk | Various       | Space analysis only                    |
| BleachBit / CCleaner           | Various       | Cleanup only, trust issues             |

**Positioning:** the only one combining a calm Finder-like UI, cross-OS consistency, tags,
space breakdown and _safe_ cleanup in a < 15 MB app.
