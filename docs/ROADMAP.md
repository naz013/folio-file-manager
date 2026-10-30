# Folio — Roadmap

Strategy: ship a fast, trustworthy **Windows browser** first, then the **differentiators**
(tags, space breakdown, cleanup), then **cross-platform**.

Estimates assume one full-time developer; roughly double them for part-time work.
Total: ~5–6 months to Windows 1.0, ~7–8 months to cross-platform.

Priorities (P0/P1/P2) refer to [PRODUCT.md](PRODUCT.md).

---

## 0.1 — Skeleton (~3 weeks) · audience: you

**Theme:** prove the stack and retire the scariest risks early.

- Tauri 2 + SvelteKit app shell with the design tokens
- Sidebar, grid & list views with virtualized rendering
- Open, copy, move, delete-to-trash (Rust core, background tasks)
- File system watching (`notify`)
- **Spikes:** native drag-out to Explorer, native shell context menu, Windows thumbnails

**Exit criteria**
- 50,000-file folder scrolls at 60 fps
- Idle memory and cold-start budgets met
- Drag-out spike works with Explorer

## 0.2 — Alpha (~5 weeks) · audience: friends

**Theme:** a browser you can use every day.

- Tabs, breadcrumb path bar, Quick Look
- Operations queue (progress, pause, cancel)
- Conflict dialog with "apply to all"
- **Undo / redo** for move, rename, delete, new folder
- Context menu (own + native "More options")
- Drag & drop inside the app and to/from other apps
- Light / dark / system theme

**Exit criteria**
- You use it as your main file manager for one week
- File-operation engine passes the integration test suite (conflicts, locked files,
  long paths, disk full)

## 0.5 — Beta (~5 weeks) · audience: public beta, Windows

**Theme:** organization.

- Color + named tags, tag filters (sidebar + header chips)
- Ctrl K command palette
- Inspector panel
- Settings
- Auto-update, code-signed installer
- Opt-in crash reporting

**Exit criteria**
- Zero known data-loss bugs
- Crash-free sessions > 99%

## 1.0 — Launch (~5 weeks) · audience: public, Windows

**Theme:** storage & cleanup.

- Space breakdown: donut + treemap, drill-down
- Folder sizes in list view
- Cleanup: large & old files, old downloads, duplicates review
- Onboarding, accessibility pass, landing page

**Exit criteria**
- Cleanup safety review passed: trash-only, review step everywhere, validated against a
  known test corpus with no false positives in protected paths

## 1.1 — Cross-platform (~6 weeks) · audience: macOS + Linux

- macOS and Linux builds and installers
- Native tag sync (Finder tags, `user.xdg.tags`)
- Per-OS menus, shortcuts, path conventions

**Exit criteria:** all 1.0 features at parity on all three platforms.

## 1.2 — Power features · audience: all

Prioritized by user feedback:

- Column view
- Batch rename
- Recursive search
- Zip compress / extract
- Temp & cache cleanup
- Network drives and OneDrive placeholders

## 2.0 — Expansion

- Gallery view
- Smart folders
- Dual pane
- MFT fast scan (opt-in, admin)
- Plugin API

---

## Key risks

| Risk | Mitigation |
|---|---|
| Native integration (drag-out, shell menu, thumbnails) is where Tauri apps struggle | Spike all three in 0.1 |
| Data loss in copy/move | Build and test the operations engine headless before UI depends on it |
| Cleanup false positives | Suggestions only, review step, trash-only, protected-path deny list |
| WebKitGTK quirks on Linux | Budget extra QA time in 1.1 |
| Scope creep | "Out of scope" list in PRODUCT.md is binding |
