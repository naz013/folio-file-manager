# Folio on Linux

Packages, supported systems, and the rendering checks for the WebKitGTK web view. Folio's
window is a web page; on Linux it is drawn by the system's WebKitGTK, so the distro decides
which web features exist. Windows (WebView2) and macOS (WKWebView) keep up on their own.

## Packages

| Package     | Made by        | For                                                    |
| ----------- | -------------- | ------------------------------------------------------ |
| `.deb`      | Release and CI | Debian, Ubuntu and relatives                           |
| `.rpm`      | Release and CI | Fedora, openSUSE and relatives                         |
| `.AppImage` | Release only   | Anything else; carries its own libraries, run directly |

The packages depend on the system's WebKitGTK 4.1 and GTK 3 (Tauri adds those itself) and
recommend `xdg-utils` (opening files) and `webp-pixbuf-loader` (WebP thumbnails). They are built
on Ubuntu 22.04 so they run on older libc versions too.

CI installs each package in a plain image and starts it (`package-smoke` in `ci.yml`,
`scripts/linux-smoke.sh`): Ubuntu 22.04 and 24.04, Debian 12 and Fedora 43. Try it locally with
`npm run build` then `npm run linux:packages`, which makes the `.deb` and `.rpm` in `dist-linux/`.
That build is on Debian 12, not Ubuntu 22.04, so it is for a quick look; the CI packages are the
ones to ship.

Checked by hand on 2026-11-04 with `npm run linux:packages`: the `.deb` installs, links and
stays running on Ubuntu 22.04 (which installed WebKitGTK 2.50.4 from its updates) and Debian 12,
and the `.rpm` on Fedora 43. Ubuntu 24.04 is only covered by CI. No window was looked at.

## Minimum WebKitGTK

From what the styles use, Folio needs WebKitGTK 2.40 (Safari 16.4; worked out from the
feature list, not yet tried on such a system): `color-mix()` for the tag shades and `@property` for
the spring-loaded folder ring. Check `apt policy libwebkit2gtk-4.1-0` or `rpm -q webkit2gtk4.1` on a system; Ubuntu
22.04 with updates, Debian 12 and Fedora 38 or newer should have it. Newer features are used with a fallback:

| Feature                 | Used in             | Since WebKitGTK | Without it                                     |
| ----------------------- | ------------------- | --------------- | ---------------------------------------------- |
| `scrollbar-width: none` | the tab strip       | 2.48            | `::-webkit-scrollbar` hides it instead         |
| `light-dark()`          | (not used any more) | 2.46            | spelled out per theme, so borders never vanish |

Before adding a CSS or JS feature, check its WebKit support on
[caniuse](https://caniuse.com) against Safari 16.4; if it is newer, give it a fallback and add
it to this table.

## Rendering QA

Run on a real Linux desktop (X11 and Wayland) before each release, on at least the oldest
supported system (Ubuntu 22.04) and a current one (Fedora). Record the WebKitGTK version
(`apt policy libwebkit2gtk-4.1-0` or `rpm -q webkit2gtk4.1`) and the session type.

1. **First paint.** The window shows content with no blank flash; the custom title bar's
   window buttons (minimise, maximise, close) work and the bar drags the window.
2. **Themes.** Light, dark, and following the OS; switching live. With the OS set to "high
   contrast" or `prefers-contrast: more` borders stay visible in both themes.
3. **Tag colours.** All nine hues and their shades in the sidebar, the inspector and the file
   list (they use `color-mix()`); a tagged folder icon shows its tint.
4. **Scrolling.** A folder of 50,000 files scrolls at 60 fps in list and grid; the scrollbar is
   thin and themed; the tab strip with many tabs scrolls with no bar showing.
5. **Text.** File names with CJK, emoji and right-to-left text render and truncate; the
   system font is used and no text is clipped.
6. **Thumbnails and icons.** Pictures show thumbnails (check JPEG, PNG, GIF, WebP, SVG); other
   files show their icon.
7. **Drag and drop.** Drag a file to another Folio window, out to the desktop or Files, and in
   from Files; the drag image appears; copy and move follow Ctrl and Shift.
8. **Quick Look.** Space opens pictures, PDF, video, audio, text and Markdown; Esc and the
   arrow keys work; the PDF viewer renders pages.
9. **Menus and dialogs.** Context menus open at the pointer and stay on screen; dialogs trap
   focus and close with Esc; the command palette (Ctrl+K) filters as you type.
10. **Fractional scaling.** At 125%, 150% and 200% (and a HiDPI screen) icons and 1px borders
    stay sharp and nothing overlaps.
11. **Animation.** Opening a folder, the spring-loaded ring and Quick Look transitions play
    smoothly; with reduced motion on they are skipped.
12. **Keyboard.** Every shortcut in the list (F1) works; the keys follow Linux conventions
    (Ctrl, not Cmd).

If hardware acceleration is the problem (a blank or flickering window on some GPU drivers),
start with `WEBKIT_DISABLE_COMPOSITING_MODE=1` to confirm, and note the driver in the report.
Open an issue with the WebKitGTK version, the distro, the session type and a screenshot.

## Known limits

- The AppImage is not tested by CI; test it by hand on the QA machines.
- The CI start check shows the program installs, links and keeps running on a virtual screen
  with WebKit's sandbox off; it does not show the web view drew. That is the manual QA above.
- Package names such as `webp-pixbuf-loader` are recommendations: a distro without them just
  skips them, and WebP thumbnails then fall back to the file icon.
- Packages are not signed. The update files (`latest.json`) are. Which package kinds the in-app
  updater can replace on Linux is not checked yet; until it is, treat `.deb` and `.rpm` installs
  as updated by installing the new file.
