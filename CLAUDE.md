# Folio — notes for Claude

Cross-platform file manager (Windows first). Tauri 2 + Rust core + SvelteKit SPA (Svelte 5, TS).

## Read first

- `docs/PRODUCT.md` — principles and P0/P1/P2 feature catalog
- `docs/ROADMAP.md` — current milestone and exit criteria
- `docs/ARCHITECTURE.md` — module layout and decisions
- `docs/DESIGN.md` — tokens; Penpot is the visual source of truth
- `docs/CODE_STYLE.md` — code standards (rustfmt + Clippy pedantic, Prettier + ESLint, Svelte 5)

## Rules

- All mutating file operations go through the Rust core, never directly from the UI.
- Deletes go to the OS trash. Never add permanent delete to cleanup (ADR 0003).
- UI colors, spacing and type come from `src/lib/styles/tokens.css` — no hard-coded colors in components.
- Svelte 5 runes (`$state`, `$derived`, `$props`), TypeScript everywhere.
- Rust commands return `Result<T, String>` (or a serializable error) and stay thin; logic lives in modules.
- Respect performance budgets in PRODUCT.md; large lists must be virtualized.
- Code follows `docs/CODE_STYLE.md`; `npm run lint` must pass before a commit.
- Silence a lint only at the narrowest scope, with a reason (`#[expect(..., reason)]`,
  `eslint-disable-next-line` + comment). Every `unsafe` block has a `// SAFETY:` comment.

## Agents

- Work is one GitHub issue at a time from the Folio Roadmap project: `/work-issue <n>`.
- `docs/AGENTS.md` — lead vs specialist roles, routing by `area:` label, when to fan out.
- Specialists in `.claude/agents/`: `reviewer` (every change), `safety-reviewer`, `test-writer`, `perf-auditor`.
- Specialists never delegate further, commit, or write to GitHub; the lead does.
- Before running several agents in parallel, list the phases and agent count and wait for a go-ahead.

## Commands

- `npm run tauri dev` — run the app
- `npm run format` — format everything (Prettier + rustfmt)
- `npm run lint` — all checks: Prettier, ESLint, svelte-check, rustfmt, clippy (`-D warnings`)
- `npm run check` — type-check frontend only
- `npm run test:js` — frontend unit tests (Node runs them directly, no framework)
- `npm run linux -- <fmt|clippy|test|all|shell>` — Linux Rust checks in Docker (mirrors the rust-linux CI job)
- `cargo test --manifest-path src-tauri/Cargo.toml` — Rust tests
