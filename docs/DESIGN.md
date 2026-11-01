# Folio — Design

The source of truth for visual design is the **Penpot** file
(currently named "New File 1" — rename it to _Folio — Design System & App_).

## Pages in the Penpot file

| Page          | Board                  | Contents                                                                                                                    |
| ------------- | ---------------------- | --------------------------------------------------------------------------------------------------------------------------- |
| Design System | Foundations            | Colors (light, dark, tags), typography, spacing, radius, elevation, icons                                                   |
| Design System | Components             | Thumbnails, Sidebar Item, Sidebar Tag, Tag Pill, Tab, Icon Button, View Switcher, Search Field, Button, File Tile, File Row |
| Design System | Patterns               | Overlays and feedback for 0.2–0.5 features (see below), plus their library components                                       |
| Screens       | Browser — Light / Dark | Main window: sidebar, tabs, toolbar, grid, inspector, status bar                                                            |
| Screens       | Cleanup — Light / Dark | Storage & Cleanup: donut, largest-folders treemap, suggestions, summary panel                                               |

The dark screens are recolored copies, not token-driven. In code, both themes come from the
same tokens. All text uses **Inter** (not Inter Tight; Penpot's name lookup returns Inter Tight
for "Inter", so pick the font by exact name).

### Patterns board

| Section          | Contents                                                                                                                     | Issue    |
| ---------------- | ---------------------------------------------------------------------------------------------------------------------------- | -------- |
| Drag & drop      | Folder tile: default, drop target, spring-loading ring, opening; breadcrumb, sidebar and tab spring states; auto-scroll edge | #67, #68 |
| Clipboard shelf  | Collapsed button with count bubble; Move/Copy toggle; expanded panel grouped by source folder with Paste                     | #69      |
| Operations queue | Status-bar pill; panel with running, paused, queued, done (Undo) and failed rows                                             | #14      |
| Toasts           | Success with Undo, Info, Error                                                                                               | —        |
| Dialogs & menus  | Conflict dialog (Keep both default, Replace → Recycle Bin); Folio context menu with tag row                                  | #15, #17 |
| Command palette  | Ctrl K: folder results, actions with shortcuts, keyboard hints                                                               | #23      |
| Quick Look       | Dark preview window with Open with, full screen, previous/next                                                               | #13      |

Library components added with the patterns:

| Component     | Variants                                     |
| ------------- | -------------------------------------------- |
| Toast         | Kind: Success, Info, Error                   |
| Menu Item     | State: Default, Hover, Disabled, Danger      |
| Shelf Item    | State: Default, Hover, Missing               |
| Operation Row | State: Running, Paused, Queued, Done, Failed |
| Shelf Button  | —                                            |
| Ops Pill      | —                                            |
| Kbd           | —                                            |

## Tokens

Penpot token sets: `core`, `mode/light`, `mode/dark` (themes **Mode / Light** and **Mode / Dark**).
They are mirrored in code in [`src/lib/styles/tokens.css`](../src/lib/styles/tokens.css).
Keep both in sync: change Penpot first, then the CSS.

### Color (light / dark)

| Token            | Light     | Dark      |
| ---------------- | --------- | --------- |
| `bg.window`      | `#FFFFFF` | `#1C1C1F` |
| `bg.sidebar`     | `#F4F4F7` | `#252529` |
| `bg.subtle`      | `#F7F7FA` | `#222226` |
| `bg.hover`       | `#ECECF1` | `#2E2E33` |
| `bg.selected`    | `#E8EEFE` | `#26315A` |
| `bg.control`     | `#EFEFF3` | `#2C2C31` |
| `border.default` | `#E7E7EC` | `#34343A` |
| `border.strong`  | `#D6D6DE` | `#44444C` |
| `text.primary`   | `#1D1D22` | `#F4F4F6` |
| `text.secondary` | `#6B6B76` | `#A4A4AE` |
| `text.tertiary`  | `#A3A3AD` | `#6E6E78` |
| `accent.default` | `#4D6BFE` | `#6C85FF` |
| `accent.hover`   | `#3F5BEA` | `#7F95FF` |
| `accent.subtle`  | `#E8EEFE` | `#26315A` |
| `folder.back`    | `#7CB7F7` | `#4E8FD9` |
| `folder.front`   | `#A3CDFB` | `#6FA8EA` |

### Tag colors (both themes)

red `#FF5F57` · orange `#FF9F0A` · yellow `#FFCC00` · green `#34C759` ·
blue `#0A84FF` · purple `#AF52DE` · gray `#8E8E93`

### Typography — Inter

| Style         | Size / line height | Weight             |
| ------------- | ------------------ | ------------------ |
| Display       | 28 / 34            | 700                |
| Heading       | 20 / 26            | 600                |
| Title         | 15 / 20            | 600                |
| Body          | 13 / 18            | 400                |
| Body Medium   | 13 / 18            | 500                |
| Caption       | 11 / 14            | 500                |
| Section Label | 11 / 14            | 600, +0.4 tracking |

### Spacing, radius, size, elevation

- Spacing (4 pt grid): 2, 4, 8, 12, 16, 20, 24, 32
- Radius: xs 4, sm 6, md 8, lg 12, xl 16, full 999, tab 9 (`radius.tab`, the Tab component)
- Size: titlebar 46 (`size.titlebar`, the Tab Bar and the sidebar brand row, which share one drag strip)
- Elevation: _raised_ (controls, active tab), _popover_ (menus), _window_

## UX rules

- One accent color. Tag colors are for tags only, never for UI chrome.
- Selection in the grid: grey thumbnail plate + accent name pill (Finder-style).
- Destructive actions name the exact outcome ("Move 9.9 GB to Recycle Bin").
- Section labels in the sidebar use Title case, tertiary color.
- Icons: 1.75–2 px stroke, rounded caps, 16 px in chrome, 20 px in docs.

## Accessibility

- Contrast (WCAG AA): every text token is at least 4.5:1 on every background token in both
  modes, and text on the accent is too. The values in `tokens.css` were changed for this
  (light: secondary `#5e5e69`, tertiary `#686873`, accent `#4561f2`, accent hover `#3650d8`;
  dark: secondary `#b0b0ba`, tertiary `#9c9ca7`, text on accent `#101014`). **Penpot still has
  the old values; update it to match.** Recheck the pairs when changing any color.
- Focus is always visible (2 px accent ring); never remove an outline without a replacement.
- Motion follows `prefers-reduced-motion`; high contrast follows `prefers-contrast: more` and
  Windows contrast themes (`forced-colors`), where selection and focus become system-color outlines.
- The file list is a focusable listbox (`aria-activedescendant` follows the keyboard cursor);
  buttons that only have an icon carry an `aria-label`.
