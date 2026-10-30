# Folio — notes for Claude

Cross-platform file manager (Windows first). Tauri 2 + Rust core + SvelteKit SPA (Svelte 5, TS).

## Read first
- `docs/PRODUCT.md` — principles and P0/P1/P2 feature catalog
- `docs/ROADMAP.md` — current milestone and exit criteria
- `docs/ARCHITECTURE.md` — module layout and decisions
- `docs/DESIGN.md` — tokens; Penpot is the visual source of truth

## Rules
- All mutating file operations go through the Rust core, never directly from the UI.
- Deletes go to the OS trash. Never add permanent delete to cleanup (ADR 0003).
- UI colors, spacing and type come from `src/lib/styles/tokens.css` — no hard-coded colors in components.
- Svelte 5 runes (`$state`, `$derived`, `$props`), TypeScript everywhere.
- Rust commands return `Result<T, String>` (or a serializable error) and stay thin; logic lives in modules.
- Respect performance budgets in PRODUCT.md; large lists must be virtualized.

## Commands
- `npm run tauri dev` — run the app
- `npm run check` — type-check frontend
- `cargo test --manifest-path src-tauri/Cargo.toml` — Rust tests
- `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` — lint
