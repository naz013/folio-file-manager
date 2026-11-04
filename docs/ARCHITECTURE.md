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
- Recursive search (`search.rs`) walks a folder on a worker thread, never following links, and streams matches over a Tauri `Channel` in batches of up to 64; it stops on cancel and at 5,000 matches. The list shows matches instead of the folder's items, updated in 100 ms steps.

### Operations engine

- All mutating operations go through a single queue in Rust (`ops/`), never directly from the UI.
- Zip (`ops/archive.rs`) packs files and folders into a new `.zip` and unpacks one into a new folder named after it. Both stream through a temporary `.folio-partial` name and rename when complete, report bytes and files to the queue, stop on cancel, never follow or store links, and never write outside the target folder (entry names that climb out are skipped and counted). Deflate and deflate64. Stamps are the local wall-clock time, as zip keeps them. An unpack is refused up front if it won't fit in the free space, and files inherit the archive's Mark-of-the-Web on Windows. A failed unpack removes only the files and folders it made. Cloud-only files are refused, not downloaded. Undo trashes what was made, if it is unchanged and what it was made from still exists (refused for a folder of over 200,000 entries).
- Each operation writes an **undo journal** entry to SQLite before it executes.
- Deletes use the OS trash (`trash` crate). Permanent delete requires an explicit
  Shift+Delete-style confirmation. See [ADR 0003](decisions/0003-trash-first.md).
- Conflicts pause the operation and emit an event; the UI answers with a resolution.
- Links are recreated, never followed. After a copy or move, a link that led into the tree
  points at the same place in the new tree. A relative link that stays inside the tree keeps
  its text. A relative link that leaves the tree is rewritten to an absolute link to the place
  it named, because the same text would name something else from the new folder; a copy or
  rename into a folder beside the original keeps the text, as the folders above are the same.
  So does a link to another item of the same batch (folders copied or moved together stay side
  by side). Undoing a move does not turn a rewritten link back into its relative text.
  A link that can't be rewritten is reported (`not_repointed`). `..` is resolved by text, which
  is wrong only when a folder along the target is itself a link.

### Tags

SQLite index keyed by a stable file identity (NTFS file ID + volume serial on Windows,
inode + device on Unix), with path as a fallback for re-linking.
See [ADR 0002](decisions/0002-tag-storage.md).

Export and import (`tags.rs`, `Backup`): a file identity means nothing on another computer, so a tags file names files by path (plus the tag names and colours, and the home folder, so paths below it follow to the new computer's home). Reading one in only adds: missing tags are created, and each file found at its place gets its tags; nothing is removed, so it can be read twice or into an index with tags of its own. Files not found are counted.

Smart folders (`smart.rs`) are saved searches kept in the same SQLite file as the tags. A smart folder stores its criteria (tags that must all be present, part of a name or a `*`/`?` pattern, changed within N days, an optional folder), not its results: opening one works the answer out again, so it is never stale. With tags it asks the tag index, then keeps the items that are still where they were tagged and fit the rest. With no tag it walks its folder with the search code, which drops items older than the window before counting them against its cap, and gives up (and stops the walk) after a minute. A tag it names that was deleted makes it fail with a message, not widen. The window shows the result like a tag view and re-reads it every 15 seconds (later after a slow one), when the window comes back to the front, and after any change it makes itself.

### Disk scanning

- Parallel walk (`jwalk`) in a background task, results cached in SQLite per volume.
- Incremental refresh from `notify` events; full rescan on demand.
- Fast scan (Windows, NTFS, opt-in; [ADR 0004](decisions/0004-elevated-mft-scan.md)): `mft/` reads the master file table instead of walking. Folio starts itself again as administrator through Windows' own prompt (`--mft-helper`), the helper reads the raw volume and sends folder totals and big files over a private named pipe, and `client.rs` turns them into the same tree the walker builds. Declining, a non-NTFS drive or an unreadable table falls back to the walk, with a note. `record.rs` and `volume.rs` are platform-independent and tested on made-up images; the ignored test `real_drive_gives_the_sizes_of_files_just_made` checks a real drive from an administrator shell.
- Cleanup detectors (duplicates, large/old, downloads, caches) run on top of the cache.
- Temp files and caches (`cleanup_temp.rs`) are the one detector that reads inside `AppData`, which the protected-path rules otherwise keep out of reach, so it has its own rule: a short fixed list of named places, written in the code and never read from environment variables (a `TEMP` that points at Documents changes nothing). On Windows: the `Temp` folder under Local AppData (taken from the system, not the variable), crash dumps, error reports, the `Cache`, `Code Cache` and `GPUCache` folders of each Chrome, Edge and Brave profile, Firefox `cache2`, and the npm, pip and NuGet caches. On macOS and Linux: a few named cache folders (pip, Yarn, thumbnails, shader, font and Go build caches, Chrome and Firefox on macOS), not the whole `Library/Caches` or `~/.cache`. Each has a minimum age. A file is listed, and moved, only if it is a plain file strictly inside one of those places once its links are resolved, is not cloud-only, is not on a path of 260 characters or more (the Recycle Bin of Windows may not take it), is not something a person may have saved there (Outlook attachments, folders Explorer unzips into, AutoCAD autosaves), and has been left alone for that place's age. The window's list is never trusted: every item is checked again from disk when the move is asked for and when its job runs, and what is moved is the resolved path.

### Plugins

- A plugin is a folder (`plugin.json` and web files) under the app data `plugins` folder. Each runs in a `sandbox="allow-scripts"` iframe, served by the `plugin://` protocol (`plugins.rs`) from its own folder only, with a policy that blocks the network. The frame's page is given as `srcdoc` (never a link to the protocol's address, which Tauri would trust like Folio's own page); the app page's `frame-src about:` keeps frames at that.
- The page talks to Folio through `postMessage` (`lib/plugins.ts`: manifest, permissions, matching, and the bridge). Folio answers only its own frame and only for permissions the person granted in Settings; there is no write API (ADR 0005). Authoring guide: [PLUGINS.md](PLUGINS.md).

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
