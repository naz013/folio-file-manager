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

## Interaction states

Every button has the same states, from `src/lib/styles/global.css`; a new button gets them
without any CSS of its own.

| State    | Neutral button                               | Filled accent button  |
| -------- | -------------------------------------------- | --------------------- |
| Hover    | `bg.hover` laid over its own fill            | `accent.hover` fill   |
| Pressed  | `bg.pressed` laid over its own fill          | `accent.pressed` fill |
| Focus    | 2 px `focus.ring` outline, 1 px offset       | same                  |
| Disabled | `opacity.disabled` (0.45), no hover or press | same                  |

- A button that is on (`aria-pressed`, `.on`, `.active`, `.selected`) keeps its own look on hover.
- A filled accent button must carry one of the classes named in `global.css`
  (`primary`, `done`, `apply`, `go`, `add`) to get the accent states; a filled destructive
  button takes `danger-fill`. The shared rules are low in specificity (`:where`), so a
  component can override a state for itself.
- Tokens: `bg.pressed`, `accent.pressed`, `danger.hover`, `danger.pressed`, `focus.ring`,
  `opacity.disabled` (CSS: `--bg-pressed`, `--accent-pressed`, `--bg-danger-hover`,
  `--bg-danger-pressed`, `--focus-ring`, `--opacity-disabled`). They are in the Penpot token sets.
- Transitions use `--duration-fast` and `--ease-standard`, and are off under reduced motion.
- **Penpot:** Button has a State variant for Primary, Secondary and Danger (Default, Hover,
  Pressed, Focus, Disabled) and Icon Button has Focus. Danger and the Focus rings are drawn with
  the values above, in a 3 × 5 grid (kind by state) on the Components board.

## Motion

Motion tells you what changed; it is never a wait. Tokens (`tokens.css`): `--duration-fast`
120 ms, `--duration-base` 200 ms, `--duration-slow` 320 ms; `--ease-standard` for things that
change in place, `--ease-emphasized` for things that arrive; `--motion-shift` (6 px) and
`--motion-scale` (0.97) for how far an arriving thing travels. No component hard-codes a timing.

| What                                    | Motion                                          |
| --------------------------------------- | ----------------------------------------------- |
| Dialog scrim, `.backdrop`               | fade, base                                      |
| Dialog (`.backdrop > [role=dialog]`)    | fade + scale + rise, base, emphasized           |
| Menus (`[role=menu]`)                   | fade + scale + drop, fast, emphasized           |
| Toast, operations panel (`.motion-pop`) | as a dialog                                     |
| Tab opening (`.motion-fade`)            | fade; tabs reorder and close with a glide, fast |
| Selection, hover and switches           | colour fades, fast                              |
| Grid and list switch                    | the list fades in once, base                    |
| Progress bar                            | `scaleX`, base                                  |

- Only `opacity`, `scale`, `translate` and `transform` animate, so nothing re-lays-out.
- Things only animate **in**; leaving is instant. Closing and keyboard actions never wait, and an
  animation that is interrupted just stops.
- Items of the virtual file list do not animate as they scroll in; a folder over
  2000 entries skips even the view-switch fade (`LIST_ANIMATION_MAX`).
- Off under `prefers-reduced-motion`, or Settings › Motion › Off. "Always on" overrides the OS.
  The spring-loading ring is a countdown, not decoration, and keeps its timing.
- **Penpot:** the Motion board on the Design System page (right of Patterns) lists the tokens and
  the table above as text. Penpot cannot play motion, so it is a spec sheet, not a prototype.

## UX rules

- One accent color. Tag colors are for tags only, never for UI chrome.
- The Tags screen offers nine hues in five shades each (`--tag-<hue>-1` lightest to `-5` darkest; shade 3 is the hue, stored as its plain name). The tag dialog and the keys 1 to 7 keep the seven base colours. Mirror any change in Penpot first.
- Selection in the grid: grey thumbnail plate + accent name pill (Finder-style).
- Quick Look shows a **source bar** between the preview and the footer when a plugin draws it: "Preview by <name>", a badge (grey "Built-in plugin", amber "Plugin") and a "Show normally" button. Folio draws it, so a plugin can't pass off its page as Folio's own. It is in Penpot under Patterns > Command palette & Quick Look, as "Quick Look · plugin preview".
- File thumbnails are a 56 x 72 page with a folded corner, the file's content hinted in the middle, and a coloured badge with the extension in the lower left. Plugins may give the page art (an SVG, no badge) for their kinds of file; Folio draws the badge. The bundled ones are in Penpot under Components > Thumbnails, in the row "Plugin thumbnails · bundled".
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
