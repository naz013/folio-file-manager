# CSV viewer

Quick Look for CSV and TSV files: a table you can sort and filter, with the delimiter found for you. A plugin that ships with Folio (id `csv-viewer`, version 1.0.0); it is off until you turn it on. See [Plugins](../PLUGINS.md) for how plugins work.

## File types

- **Quick Look** draws `.csv`, `.tsv`.
- **Page icons** are drawn for the same kinds of file.

## What it asks for

- `read` (it can read the files Folio hands it, and nothing else). It has no network access and cannot change a file.

## What it shows

- A table with a fixed header row and only the rows in view drawn, so large files stay quick.
- Sortable columns; numbers sort as numbers.
- A row filter, and **Copy cell** and **Copy row** for the selected cell or row.
- The delimiter found for you (comma, semicolon, tab or pipe), which you can change, and a **Header row** switch for whether the first row is the header.
- The encoding of the text, in a menu (Auto, UTF-8, UTF-16 LE or BE, Windows-1252).

## Limits

- Reads up to 8 MB, 500,000 rows and 300 columns; a note says when a file is cut.
- Files above 8 MB are not shown whole (see issue #124).

## How it is built

A page (`index.html`), a parser (`csv.js`, a plain script with its own tests) and `encoding.js`. Rows are virtualised; cells are put on the page with `textContent`.

Source: [`src-tauri/resources/plugins/csv-viewer/`](../../src-tauri/resources/plugins/csv-viewer/).

## Thumbnail

![CSV viewer page art](../../src-tauri/resources/plugins/csv-viewer/thumb.svg)

The page art is [`thumb.svg`](../../src-tauri/resources/plugins/csv-viewer/thumb.svg) (56 × 72, colour `#12a594`); Folio draws the extension badge over it. It was designed in Penpot, in Design System > Components > Thumbnails, in the row "Plugin thumbnails · bundled".
