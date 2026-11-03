# Releasing

Folio updates itself from GitHub Releases. Every update file is signed with a key only the
maintainer holds; the app refuses anything that does not verify against the public key built
into it, so a hijacked download or release page cannot install code.

## One-time setup

1. Make the key pair (keep the password):

   ```
   npm run tauri signer generate -- -w ~/.tauri/folio.key
   ```

2. Put the printed **public** key in `src-tauri/tauri.conf.json` under `plugins.updater.pubkey`.
   Commit it. Until you do, update checks fail quietly.
3. In the GitHub repository settings add two Actions secrets:
   `TAURI_SIGNING_PRIVATE_KEY` (the contents of `folio.key`) and
   `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.
4. Back up `folio.key` and its password somewhere safe. **If the key is lost, installed copies
   can never be updated again**, because they trust only the old public key; users would have to
   install a new version by hand.

The update endpoint is `releases/latest/download/latest.json` of the public releases
repository named in `tauri.conf.json`. A private repository does not work: the app cannot sign
in to download from it.

## Code signing (Windows)

An unsigned installer makes SmartScreen warn users. Signing is automatic when two more Actions
secrets exist: `WINDOWS_CERTIFICATE` (the `.pfx` file, base64: `[Convert]::ToBase64String([IO.File]::ReadAllBytes("cert.pfx"))`)
and `WINDOWS_CERTIFICATE_PASSWORD`. Without them the release still builds, unsigned, with a
warning in the log. With them the workflow signs `folio.exe` and the installer with a timestamp
and fails the build if either signature is not valid.

This covers an OV or EV certificate exported as a `.pfx`. Most EV certificates and Azure Trusted
Signing cannot be exported; for those, replace the "Prepare code signing" step with one that
sets `bundle.windows.signCommand` in `tauri.signing.conf.json` (see the Tauri Windows signing
guide) and keep the checks that follow. A new certificate starts with no SmartScreen reputation
unless it is EV.

The workflow also fails if the installer is 15 MB or more.

## Test builds

Every push to `main` also builds an unsigned installer and `folio.exe` in the **CI** workflow
(job `build-windows`, after the checks pass). Download them from the run page, under Artifacts;
they are kept for 14 days. They are for trying a commit, not for users: no signing, no update files.

## Third-party licenses

`scripts/licenses.mjs` reads cargo and npm metadata for what is built into the Windows app
(production npm packages, normal Rust dependencies) and writes `src/lib/licenses.json` (About ›
Open-source licenses) and `src-tauri/resources/THIRD-PARTY-LICENSES.txt` (installed next to
the app). Both are committed as empty stubs; the **Release** workflow and the CI build job
regenerate them, and the CI `licenses` job runs `npm run licenses:check` on every push.

- The build fails when a dependency has no license or one that is not in
  `scripts/licenses-policy.mjs`. Allowing a new license is a decision: add it there, with a reason.
- A package that ships no license file gets the standard text of its license with the authors it
  lists (a warning says so); if that cannot be made, the build fails.
- Do not commit the generated files; `git checkout` them after a local run.

## Making a release

1. Set the new version in `package.json`, `src-tauri/Cargo.toml` and `src-tauri/tauri.conf.json`.
2. Optional: write the release notes as the body of a draft release for the tag first. The app shows them once after an update, so write for users. Without one, GitHub's generated notes are used, and they only list merged pull requests: if there are none, the app shows no notes. Do not edit the draft after pushing the tag; the app carries the text as it was then.
3. Commit, then tag and push: `git tag v0.5.0 && git push origin main v0.5.0`.
4. The **Release** workflow builds the installer and `latest.json` into a **draft** release.
5. Install the draft's installer on a clean Windows machine and check it starts. Then publish
   the release. Running copies find it within a day.

## How the app behaves

- It checks in the background about 15 seconds after start, at most once a day (Settings →
  Updates turns this off). Offline or no release is silent.
- A small banner offers "Update and restart" or "Later". "Later" is remembered for that version.
- Nothing is installed while a copy, move or delete is running, or without a click.
- The download's signature is verified before anything is replaced; a bad download leaves the
  installed version as it was and the banner says so. The busy check runs again after the
  download, right before the installer starts. The updater does no rollback of its own: if
  the Windows installer itself fails after Folio has closed, reinstall from the release page.
- A build made without `--config src-tauri/tauri.release.conf.json` makes no update files, so
  local `tauri build` needs no key.
