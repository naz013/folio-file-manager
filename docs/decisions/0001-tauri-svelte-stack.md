# ADR 0001 — Tauri 2 + Rust + SvelteKit

- **Status:** Accepted
- **Date:** 2026-10-30

## Context

Goals: lightweight (< 15 MB installer, < 80 MB idle), beautiful custom UI, Windows first,
macOS and Linux later, strong native file-system integration.

## Options considered

| Option                      | Pros                                                              | Cons                                                                                         |
| --------------------------- | ----------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| **Tauri 2 + Rust + web UI** | Small binaries, low memory, Rust for fast I/O, one UI on all OSes | System webviews differ (WebKitGTK weakest); native drag-out and shell menus need custom code |
| Electron                    | Mature, consistent Chromium                                       | ~150 MB, high memory — contradicts "lightweight"                                             |
| WinUI 3 / C#                | Most native on Windows                                            | Windows only (this is what Files uses, and it's heavy)                                       |
| Avalonia (C#)               | Cross-platform, good performance                                  | Smaller ecosystem for polished custom UI; .NET runtime size                                  |
| Flutter desktop             | Good UI toolkit                                                   | Weaker OS integration, larger binaries                                                       |
| Qt / QML                    | Powerful, native                                                  | Heavy toolchain, licensing considerations                                                    |
| egui / iced / Slint         | Pure Rust                                                         | Harder to reach a polished, Finder-like look                                                 |

## Decision

Tauri 2 with a Rust core and a SvelteKit SPA (adapter-static) frontend using Svelte 5 and
TypeScript.

## Consequences

- Native features (OLE drag & drop, `IContextMenu`, thumbnails) are implemented in Rust under
  `platform/` per OS. These are spiked in 0.1.
- Linux needs extra QA for WebKitGTK rendering and performance.
- The UI is plain CSS driven by tokens that mirror the Penpot design system.
