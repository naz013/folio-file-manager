# JSON viewer

Quick Look for JSON files: a tree you can fold and search, pretty and raw views. A plugin that ships with Folio (id `json-viewer`, version 1.0.0); it is off until you turn it on. See [Plugins](../PLUGINS.md) for how plugins work.

## File types

- **Quick Look** draws `.json`, `.geojson`, `.jsonld`, `.har`.
- **Page icons** are drawn for the same kinds of file.

## What it asks for

- `read` (it can read the files Folio hands it, and nothing else). It has no network access and cannot change a file.

## What it shows

- A **Tree** of the document to fold and unfold (**Expand all**, **Collapse all**), with the number of items in each object and array.
- **Pretty** (indented) and **Raw** (as written) text views.
- Search (Ctrl+F) through keys and values, with previous and next match.
- **Copy path** and **Copy value** for the selected row.
- Where a syntax error is, with its line and column, when the file is not valid JSON.
- The encoding of the text, in a menu (Auto, UTF-8, UTF-16 LE or BE, Windows-1252).

## Limits

- Reads files up to 4 MB. A larger file shows its first 4 MB as raw text only, with no tree.
- Search marks at most 2,000 matches; the text views draw at most 20,000 lines; nesting deeper than 400 levels is cut.

## How it is built

One page (`index.html`) and `encoding.js`. It parses with the browser's own `JSON.parse` and finds the position of an error with its own scanner. Text is put on the page only with `textContent`.

Source: [`src-tauri/resources/plugins/json-viewer/`](../../src-tauri/resources/plugins/json-viewer/).

## Thumbnail

![JSON viewer page art](../../src-tauri/resources/plugins/json-viewer/thumb.svg)

The page art is [`thumb.svg`](../../src-tauri/resources/plugins/json-viewer/thumb.svg) (56 × 72, colour `#f59f00`); Folio draws the extension badge over it. It was designed in Penpot, in Design System > Components > Thumbnails, in the row "Plugin thumbnails · bundled".
