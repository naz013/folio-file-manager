# Contributing to Folio

## Setup

Requirements: Node 22+, the Rust stable toolchain, and the
[Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for your OS.

```
npm install
npm run tauri dev
```

## Workflow

1. Pick an issue from the [Folio Roadmap](https://github.com/users/naz013/projects/3) project.
2. Branch from `main`: `feature/<milestone>-<slug>`.
3. Follow [docs/CODE_STYLE.md](docs/CODE_STYLE.md) and the rules in [CLAUDE.md](CLAUDE.md).
4. Before committing: `npm run format`, `npm run lint`, and
   `cargo test --manifest-path src-tauri/Cargo.toml`.
5. Commit with an imperative subject and `Closes #<n>` in the body.

## Branches and `main`

`main` is protected by the ruleset in [.github/rulesets/main.json](.github/rulesets/main.json):
no deletion, no force-push, linear history. Everyone except the repository admin changes it
through a pull request that has one approval from a code owner (`.github/CODEOWNERS`) and whose
`frontend`, `licenses`, `rust-linux`, `rust-windows` and `rust-macos` checks pass. The admin
may still push to `main` directly (the ruleset's bypass), which is how maintainer work lands:
commit on the feature branch, rebase onto `main`, fast-forward `main`, push.

Apply or refresh the ruleset (needs a public repository or GitHub Pro):

```
gh api -X POST repos/naz013/folio-file-manager/rulesets --input .github/rulesets/main.json
```

To update an existing one, use `PUT repos/naz013/folio-file-manager/rulesets/<id>` instead.

## Commands

| Command                                           | What it does                                                  |
| ------------------------------------------------- | ------------------------------------------------------------- |
| `npm run tauri dev`                               | Run the app                                                   |
| `npm run format`                                  | Format everything (Prettier + rustfmt)                        |
| `npm run lint`                                    | All checks: formatting, ESLint, svelte-check, rustfmt, clippy |
| `npm run lint:js` / `npm run lint:rust`           | Frontend or Rust checks only                                  |
| `npm run check`                                   | Type-check the frontend                                       |
| `cargo test --manifest-path src-tauri/Cargo.toml` | Rust tests                                                    |

## Docs

- [PRODUCT.md](docs/PRODUCT.md) — principles, features, performance budgets
- [ROADMAP.md](docs/ROADMAP.md) — milestones and exit criteria
- [ARCHITECTURE.md](docs/ARCHITECTURE.md) — module layout and decisions
- [DESIGN.md](docs/DESIGN.md) — design tokens; Penpot is the visual source of truth
- [CODE_STYLE.md](docs/CODE_STYLE.md) — code standards
- [AGENTS.md](docs/AGENTS.md) — how Claude Code agents work on issues
