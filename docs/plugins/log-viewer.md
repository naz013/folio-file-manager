# Log viewer

Quick Look for log files: levels in colour, a filter, and a way to follow the end of the file. A plugin that ships with Folio (id `log-viewer`, version 1.0.0); it is off until you turn it on. See [Plugins](../PLUGINS.md) for how plugins work.

## File types

- **Quick Look** draws `.log`, `.out`, `.err`.
- `.log` is also taken by the text viewer. When both are on, the log viewer draws it: Folio picks the first plugin in id order, and `log-viewer` comes before `text-viewer`.
- **Page icons** are drawn for the same kinds of file.

## What it asks for

- `read` (it can read the files Folio hands it, and nothing else). It has no network access and cannot change a file.

## What it shows

- The file opened **at the end**, where the news is, with earlier parts loaded as you scroll up.
- Error, warn, info, debug and trace lines in colour; a stack trace takes the level of the line above it. Timestamps are dimmed.
- Filters by level (a row of chips: Error, Warn, Info, Debug, Trace, Other) and by text.
- **Start** and **End** jump to either end of the file.
- **Follow**: keeps up with a file as it grows. A half-written last line waits until it is finished; a replaced file (a rotated log) is noticed and read again.

## Limits

- Holds at most 24 MB of the file at a time; only the rows in view are drawn. Reads the end of the file first (512 KB), then 1 MB pieces, and shows at most 4,000 characters of a line.

## How it is built

A page (`index.html`) and a helper (`log.js`, a plain script with its own tests). It reads pieces with `readRange` and checks that the file it is following is still the same one.

Source: [`src-tauri/resources/plugins/log-viewer/`](../../src-tauri/resources/plugins/log-viewer/).

## Thumbnail

![Log viewer page art](../../src-tauri/resources/plugins/log-viewer/thumb.svg)

The page art is [`thumb.svg`](../../src-tauri/resources/plugins/log-viewer/thumb.svg) (56 × 72, colour `#e8590c`); Folio draws the extension badge over it. It was designed in Penpot, in Design System > Components > Thumbnails, in the row "Plugin thumbnails · bundled".
