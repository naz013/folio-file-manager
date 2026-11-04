# CSV viewer

Quick Look for CSV and TSV files: a table you can sort and filter, with the delimiter found for you. A plugin that ships with Folio (id `csv-viewer`, version 1.0.0); it is off until you turn it on. See [Plugins](../PLUGINS.md) for how plugins work.

## File types

- **Quick Look** draws `.csv`, `.tsv`.
- **Page icons** are drawn for the same kinds of file.

## What it asks for

- `read` (it can read the files Folio hands it, and nothing else). It has no network access and cannot change a file.

## What it shows

- A table with a fixed header row and only the rows in view drawn, so large files stay quick.
- A large file is read in pieces of 4 MB: the first rows show at once, and a bar under the toolbar shows the reading (with a **Stop** button) while the table stays usable.
- Sortable columns; numbers sort as numbers. Sorting and filtering work in short slices, so the table keeps scrolling while they run ("Sorting…", "Filtering… 40%"); a new request replaces the one under way, and filtering again keeps the order already found.
- A row filter, and **Copy cell** and **Copy row** for the selected cell or row.
- The delimiter found for you (comma, semicolon, tab or pipe), which you can change, and a **Header row** switch for whether the first row is the header.
- The encoding of the text, in a menu (Auto, UTF-8, UTF-16 LE or BE, Windows-1252).

## Limits

- Reads the first 16 MB by itself. For a larger file a **Read more** button reads on, up to 64 MB or 750,000 rows (whichever comes first) and 300 columns; the note under the toolbar says how much is shown, and why it stops. Every row is an entry in memory, which is what the limits protect.
- While a file is still being read, or when it is cut at a limit, sorting and filtering cover the rows read so far, and the page says so. When reading finishes, an active sort or filter is applied again to the whole.
- Changing the delimiter reads the file again from the start; changing the header switch does not.
- The host limits how many reads a plugin makes in a while (20 in 10 seconds); the viewer reads 4 MB at a time and waits when told to slow down, so a large file takes a little longer rather than stopping.
- A single row (or a quote that is never closed) longer than 8 MB ends the reading, and the notice says so.

## How it is built

A page (`index.html`), a parser (`csv.js`, a plain script with its own tests) and `encoding.js`. Rows are virtualised; cells are put on the page with `textContent`.

Source: [`src-tauri/resources/plugins/csv-viewer/`](../../src-tauri/resources/plugins/csv-viewer/).

## Thumbnail

![CSV viewer page art](../../src-tauri/resources/plugins/csv-viewer/thumb.svg)

The page art is [`thumb.svg`](../../src-tauri/resources/plugins/csv-viewer/thumb.svg) (56 × 72, colour `#12a594`); Folio draws the extension badge over it. It was designed in Penpot, in Design System > Components > Thumbnails, in the row "Plugin thumbnails · bundled".
