# Folio plugins

A plugin adds **actions** to the right-click menu and **previews** to Quick Look. It is a folder
of web files that runs in a sandbox and can do only what you allow. The reasoning is in
[ADR 0005](decisions/0005-plugin-api.md); a complete example is in
[`examples/plugins/text-tools`](../examples/plugins/text-tools).

## Installing

1. Open the Plugins screen (Settings › Manage plugins, or Ctrl+K and type "plugins"), and choose **Open folder** to see the plugins folder in a tab (**Show in Explorer** opens it in the system's file manager). Folio makes the plugins folder if it isn't
   there.
2. Copy a plugin folder into it. The folder's name is the plugin's id and must match `"id"` in the
   manifest (lower-case letters, digits and hyphens, up to 40 characters).
3. Choose **Reload**, then turn the plugin on. Folio lists what it asks to be allowed to do;
   nothing runs until you choose **Allow and turn on**.

A plugin turns itself off if its `version`, its permissions or any of its files change, so what
you approved is what runs. Folio hashes the manifest and every file it could serve when the
folder is read (at start and on **Reload**); a plugin edited in place needs approving again.
Turning it off, or removing its folder, forgets the approval. A `name` can't start like Folio or
a part of Windows, and its messages are shown as `Name (plugin): …`.

## Plugins that come with Folio

Some plugins ship inside the app (source in `src-tauri/resources/plugins/`, shown on the Plugins
screen as **Built in**). They are plugins like any other: they run in the same sandbox, are off until
you turn them on and read what they can do, and are checked file by file when served. A plugin of
the same id in your plugins folder replaces the built-in one, and has to be approved on its own
files.

| Plugin          | What it does                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| --------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Text viewer     | Quick Look and a page icon for text and code files: line numbers, wrapping, search (Ctrl+F). Loads more of a large file as you scroll, up to 2 MB                                                                                                                                                                                                                                                                                                                                                                                                           |
| Hex viewer      | Quick Look and a page icon for binary kinds (`.bin`, `.dat`, `.dll`, `.so`, `.class`, `.wasm`, `.db`, `.sqlite`, `.iso`, `.dmp` and more; the preview also takes `.exe`): offset, hex and text columns, a selected byte read as numbers (u8, i8, u16, u32), go to an offset (`0x1F00`, `7936`, `1F00h`), and find bytes or text, forwards and backwards, round the end of the file, with progress and a Stop button. Only the rows in view are drawn, and the scroll bar maps onto files of any size. The page icon leaves `.exe` to the program's own icon |
| Log viewer      | Quick Look and a page icon for `.log`, `.out` and `.err`: opens at the end of the file, colours error, warn, info, debug and trace lines (a stack trace takes the level of the line above it), dims timestamps, filters by level and by text, loads earlier parts as you scroll up, and can follow a file as it grows (a half-written last line waits until it is finished; a replaced file is noticed and read again). Only the rows in view are drawn, and at most 24 MB are held. When the text viewer is also on, the log viewer takes `.log`           |
| CSV viewer      | Quick Look for `.csv` and `.tsv`: a table with a fixed header, sortable columns (numbers sort as numbers), a row filter, the delimiter found for you (or chosen), a header-row switch, and copy cell or row. Only the rows in view are drawn, so large files stay quick; reads up to 8 MB, 500,000 rows and 300 columns                                                                                                                                                                                                                                     |
| Markdown viewer | Quick Look for `.md`, `.markdown`, `.mdown`, `.mkd`: headings, lists, tasks, tables, code, quotes, front matter, with search and a Source view. Links are shown with their address but not opened, remote images are not loaded, and any HTML in the file is shown as text. Reads the first 1 MB                                                                                                                                                                                                                                                            |
| JSON viewer     | Quick Look for `.json`, `.geojson`, `.jsonld`, `.har`: a tree to fold and search, copy a path or value, pretty and raw views, and where a syntax error is. Reads files up to 4 MB; a larger one shows its first 4 MB as raw text only                                                                                                                                                                                                                                                                                                                       |

## The manifest

`plugin.json` in the plugin folder:

```json
{
  "id": "text-tools",
  "name": "Text tools",
  "version": "1.0.0",
  "description": "One line about it.",
  "api": 1,
  "entry": "index.html",
  "permissions": ["selection", "read", "notify"],
  "actions": [
    {
      "id": "count",
      "title": "Count words and lines",
      "kinds": ["file"],
      "extensions": ["txt", "md"],
      "multiple": true
    }
  ],
  "previews": [{ "id": "csv", "title": "CSV table", "extensions": ["csv"] }]
}
```

| Field         | Meaning                                                                             |
| ------------- | ----------------------------------------------------------------------------------- |
| `id`          | Same as the folder name.                                                            |
| `name`        | Shown on the Plugins screen and before its messages. Up to 60 characters.           |
| `version`     | Any text up to 20 characters. Changing it turns the plugin off until re-approved.   |
| `description` | Optional, up to 200 characters.                                                     |
| `api`         | The plugin API version, `1`. A higher number than Folio knows disables it.          |
| `entry`       | The page, an `.html` file in the folder. Default `index.html`.                      |
| `permissions` | What it asks for; see below. Default none.                                          |
| `actions`     | Menu items. At most 20. Needs the `selection` permission.                           |
| `previews`    | Quick Look pages. At most 10, and at most 20 file types claimed in all.             |
| `thumbnails`  | Pictures for kinds of file in the grid and lists. At most 10, 40 file types in all. |

An **action** has an `id` (lower-case letters, digits, `-`, `_`), a `title`, `kinds` (`"file"`,
`"folder"`; default `["file"]`), `extensions` (files only; none means any file) and `multiple`
(may run on several items at once; default one). It is offered when **every** selected item fits.

A **preview** has an `id`, a `title` and the `extensions` it draws. It replaces Quick Look's own
view for those files while the plugin is on. A preview opens on every such file you merely browse
to, so it may not claim kinds of file that hold secrets (`kdbx`, `pem`, `key`, `pfx`, `p12`,
`ppk`, `env`, `gpg`, `ovpn`, `rdp`, and a few more); an action may, because you run it on
purpose.

A **thumbnail** gives the files of some kinds a picture in place of the plain page icon:
`{ "extensions": ["json"], "image": "thumb.svg", "color": "#f59f00" }`. The `image` is an `.svg`,
`.png` or `.webp` file in the plugin's folder, drawn at 56 x 72 like Folio's own page icon (the page,
its folded corner, and what the file holds, with room for a badge in the lower left). Folio draws the
badge itself, with the file's extension on the `color` (`#rrggbb`; grey if left out). The picture is
a static image: it is not made from the file, can't run anything and is only shown while the plugin
is on. It takes the place of the system's picture for those kinds of file, which for text and data files is only a file-type icon; a plugin's art is shown instead. The artwork is
designed in Penpot (Design System > Components > Thumbnails, "Plugin thumbnails · bundled").

Unknown fields are ignored. Settings shows what is wrong with a manifest that can't be read.

## Permissions

| Permission  | Lets the plugin                                                             |
| ----------- | --------------------------------------------------------------------------- |
| `selection` | be told which files an action was run on, and ask again for them            |
| `read`      | read the start of the text of those files, and of the file it is previewing |
| `notify`    | show a short message (up to 200 characters, a few every few seconds)        |

A preview is always told the name and path of its file; reading it needs `read`. There is **no
permission to change, move or delete files**: in this version a plugin can only look and say.

## Who made a preview

A plugin's preview fills Quick Look, and a plugin can draw anything on its page. So Folio always
draws a bar of its own under the page, which the plugin cannot draw over: "Preview by <name>", a
badge saying **Built-in plugin** (grey) or **Plugin** (amber, for one you installed), and a **Show
normally** button that puts Folio's own view of the file in its place. The name is the one in the
manifest, shown as plain text. The bar is a labelled group for screen readers.

## The sandbox

The page runs in an iframe with `sandbox="allow-scripts"` and nothing more. It has no cookies or
storage and no access to Folio's page, cannot open windows, submit forms or navigate away, and
its requests for scripts, data, images, fonts and frames are blocked except for its own files
(`connect-src 'none'`; images and fonts from itself or `data:`). This stops it from sending what
it reads to a server in the usual ways, but it is **not a guarantee**: the webview may still let a
page leak a few bytes through a side channel such as a DNS lookup. WebRTC, which could carry data
out past that policy, is removed from the page and switched off for direct connections in the
webview. Allow `read` only for plugins you trust. Its files come from its folder only, and only these types: `html`, `htm`,
`js`, `mjs`, `css`, `json`, `txt`, `md`, `svg`, `png`, `jpg`, `jpeg`, `gif`, `webp`, `woff2`, up to
5 MB each. All plugins are served from one address, so a plugin's page can load another plugin's scripts and
images; nothing a plugin holds is secret from another one that is on. Anything it needs from Folio
goes through the messages below. Treat a file's contents as
untrusted when you show them: set `textContent`, not `innerHTML`.

## Messages

Every message is an object with `folio: 1`. The page sends them with
`parent.postMessage(message, "*")` and listens with `window.addEventListener("message", …)`.

**The page says it is ready**, once loaded. Folio then sends the action or preview that opened it:

```js
parent.postMessage({ folio: 1, event: "ready" }, "*");
```

**Folio tells the page**

```js
{ folio: 1, event: "action", action: "count", paths: ["C:\\a.txt", "C:\\b.txt"] }
{ folio: 1, event: "preview", preview: "csv", path: "C:\\data.csv", name: "data.csv", size: 1200 }
```

An action's page lives while the plugin is on, after the first action; it gets one `action` event
for each run, in order. The files of the last five runs can be read, so runs that overlap don't
cut each other off. A preview's page is made when the file is shown and removed when Quick Look moves on.

**The page asks Folio** `{ folio: 1, id, method, params }` and gets `{ folio: 1, id, result }` or
`{ folio: 1, id, error }` (the error is a sentence):

| Method      | Params                     | Needs       | Result                                                      |
| ----------- | -------------------------- | ----------- | ----------------------------------------------------------- |
| `selection` |                            | `selection` | the paths of the files of the last action                   |
| `read`      | `{ path }`                 | `read`      | `{ text, truncated }`; start of a text file                 |
| `readRange` | `{ path, offset, length }` | `read`      | `{ text, start, end, size }`; a piece of a text file        |
| `readBytes` | `{ path, offset, length }` | `read`      | `{ data, start, end, size }`; a piece of any file, as bytes |
| `notify`    | `{ message }`              | `notify`    | `null`                                                      |

`read` works only for paths Folio gave the plugin in its last few `action` or `preview` events, for
a minute after each, at most 2 at once and 20 per 10 seconds, as text,
up to the size Quick Look reads, and not for files that are only in the cloud (the error then is
`Only in the cloud`). A call without its permission is answered with an error naming it.

`readRange` reads a piece of a text file, for pages that show a large file bit by bit. `offset` and
`length` are in bytes; `length` is at most 4 MB. The piece never holds half a character: if `offset`
falls inside one, `start` is the next character's byte, and if `length` would cut one, the piece
stops before it. `end` is where to ask for the next piece (`offset: end`), and `end >= size` means the
file is done. `size` is the file's size in bytes. It follows the rules of `read` (a file Folio gave
the plugin, the same limits on reads), and the files of a **preview** stay readable for as long as it
is open, not only for a minute. A file with NUL bytes (binary) is refused.

`readBytes` reads a piece of any regular file as bytes, for pages that show binary files. `offset` and
`length` are in bytes; `length` is at most 1 MB (less than a text piece, since bytes are raw), and
`offset` may be anywhere in the file, even past its end, which gives no bytes. `data` is a
`Uint8Array`; `start` is where it begins (the end of the file for an offset past it), `end` is where the
next piece starts and `size` is the file's size. It follows the rules of `read` and `readRange`: the
`read` permission, a file Folio gave the plugin, the same limits on reads, and not for a file that is
only in the cloud. It never reads a whole file: only ranges, so a plugin that wants more asks again.

## Writing one

Start from `examples/plugins/text-tools`: copy it into the plugins folder, change the id in the
folder name and `plugin.json`, and Reload. After changing a file, close and reopen Quick Look, or
turn the plugin off and on, to load the page again. The folder is read when you press Reload; the
files are read when the page loads.

## Limits of this version

- No way to change files, and no storage between runs.
- Files are served only as they were when you approved the plugin: Folio hashes each file it
  serves against that approval, so one changed afterwards (without Reload and approving again)
  is refused.
- A page that keeps sending more than 50 messages a second is turned off.
- Two extension points: actions and previews. Toolbar buttons and sidebar sections are not there.
- Plugins are not signed or reviewed, and there is no store; turn on only ones you trust.
