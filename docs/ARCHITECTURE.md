# Folio — Architecture

## Stack

| Layer      | Choice                                                      | Why                                                                              |
| ---------- | ----------------------------------------------------------- | -------------------------------------------------------------------------------- |
| Shell      | **Tauri 2**                                                 | System webview (WebView2 / WKWebView / WebKitGTK), ~10 MB installers, low memory |
| Core       | **Rust**                                                    | Fast parallel file I/O, safe native OS integration                               |
| UI         | **SvelteKit (SPA, adapter-static) + Svelte 5 + TypeScript** | Compiled, tiny runtime, fast reactive lists                                      |
| Local data | **SQLite** (`rusqlite`, bundled)                            | Tags, favorites, scan cache, undo journal                                        |
| Styling    | Plain CSS with design tokens (CSS custom properties)        | Mirrors Penpot tokens 1:1, no framework weight                                   |

See [ADR 0001](decisions/0001-tauri-svelte-stack.md) for alternatives considered.

## High-level structure

```
┌──────────────────────── UI (SvelteKit, src/) ────────────────────────┐
│ routes/        app shell (sidebar · tabs · toolbar · views · inspector)│
│ lib/components reusable UI matching Penpot components                  │
│ lib/stores     tabs, selection, view state                             │
│ lib/api        typed wrappers around Tauri commands & events           │
│ lib/styles     tokens.css (light/dark), global.css                     │
└───────────────────────────────▲──────────────────────────────────────┘
              invoke() / events │ (JSON over IPC)
┌───────────────────────────────┴──────── Core (Rust, src-tauri/) ─────┐
│ commands/   thin Tauri command layer (validation, error mapping)      │
│ fs/         listing, metadata, watching                               │
│ ops/        operations queue: copy/move/delete, conflicts, undo log   │
│ tags/       SQLite tag index keyed by file identity                   │
│ scan/       background disk scanner + size cache (breakdown, cleanup) │
│ platform/   per-OS code: Windows shell (icons, thumbnails, context    │
│             menu, OLE drag & drop), macOS/Linux equivalents           │
└───────────────────────────────────────────────────────────────────────┘
```

Only `commands/` and `fs/` exist today; the other modules arrive with their milestones
(see [ROADMAP](ROADMAP.md)).

## Key design decisions

### File listing

- Rust reads directories and returns compact entry records (name, kind, size, modified, hidden).
- Large folders stream in chunks via events so the first screen paints immediately.
- The UI uses a virtualized list/grid; only visible rows are in the DOM.

### Operations engine

- All mutating operations go through a single queue in Rust (`ops/`), never directly from the UI.
- Each operation writes an **undo journal** entry to SQLite before it executes.
- Deletes use the OS trash (`trash` crate). Permanent delete requires an explicit
  Shift+Delete-style confirmation. See [ADR 0003](decisions/0003-trash-first.md).
- Conflicts pause the operation and emit an event; the UI answers with a resolution.

### Tags

SQLite index keyed by a stable file identity (NTFS file ID + volume serial on Windows,
inode + device on Unix), with path as a fallback for re-linking.
See [ADR 0002](decisions/0002-tag-storage.md).

### Disk scanning

- Parallel walk (`jwalk`) in a background task, results cached in SQLite per volume.
- Incremental refresh from `notify` events; full rescan on demand.
- Cleanup detectors (duplicates, large/old, downloads, caches) run on top of the cache.

### Native integration (Windows first)

Implemented in `platform/windows` with the `windows` crate:
file icons & thumbnails (`IShellItemImageFactory`), native context menu (`IContextMenu`),
OLE drag source/target, Recycle Bin. Each has a macOS/Linux counterpart in 1.1.

## Security

- Tauri capabilities restrict the webview to the commands it needs.
- No network access except the updater endpoint.
- No telemetry by default; crash reporting is opt-in.

## Repository layout

```
docs/                 product, roadmap, architecture, design, ADRs
src/                  SvelteKit frontend
src-tauri/            Rust core + Tauri config
static/               static assets served as-is
.github/workflows/    CI (type-check, build, cargo check/test/clippy)
```
