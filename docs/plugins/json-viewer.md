# JSON viewer

Quick Look for JSON files: a tree you can fold and search, pretty and raw views. A plugin that ships with Folio (id `json-viewer`, version 1.0.0); it is off until you turn it on. See [Plugins](../PLUGINS.md) for how plugins work.

## File types

- **Quick Look** draws `.json`, `.geojson`, `.jsonld`, `.har`.
- **Page icons** are drawn for the same kinds of file.

## What it asks for

- `read` (it can read the files Folio hands it, and nothing else). It has no network access and cannot change a file.

## What it shows

- A **Tree** of the document to fold and unfold (**Expand all**, **Collapse all**), with the number of items in each object and array. Only the lines in view are drawn, so a 25,000-item array opens at once and a jump to a far match is one scroll. An open container shows a window of 10,000 members, moved by **Previous** and **Next** lines, and a search match moves the window to it: however far in a huge array a match is, only that many lines are made. It scrolls sideways for deep or long lines, and the keyboard works on it: the arrows move, Right and Left open and close, Enter toggles, and a screen reader hears a tree.
- **Pretty** (indented) and **Raw** (as written) text views.
- Search (Ctrl+F) through keys and values of the whole document, with previous and next match, however far the match is. The search runs in short slices ("Searching… 1,200,000 nodes"), so the page stays usable.
- **Copy path** and **Copy value** for the selected row; a match you jump to is selected, and copying takes the value from the document by its path, so it works for lines that are not drawn.
- Where a syntax error is, with its line and column, when the file is not valid JSON.
- The encoding of the text, in a menu (Auto, UTF-8, UTF-16 LE or BE, Windows-1252).

## Limits

- Reads and parses files up to 32 MB, in pieces of 4 MB (waiting when the host says to slow down). A larger file shows its first 32 MB as raw text only, with no tree, and says so.
- A document above 4 MB is called large: the Pretty view is off (it would copy the whole of it), and the Raw view shows the first 20,000 lines, each cut at 10,000 characters (a minified document is one line).
- Parsing a big file holds the page for a moment (about 0.15 s for 20 MB); the parsed document and its text are both kept, so a 20 MB document uses roughly 50 to 100 MB.
- Search marks at most 2,000 matches; the text views draw at most 20,000 lines; nesting deeper than 400 levels is cut. Expand all opens at most 3,000 containers, no deeper than 8 levels, and about 150,000 lines.
- Measured (a test page standing in for Folio): a 25,000-item array opens in 65 ms and a far match is found and scrolled to in about 250 ms; a 2.5-million-item array (19 MB) opens in 0.4 s and a match near its end is found in 0.5 s; a 20 MB document of objects opens in 0.2 s and a search to its end takes 0.4 s.

## How it is built

A page (`index.html`), a helper (`json.js`, a plain script with its own tests: the open lines of the tree, the search through a document without an index of every node, following a path) and `encoding.js`. It parses with the browser's own `JSON.parse` and finds the position of an error with its own scanner. Text is put on the page only with `textContent`.

Source: [`src-tauri/resources/plugins/json-viewer/`](../../src-tauri/resources/plugins/json-viewer/).

## Thumbnail

![JSON viewer page art](../../src-tauri/resources/plugins/json-viewer/thumb.svg)

The page art is [`thumb.svg`](../../src-tauri/resources/plugins/json-viewer/thumb.svg) (56 × 72, colour `#f59f00`); Folio draws the extension badge over it. It was designed in Penpot, in Design System > Components > Thumbnails, in the row "Plugin thumbnails · bundled".
