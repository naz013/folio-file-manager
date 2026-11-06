// Sets the app version from a release tag, so the tag is the only thing to bump.
// Usage: node scripts/set-version.mjs v1.2.3
// Edits package.json, src-tauri/tauri.conf.json, src-tauri/Cargo.toml and the folio entry of
// src-tauri/Cargo.lock. Only the release workflow runs it; do not commit the result.
import { readFileSync, writeFileSync } from "node:fs";

const tag = process.argv[2] ?? "";
const match = /^v(\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?)$/.exec(tag);
if (!match) {
  console.error(`tag "${tag}" is not a version like v1.2.3`);
  process.exit(1);
}
const version = match[1];

/** Replaces the first match of `pattern` (its first group is the text kept before the version). */
function edit(path, pattern) {
  const before = readFileSync(path, "utf8");
  if (!pattern.test(before)) {
    console.error(`${path}: no version found`);
    process.exit(1);
  }
  writeFileSync(path, before.replace(pattern, `$1${version}$2`));
  console.log(`${path} -> ${version}`);
}

edit("package.json", /^(\s*"version":\s*")[^"]+(")/m);
edit("src-tauri/tauri.conf.json", /^(\s*"version":\s*")[^"]+(")/m);
edit("src-tauri/Cargo.toml", /^(version\s*=\s*")[^"]+(")/m);
edit("src-tauri/Cargo.lock", /(name = "folio"\r?\nversion = ")[^"]+(")/);
