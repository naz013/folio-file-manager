# Folio — Design

The source of truth for visual design is the **Penpot** file
(currently named "New File 1" — rename it to *Folio — Design System & App*).

## Pages in the Penpot file

| Page | Board | Contents |
|---|---|---|
| Design System | Foundations | Colors (light, dark, tags), typography, spacing, radius, elevation, icons |
| Design System | Components | Thumbnails, Sidebar Item, Sidebar Tag, Tag Pill, Tab, Icon Button, View Switcher, Search Field, Button, File Tile, File Row |
| Screens | Browser — Light / Dark | Main window: sidebar, tabs, toolbar, grid, inspector, status bar |
| Screens | Cleanup — Light / Dark | Storage & Cleanup: donut, largest-folders treemap, suggestions, summary panel |

The dark screens are recolored copies, not token-driven. In code, both themes come from the
same tokens.

## Tokens

Penpot token sets: `core`, `mode/light`, `mode/dark` (themes **Mode / Light** and **Mode / Dark**).
They are mirrored in code in [`src/lib/styles/tokens.css`](../src/lib/styles/tokens.css).
Keep both in sync: change Penpot first, then the CSS.

### Color (light / dark)

| Token | Light | Dark |
|---|---|---|
| `bg.window` | `#FFFFFF` | `#1C1C1F` |
| `bg.sidebar` | `#F4F4F7` | `#252529` |
| `bg.subtle` | `#F7F7FA` | `#222226` |
| `bg.hover` | `#ECECF1` | `#2E2E33` |
| `bg.selected` | `#E8EEFE` | `#26315A` |
| `bg.control` | `#EFEFF3` | `#2C2C31` |
| `border.default` | `#E7E7EC` | `#34343A` |
| `border.strong` | `#D6D6DE` | `#44444C` |
| `text.primary` | `#1D1D22` | `#F4F4F6` |
| `text.secondary` | `#6B6B76` | `#A4A4AE` |
| `text.tertiary` | `#A3A3AD` | `#6E6E78` |
| `accent.default` | `#4D6BFE` | `#6C85FF` |
| `accent.hover` | `#3F5BEA` | `#7F95FF` |
| `accent.subtle` | `#E8EEFE` | `#26315A` |
| `folder.back` | `#7CB7F7` | `#4E8FD9` |
| `folder.front` | `#A3CDFB` | `#6FA8EA` |

### Tag colors (both themes)

red `#FF5F57` · orange `#FF9F0A` · yellow `#FFCC00` · green `#34C759` ·
blue `#0A84FF` · purple `#AF52DE` · gray `#8E8E93`

### Typography — Inter

| Style | Size / line height | Weight |
|---|---|---|
| Display | 28 / 34 | 700 |
| Heading | 20 / 26 | 600 |
| Title | 15 / 20 | 600 |
| Body | 13 / 18 | 400 |
| Body Medium | 13 / 18 | 500 |
| Caption | 11 / 14 | 500 |
| Section Label | 11 / 14 | 600, +0.4 tracking |

### Spacing, radius, elevation

- Spacing (4 pt grid): 2, 4, 8, 12, 16, 20, 24, 32
- Radius: xs 4, sm 6, md 8, lg 12, xl 16, full 999
- Elevation: *raised* (controls, active tab), *popover* (menus), *window*

## UX rules

- One accent color. Tag colors are for tags only, never for UI chrome.
- Selection in the grid: grey thumbnail plate + accent name pill (Finder-style).
- Destructive actions name the exact outcome ("Move 9.9 GB to Recycle Bin").
- Section labels in the sidebar use Title case, tertiary color.
- Icons: 1.75–2 px stroke, rounded caps, 16 px in chrome, 20 px in docs.
