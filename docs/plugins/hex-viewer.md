# Hex viewer

Quick Look for binary files: offsets, bytes and text, with a search for bytes or words. A plugin that ships with Folio (id `hex-viewer`, version 1.0.0); it is off until you turn it on. See [Plugins](../PLUGINS.md) for how plugins work.

## File types

- **Quick Look** draws `.bin`, `.dat`, `.exe`, `.dll`, `.sys`, `.so`, `.dylib`, `.o`, `.obj`, `.lib`, `.class`, `.pyc`, `.wasm`, `.db`, `.sqlite`, `.sqlite3`, `.iso`, `.img`, `.dmp`, `.cab`.
- **Page icons** are drawn for `.bin`, `.dat`, `.dll`, `.sys`, `.so`, `.dylib`, `.o`, `.obj`, `.lib`, `.class`, `.pyc`, `.wasm`, `.db`, `.sqlite`, `.sqlite3`, `.iso`, `.img`, `.dmp`, `.cab`.

## What it asks for

- `read` (it can read the files Folio hands it, and nothing else). It has no network access and cannot change a file.

## What it shows

- Offset, hex and text columns; only the rows in view are drawn, and the scroll bar maps onto files of any size.
- The selected byte read as numbers (u8, i8, u16, u32).
- **Go to** an offset (`0x1F00`, `7936` or `1F00h`).
- **Find** bytes or text, forwards and backwards, round the end of the file, with progress and a Stop button.

## Limits

- Reads pieces of at most 1 MB with `readBytes` (it shows 64 KB at a time); it never loads a whole file.
- A search pattern is at most 64 bytes.
- The page icon leaves `.exe` to the program's own icon, though the preview takes `.exe`.

## How it is built

A page (`index.html`) and a helper (`hex.js`, a plain script with its own tests). It reads raw bytes with `readBytes`, which hands the page a `Uint8Array`.

Source: [`src-tauri/resources/plugins/hex-viewer/`](../../src-tauri/resources/plugins/hex-viewer/).

## Thumbnail

![Hex viewer page art](../../src-tauri/resources/plugins/hex-viewer/thumb.svg)

The page art is [`thumb.svg`](../../src-tauri/resources/plugins/hex-viewer/thumb.svg) (56 × 72, colour `#5b6475`); Folio draws the extension badge over it. It was designed in Penpot, in Design System > Components > Thumbnails, in the row "Plugin thumbnails · bundled".
