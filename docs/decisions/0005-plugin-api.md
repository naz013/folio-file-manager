# ADR 0005 — Plugins: a manifest and a sandboxed page

- **Status:** Accepted
- **Date:** 2026-11-03

## Context

Power users want to add actions (a context-menu item that does something to the selected files)
and previews (Quick Look for a format Folio doesn't show) without Folio growing a feature for
each. Code from outside Folio must not be able to read the disk, write to it, or send what it
sees anywhere, unless the person has said so for that plugin and that ability.

## Decision

1. **A plugin is a folder** under `<app data>/plugins/<id>/` with a `plugin.json` manifest and
   web files (HTML, JS, CSS, images). The folder name is the plugin's id (`a-z`, `0-9`, `-`).
   Folio never runs anything from it outside the page below. There is no install step beyond
   copying the folder; the Plugins screen lists what is there.
2. **The page runs in a sandboxed iframe**: `sandbox="allow-scripts"` and nothing else, so it
   has an opaque origin (no cookies, storage, or access to Folio's page), cannot open windows,
   submit forms, navigate the window, or use powerful features. Its files are served by the
   `plugin://` protocol from that one folder only (no `..`, no links out, a short list of file
   types, a size limit), with a Content-Security-Policy that blocks fetches, scripts, images, fonts
   and frames from anywhere else (`connect-src 'none'`, images and fonts from itself or `data:`
   only). DNS lookups and WebRTC are not covered by it, so a hostile plugin that has been given
   `read` may leak small amounts of what it reads: the permission is for plugins one trusts. The app's own page
   limits where any frame can navigate (`frame-src`), so a plugin can't send itself to a
   website with data in the address.
3. **The only way out is Folio's API**, a small request/answer protocol over `postMessage`.
   Folio answers only the frame it created, and only for abilities the person granted:
   - `selection`: be told which files (paths) an action was started on.
   - `read`: read the start of the text of a file Folio gave the plugin (a selected file or the
     file being previewed), and no other.
   - `notify`: show a short message.
     A preview is always told the name and path of the file it is drawing; reading it needs `read`.
     Reading is as text, with what `read_text_preview` allows (size limit, not for files only in
     the cloud). **There is no way to write, move or delete** in this version: every change to
     files stays in the Rust core and its journal (CLAUDE.md).
4. **Permissions are asked for in the manifest and granted by the person**, on the Plugins screen, listing
   in plain words what the plugin can do. A plugin is off until then. A plugin whose version or
   requested permissions change is off again until re-approved. The grant is kept with Folio's
   settings, by plugin id.
5. **Two extension points now:** `actions` (items in the context menu of selected files, matched
   by kind and extension) and `previews` (Quick Look for given extensions, drawn in the plugin's
   own frame). More (toolbar buttons, sidebar sections) can be added to the manifest later;
   an unknown field is ignored, a newer `api` than Folio's disables the plugin with a message.

## Amendment — the page is a document, not a link

A first version put the plugin page in `<iframe src="http://plugin.localhost/…">`. That was an
escape: Tauri counts any address on one of its own protocols as local (trusted like Folio's page,
so the `default` capability applies), and on Windows it puts its command channel and invoke key
into **every** frame, not only the main one. A plugin page could call `__TAURI_INTERNALS__.invoke`
and run Folio's commands (found by the `ipc-probe` example, which opened Explorer). Now the page's
HTML is read by Folio and given to the frame as `srcdoc`, with a `<base>` pointing at the plugin's
folder and a Content-Security-Policy ahead of its own markup (`pageDoc` in `plugins.ts`). A
`srcdoc` page has no plugin address, so what it sends is treated as from an untrusted page and
refused. The app page allows frames only at `about:` (`frame-src about:`), so a plugin can't send
its frame to a plugin address either. Never add a plugin address to that policy, and run
`examples/plugins/ipc-probe` after any change to how plugin pages are loaded.

## Amendment — serving what was approved, and less to abuse

- **Serve-time pin.** The page pins a plugin to the digest the person approved before loading
  it (`plugins_pin`). The core reads the files once, checks they make that digest, and keeps each
  file's hash; `plugin://` then serves a file only if its bytes still hash to that. Files
  changed or added after approval are refused, and a plugin never pinned serves nothing.
- **Frame hardening.** `pageDoc` runs a script ahead of the plugin's own that removes Tauri's
  helpers and WebRTC from the frame. Tauri's refusal of a non-local frame stays the first lock;
  this is the second. WebView2 is also started with
  `--force-webrtc-ip-handling-policy=disable_non_proxied_udp`.
- **Previews are limited**: at most 20 file types in all, and none that hold secrets (key stores,
  certificates, `.env`), since a preview reads files without a click.
- **Bundled plugins.** Plugins can ship in the app's resources (`plugins/` next to the executable).
  They go through the same listing, approval, pinning and sandbox as the person's own; one of
  the same id in the person's folder replaces it. Nothing built in is trusted or approved by
  default, and what it may ask for is limited to what the API gives any plugin.
- **Not closed**: a DNS lookup from a page may still leak a few bytes (see `docs/PLUGINS.md`).

## Consequences

- A plugin can show anything on its own page, and may mislead a person by what it shows;
  approval is of what it can _do_, not of what it _says_. Plugins are not reviewed or signed.
- A plugin can't draw outside its frame, run in the background without an enabled action or
  preview, or keep data between runs (no storage). If that is needed later it gets an API
  (`store`) and a permission.
- Frames are created on first use and removed when the plugin is turned off, so a plugin that
  loops can't outlive its disabling.
- All plugins share one address (`plugin.localhost`), so one plugin's page can load another's
  files; a plugin keeps nothing secret from another that is on.
- Approval is of the version, the permissions and a hash of the files, and what is served is
  checked against it file by file; removing a plugin's folder drops its approval at Reload.
- A page can read a handed file for a minute, a few at a time; one that floods the bridge is
  turned off. Served pages also carry `sandbox allow-scripts` in their policy, and a plugin's
  own folder must not be a link or junction.
- The sandbox rests on the webview: a flaw in WebView2's iframe sandbox is a flaw here. Not a
  substitute for only enabling plugins one trusts.
- Out of scope: a store or updater for plugins, signing, plugins in other languages, and
  macOS/Linux differences beyond the protocol's address (`plugin://localhost` there).
