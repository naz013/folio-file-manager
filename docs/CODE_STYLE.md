# Folio — Code style and guidelines

Formatting and most rules are enforced by tools, so reviews can focus on design and
correctness. Run `npm run lint` before committing; `npm run format` fixes formatting.
CI currently runs the Rust checks (rustfmt, clippy) and svelte-check; adding `npm run lint`
to CI (issue #10) makes Prettier and ESLint blocking too.

| Area                                           | Standard                                                                                                                                                                 | Enforced by                         |
| ---------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ----------------------------------- |
| Rust formatting                                | [Rust Style Guide](https://doc.rust-lang.org/nightly/style-guide/)                                                                                                       | `rustfmt` (defaults)                |
| Rust lints                                     | [Clippy](https://doc.rust-lang.org/clippy/) `all` + `pedantic`                                                                                                           | `[lints]` in `src-tauri/Cargo.toml` |
| Rust API design                                | [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/)                                                                                                       | review                              |
| TS / Svelte / CSS / JSON / Markdown formatting | [Prettier](https://prettier.io/) + `prettier-plugin-svelte`                                                                                                              | `.prettierrc.json`                  |
| TS / Svelte lints                              | ESLint recommended, [typescript-eslint](https://typescript-eslint.io/) recommended, [eslint-plugin-svelte](https://sveltejs.github.io/eslint-plugin-svelte/) recommended | `eslint.config.js`                  |
| Types                                          | TypeScript `strict`                                                                                                                                                      | `npm run check` (svelte-check)      |
| Editors                                        | [EditorConfig](https://editorconfig.org/)                                                                                                                                | `.editorconfig`                     |
| Commits                                        | [How to write a Git commit message](https://cbea.ms/git-commit/)                                                                                                         | review                              |

Formatters wrap code at **100 columns** (rustfmt's default, mirrored in Prettier). Markdown
prose and long string literals are not rewrapped.

## Principles

- **Clear over clever.** Code is read far more than it is written. Prefer the obvious version.
- **Small, single-purpose functions.** If a function needs a comment per block, split it.
- **Names say what, comments say why.** Don't narrate the code; explain constraints, trade-offs
  and anything surprising (a Win32 quirk, a performance budget, an ADR).
- **No dead code.** No commented-out code, unused exports, or `TODO` without an issue number.
- **Fail loudly at boundaries, recover deliberately inside.** Errors reach the user as a clear
  message; nothing is swallowed silently except best-effort cleanup, which says so.
- **Follow the surrounding code.** Match the naming, structure and comment density of the file.

## Rust

- `rustfmt` defaults; never hand-format.
- Clippy `pedantic` is on. Silence a lint only at the narrowest scope, with
  `#[expect(lint, reason = "...")]` — `expect` warns once the lint no longer fires, `allow` doesn't.
  Crate-wide allowances live in `Cargo.toml` with a comment explaining why.
- **Errors:** library-style modules return `io::Result` or a typed error. Tauri commands return
  `Result<T, String>` and stay thin: validate, call a module, map the error (CLAUDE.md).
- **No panics in app code paths.** `unwrap`/`expect` only for invariants (with a message saying
  why it can't fail) and in tests. Prefer `?`, `ok_or`, `unwrap_or`.
- **Casts:** use `From`/`TryFrom` instead of `as` when a value could truncate, wrap or lose
  its sign.
- **`unsafe`:** keep blocks as small as practical. Every `unsafe` block has a `// SAFETY:`
  comment stating the invariant it relies on; every `unsafe fn` has a `# Safety` doc section.
- **Blocking work** (file I/O, shell calls) belongs off the main thread: make the command
  `async` and use the `blocking()` helper in `commands.rs`. Exceptions need the UI thread
  (the native context menu). `list_dir` and `open_path` predate this rule.
- **Docs:** `//!` at the top of each module says what it is for; `///` on items whose purpose
  or contract isn't obvious from the name and signature.
- **Tests** sit next to the code (`#[cfg(test)] mod tests`), use `tempfile` for anything on
  disk, and are named after the behavior: `move_into_keeps_source_when_copy_fails`.

## TypeScript

- `strict` TypeScript. No `any`; use `unknown` and narrow. No non-null `!` unless the
  invariant is local and obvious.
- `===` always (except `x == null`).
- Type-only imports use `import { type X }`.
- Functions over classes; small modules with named exports.
- Wrap Tauri `invoke` calls in typed functions in `src/lib/api/` — components never call
  `invoke` directly.
- Handle every promise: `await` it inside `try`, or end the chain with `.catch(...)`.

## Svelte 5

- Runes only: `$state`, `$derived`, `$effect`, `$props`, `$bindable`. No `$:`, no stores
  where runes fit.
- Prefer `$derived` to `$effect`. An effect is for syncing with the outside world (DOM, IPC,
  timers), not for computing state. Read dependencies explicitly; no bare-expression reads.
- `$state.raw` for large data that is replaced wholesale (folder listings).
- Props are typed inline in the `$props()` destructuring. Callback props are named `onx`
  (`onselect`, `onclose`), matching DOM events.
- Every `{#each}` has a key.
- `{@html}` only for trusted constants, with an `eslint-disable-next-line` and a comment.
- Component file order: `<script module>`, `<script>`, markup, `<style>`.
- Styles are component-scoped. Colors, radii, shadows and type come from
  `src/lib/styles/tokens.css` — no hard-coded colors (CLAUDE.md). Sizes taken from the
  Penpot design may be literal until a token exists.
- Accessibility: interactive elements are buttons or have a role, a label and keyboard
  support; svelte-check a11y warnings are fixed, not ignored.

## Files and naming

| Kind                  | Convention                            | Example          |
| --------------------- | ------------------------------------- | ---------------- |
| Rust modules          | `snake_case.rs`                       | `session.rs`     |
| Svelte components     | `PascalCase.svelte`                   | `TabBar.svelte`  |
| TS modules            | `camelCase.ts` / `kebab-case` folders | `api/fs.ts`      |
| TS values / functions | `camelCase`                           | `openTab`        |
| TS types              | `PascalCase`                          | `WindowTabs`     |
| Constants             | `UPPER_SNAKE_CASE`                    | `DRAG_THRESHOLD` |
| Tauri commands        | `snake_case`, verb first              | `save_tabs`      |

## Commits and branches

- Branch per issue: `feature/<milestone>-<slug>` (e.g. `feature/0.2-tabs`).
- Subject in the imperative mood, capitalized, no trailing period, ≤ 72 characters:
  "Add tabs in the title bar". The body explains what and why, wrapped at 72.
- Reference the issue: `Closes #11`.
- One logical change per commit; formatting-only changes go in their own commit.

## Before you commit

```
npm run format   # Prettier + rustfmt
npm run lint     # format check, ESLint, svelte-check, rustfmt check, clippy
cargo test --manifest-path src-tauri/Cargo.toml
```
