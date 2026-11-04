# Folio plugins

A plugin adds **actions** to the right-click menu and **previews** to Quick Look. It is a folder
of web files that runs in a sandbox and can do only what you allow. The reasoning is in
[ADR 0005](decisions/0005-plugin-api.md); a complete example is in
[`examples/plugins/text-tools`](../examples/plugins/text-tools).

## Installing

1. Open the Plugins screen (Settings › Manage plugins, or Ctrl+K and type "plugins"), and choose **Open folder**. Folio makes the plugins folder if it isn't
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

| Field         | Meaning                                                                           |
| ------------- | --------------------------------------------------------------------------------- |
| `id`          | Same as the folder name.                                                          |
| `name`        | Shown on the Plugins screen and before its messages. Up to 60 characters.         |
| `version`     | Any text up to 20 characters. Changing it turns the plugin off until re-approved. |
| `description` | Optional, up to 200 characters.                                                   |
| `api`         | The plugin API version, `1`. A higher number than Folio knows disables it.        |
| `entry`       | The page, an `.html` file in the folder. Default `index.html`.                    |
| `permissions` | What it asks for; see below. Default none.                                        |
| `actions`     | Menu items. At most 20. Needs the `selection` permission.                         |
| `previews`    | Quick Look pages. At most 10.                                                     |

An **action** has an `id` (lower-case letters, digits, `-`, `_`), a `title`, `kinds` (`"file"`,
`"folder"`; default `["file"]`), `extensions` (files only; none means any file) and `multiple`
(may run on several items at once; default one). It is offered when **every** selected item fits.

A **preview** has an `id`, a `title` and the `extensions` it draws. It replaces Quick Look's own
view for those files while the plugin is on.

Unknown fields are ignored. Settings shows what is wrong with a manifest that can't be read.

## Permissions

| Permission  | Lets the plugin                                                             |
| ----------- | --------------------------------------------------------------------------- |
| `selection` | be told which files an action was run on, and ask again for them            |
| `read`      | read the start of the text of those files, and of the file it is previewing |
| `notify`    | show a short message (up to 200 characters, a few every few seconds)        |

A preview is always told the name and path of its file; reading it needs `read`. There is **no
permission to change, move or delete files**: in this version a plugin can only look and say.

## The sandbox

The page runs in an iframe with `sandbox="allow-scripts"` and nothing more. It has no cookies or
storage and no access to Folio's page, cannot open windows, submit forms or navigate away, and
its requests for scripts, data, images, fonts and frames are blocked except for its own files
(`connect-src 'none'`; images and fonts from itself or `data:`). This stops it from sending what
it reads to a server in the usual ways, but it is **not a guarantee**: the webview may still let a
page leak a few bytes through a side channel such as a DNS lookup. Allow `read` only for plugins
you trust. Its files come from its folder only, and only these types: `html`, `htm`,
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

| Method      | Params        | Needs       | Result                                      |
| ----------- | ------------- | ----------- | ------------------------------------------- |
| `selection` |               | `selection` | the paths of the files of the last action   |
| `read`      | `{ path }`    | `read`      | `{ text, truncated }`; start of a text file |
| `notify`    | `{ message }` | `notify`    | `null`                                      |

`read` works only for paths Folio gave the plugin in its last few `action` or `preview` events, for
a minute after each, at most 2 at once and 20 per 10 seconds, as text,
up to the size Quick Look reads, and not for files that are only in the cloud (the error then is
`Only in the cloud`). A call without its permission is answered with an error naming it.

## Writing one

Start from `examples/plugins/text-tools`: copy it into the plugins folder, change the id in the
folder name and `plugin.json`, and Reload. After changing a file, close and reopen Quick Look, or
turn the plugin off and on, to load the page again. The folder is read when you press Reload; the
files are read when the page loads.

## Limits of this version

- No way to change files, and no storage between runs.
- A page that keeps sending more than 50 messages a second is turned off.
- Two extension points: actions and previews. Toolbar buttons and sidebar sections are not there.
- Plugins are not signed or reviewed, and there is no store; turn on only ones you trust.
