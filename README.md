# Folio

A beautiful, lightweight file manager. Windows first, then macOS and Linux.
Tabs, color tags, drag & drop, a space breakdown and safe cleanup — in a calm,
Finder-inspired UI.

> Status: **0.1 Skeleton** in progress.

## Documentation

| Doc                                  | What's inside                                            |
| ------------------------------------ | -------------------------------------------------------- |
| [Product](docs/PRODUCT.md)           | Vision, principles, performance budgets, feature catalog |
| [Roadmap](docs/ROADMAP.md)           | Releases 0.1 → 2.0 with exit criteria                    |
| [Architecture](docs/ARCHITECTURE.md) | Stack, module layout, key technical decisions            |
| [Design](docs/DESIGN.md)             | Penpot file map, tokens, UX rules                        |
| [Decisions](docs/decisions/)         | Architecture decision records (ADRs)                     |

## Tech stack

Tauri 2 · Rust · SvelteKit (Svelte 5, TypeScript) · SQLite (planned)

## Getting started

Prerequisites:

- Node.js 20+
- Rust (stable, via rustup)
- Windows: Microsoft C++ Build Tools and WebView2 (preinstalled on Windows 11)
- See [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for macOS/Linux

```bash
npm install
npm run tauri dev
```

Other scripts:

| Command                                           | Purpose                        |
| ------------------------------------------------- | ------------------------------ |
| `npm run check`                                   | Svelte + TypeScript type-check |
| `npm run build`                                   | Build the frontend only        |
| `npm run tauri build`                             | Build the installer            |
| `cargo test --manifest-path src-tauri/Cargo.toml` | Rust unit tests                |

## License

[GNU AGPL-3.0](LICENSE). You may use, modify and redistribute Folio, but any redistributed or
network-served version, modified or not, must stay under the AGPL with its source available.
Copyright (C) 2026 Nazar Sukhovych.
