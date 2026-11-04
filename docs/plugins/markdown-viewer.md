# Markdown viewer

Quick Look for Markdown files, drawn safely, with search and a source view. A plugin that ships with Folio (id `markdown-viewer`, version 1.0.0); it is off until you turn it on. See [Plugins](../PLUGINS.md) for how plugins work.

## File types

- **Quick Look** draws `.md`, `.markdown`, `.mdown`, `.mkd`.
- **Page icons** are drawn for the same kinds of file.

## What it asks for

- `read` (it can read the files Folio hands it, and nothing else). It has no network access and cannot change a file.

## What it shows

- **Rendered**: headings, lists, task lists, tables, code blocks, quotes, emphasis and front matter.
- **Source**: the file as written.
- Search (Ctrl+F) with previous and next match.
- Links are shown with their address but not opened; images are not loaded; any HTML in the file is shown as text.
- The encoding of the text, in a menu (Auto, UTF-8, UTF-16 LE or BE, Windows-1252).

## Limits

- Reads the first 1 MB; a note says how much of the file is shown.
- Not drawn yet: images next to the file, footnotes and code colours (see issue #123).

## How it is built

A page (`index.html`), a parser (`markdown.js`, a plain script with its own tests) and `encoding.js`. The parser makes a tree and the page draws it with `createElement` and `textContent`; nothing from the file is ever turned into markup.

Source: [`src-tauri/resources/plugins/markdown-viewer/`](../../src-tauri/resources/plugins/markdown-viewer/).

## Thumbnail

![Markdown viewer page art](../../src-tauri/resources/plugins/markdown-viewer/thumb.svg)

The page art is [`thumb.svg`](../../src-tauri/resources/plugins/markdown-viewer/thumb.svg) (56 × 72, colour `#1d1d22`); Folio draws the extension badge over it. It was designed in Penpot, in Design System > Components > Thumbnails, in the row "Plugin thumbnails · bundled".
