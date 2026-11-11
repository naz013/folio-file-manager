# ADR 0006 — Thumbnails drawn from the file's content by a plugin

- **Status:** Proposed (spike for issue #121; the decision is the owner's)
- **Date:** 2026-11-11

## Context

A plugin can give a kind of file a thumbnail today (`thumbnails` in `plugin.json`), but it is one
static picture for every file of that kind. #121 asks whether a plugin could instead _draw_ the
thumbnail from the file: the first rows of a CSV, the heading of a Markdown file, the shape of a
JSON document.

Two things already exist and shape the answer:

- Thumbnails are served by the `thumb://` protocol (`thumbs.rs`). The core keeps drawn pictures in
  a 64 MB cache keyed by path, size, modified time and length, and draws up to 32 at once with a
  5 second limit; the newest request goes first. A failure is a 404 and the tile shows the plain
  icon.
- A plugin page runs in a `sandbox="allow-scripts"` frame and reads files only through the bridge,
  which limits `read`, `readRange` and `readBytes` to 2 in flight and 20 per 10 seconds, and only for
  files Folio handed the plugin in the last minute (`createBridge` in `plugins.ts`).

## Options

**A. A hidden plugin frame draws to a canvas and returns a PNG.** One frame per plugin stays alive
while the plugin is on; Folio sends it `{event: "thumbnail", id, path, width, height}`, the page
reads the start of the file with `read`, draws with `OffscreenCanvas` and answers with the PNG bytes.
The plugin keeps full control of what the picture shows and new formats need no Folio release.

**B. A core-side renderer.** Rust reads the start of the file and draws it (a table for CSV, a
heading for Markdown). No frame, no bridge limits, no new way for plugin code to see files, and the
picture goes straight into the existing `thumb://` cache. But only formats Folio itself ships can be
drawn: it is not a plugin feature, and each format is Rust code to write and keep.

## What was measured

The prototype is in [`examples/thumbnail-prototype`](../../examples/thumbnail-prototype): a CSV
drawing page (`thumb-csv.js`, the page a bundled CSV viewer would ship) and a harness
(`harness.html`, served with `serve.py`) that plays Folio's part: it runs the page in a sandboxed
frame, answers its `read` calls, validates every answer and times it. Numbers are from one run, on an
8-core Windows 11 machine, in the Chromium of the Claude desktop app's browser pane (Chrome 156),
112 x 144 pictures (56 x 72 at 2x, the size of Folio's page icon), 40-row synthetic CSVs held in memory.

| Case                                                                 | Result                                                                                                   |
| -------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| One persistent frame, files one at a time                            | median 5.9 ms, p95 15 ms, 7.3 ms per file                                                                |
| One persistent frame, 5 requests in flight                           | 31.6 ms per file: 4x _slower_, p95 about 800 ms                                                          |
| A new frame for every file                                           | median 140 ms, p95 419 ms: about 20x dearer                                                              |
| Size of the PNG                                                      | about 5 KB                                                                                               |
| Page that never answers                                              | refused by the 3 s limit, after 3.1 s                                                                    |
| Page answers with text, a non-PNG, a PNG of the wrong size, or 50 MB | all refused by the header check (see below)                                                              |
| Page that spins for 6 s                                              | Folio's window kept running: worst gap between timers 38 ms (30 ms idle). Removing the frame is the stop |

Reading the numbers:

- **Drawing is cheap; the bridge's limits are the cost.** At 7 ms a file, 300 tiles are about 2
  seconds of drawing. But `read` is limited to 20 calls per 10 seconds, so the same 300 tiles would
  take about 150 seconds through the bridge as it is. Thumbnails need their own budget, which means
  a new, separate permission or a different limit for the thumbnail path.
- **One frame, one at a time.** Parallel requests to a single frame only queue inside it. Concurrency
  across plugins (one frame each) is the only parallelism; within a plugin keep a queue of 1 to 2.
- **A frame per file is too dear** for a grid; keep the frame alive while the plugin is on and tear
  it down after it has been idle (Folio already creates frames on first use).
- **A 50 MB answer is only refused after it has arrived** (about 600 ms): the structured clone
  happens first. A host cannot stop that, only bound how often it can happen.

Not measured, said plainly: Option B was not built. Its cost is expected to be well under Option A's
(no frame, no message hop), but that is reasoning, not a number. The prototype's reads were served
from memory, so disk and cloud-file cost is not in the figures. It ran in Chrome 156 in the app's
browser pane, not in Folio's own WebView2: that a spinning page stays out of the main thread's way
was seen there and must be rechecked in Folio before anyone relies on it.

## How it would fit the sandbox (Option A)

- **What the page may read.** Only the files Folio names in the `thumbnail` event, through the
  same `read` call and permission as a preview. A thumbnail page runs without a click for every file
  scrolled into view, so it carries the preview rules: it may not claim the kinds that hold secrets
  (`SENSITIVE` in `plugins.ts`), and a file only in the cloud is never read (the thumbnail
  protocol already refuses those). The 4 KB the prototype reads is enough for a first-rows picture;
  a plugin that asks for more is capped by the bridge.
- **How the image is checked before it is shown.** In the core, not the page: bytes only (never a
  string, URL or SVG), at most 32 KB, the PNG signature, and an `IHDR` that is exactly the width and
  height asked for. Better still, decode and re-encode it in Rust so a crafted PNG never reaches the
  webview's decoder as the plugin made it. The prototype checks the first three in JavaScript
  (`checkPng`); the re-encode is not prototyped.
- **Where it is kept.** Hand the checked bytes to the core and let them be served as `thumb://`, so
  the tile code (`Thumbnail.svelte`) and the 64 MB path/size/modified-time cache stay as they are,
  and a thumbnail survives the frame being torn down. Folio's page has no content-security policy
  today (`csp: null`), so a blob URL would also load, but keeping one source for tiles is simpler.
- **When it fails.** Limit of 3 seconds per file, then remove and recreate the frame (a page that
  spun in the prototype did not freeze the window, but only removing the frame stops it), answer 404 and
  show the static `thumb.svg` or the plain icon, which is exactly what happens today. Three failures in
  a row turn the plugin's drawn thumbnails off for the session.
- **Concurrency and cost limits.** One request at a time per plugin frame; all plugins together at
  most 2 frames busy; a separate read budget for thumbnails (the `read` one is too small, see
  above); results cached by path, size and modified time and length, as in `thumbs.rs`.
- **What it does not fix.** Everything in ADR 0005 about the sandbox still holds: a DNS lookup from a
  page may leak a few bytes, and thumbnails widen _when_ that could happen from "files you open" to
  "files you scroll past".

## Decision (proposed)

**Do not build drawn thumbnails as a plugin feature now.** The drawing is cheap and the sandbox can
carry it, but:

1. It changes what a plugin with `read` can see without a click: from the one file in Quick Look to
   every file in view. That is the case ADR 0005 limited previews for, and it would need a new
   permission, a per-plugin read budget, and an audit of what can be left out.
2. The picture itself shows file content on the screen of a folder with no click: rows of a CSV of
   salaries or a Markdown draft are on show in a grid, in a screen share, in a screenshot. A person
   should choose that.
3. The visible gain is small: for CSV, Markdown and JSON, the preview is one click away and already
   good.

**If the owner wants the feature anyway,** build Option B (core-side renderer) for the bundled
formats first, behind a setting that is off by default, since it needs no new plugin ability and no
new sandbox surface, and reaches the same `thumb://` cache. Option A can follow when a third-party
plugin needs it, with the design above and its own permission.

## Consequences

- No change to `plugin.json`, the bridge or the sandbox from this spike.
- `examples/thumbnail-prototype` stays as the record of the measurements and the checks a host would
  need; it is not part of the app or its tests.
- A later build of either option should add its own ADR amendment, a Penpot design for how a drawn
  thumbnail looks next to the badge Folio draws, and a performance check against the scroll budget in
  `docs/PRODUCT.md` (`perf-auditor`).
