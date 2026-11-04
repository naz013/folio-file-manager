# Text viewer

Quick Look for text and code files, with line numbers, wrapping and search. A plugin that ships with Folio (id `text-viewer`, version 1.0.0); it is off until you turn it on. See [Plugins](../PLUGINS.md) for how plugins work.

## File types

- **Quick Look** draws `.txt`, `.log`, `.ini`, `.cfg`, `.conf`, `.yaml`, `.yml`, `.toml`, `.xml`, `.sh`, `.bat`, `.ps1`, `.py`, `.js`, `.ts`, `.rs`, `.go`, `.css`, `.sql`.
- **Page icons** are drawn for the same kinds of file.

## What it asks for

- `read` (it can read the files Folio hands it, and nothing else). It has no network access and cannot change a file.

## What it shows

- Every line as a numbered row; **Wrap** wraps long lines and **Lines** hides the numbers.
- Search (Ctrl+F) with a match-case switch (**Aa**), previous and next match (Shift+Enter goes back) and a count of matches (at most 5,000 are marked).
- More of a large file as you scroll: the first 256 KB at once, then 1 MB at a time.
- The encoding of the text, in a menu: Auto (a byte-order mark, else a guess), UTF-8, UTF-16 LE, UTF-16 BE or Windows-1252.

## Limits

- Loads at most 2 MB of a file; a note says how much of it is shown.
- A file with NUL bytes that is not UTF-16 is refused as binary (the hex viewer takes those).

## How it is built

One page (`index.html`) and the shared encoding menu (`encoding.js`). It reads with `readRange`, so a piece never cuts a character, and puts text on the page only with `textContent`.

Source: [`src-tauri/resources/plugins/text-viewer/`](../../src-tauri/resources/plugins/text-viewer/).

## Thumbnail

![Text viewer page art](../../src-tauri/resources/plugins/text-viewer/thumb.svg)

The page art is [`thumb.svg`](../../src-tauri/resources/plugins/text-viewer/thumb.svg) (56 × 72, colour `#6b7280`); Folio draws the extension badge over it. It was designed in Penpot, in Design System > Components > Thumbnails, in the row "Plugin thumbnails · bundled".
