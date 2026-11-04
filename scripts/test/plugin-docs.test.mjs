import assert from "node:assert/strict";
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { test } from "node:test";

const repo = new URL("../../", import.meta.url);
const plugins = new URL("src-tauri/resources/plugins/", repo);
const pages = new URL("docs/plugins/", repo);
const ids = readdirSync(plugins, { withFileTypes: true })
  .filter((e) => e.isDirectory())
  .map((e) => e.name)
  .sort();
const manifest = (id) => JSON.parse(readFileSync(new URL(`${id}/plugin.json`, plugins), "utf8"));
const page = (id) => readFileSync(new URL(`${id}.md`, pages), "utf8");

test("every bundled plugin has a page, and every page has a plugin", () => {
  assert.ok(ids.length >= 6, "the bundled plugins were found");
  for (const id of ids) assert.ok(existsSync(new URL(`${id}.md`, pages)), `docs/plugins/${id}.md`);
  const written = readdirSync(pages)
    .filter((f) => f.endsWith(".md"))
    .map((f) => f.slice(0, -3))
    .sort();
  assert.deepEqual(written, ids, "no page for a plugin that is gone");
});

test("each page says what it takes, asks for, shows and limits, and shows its art", () => {
  for (const id of ids) {
    const m = manifest(id);
    const text = page(id);
    assert.ok(text.startsWith(`# ${m.name}\n`), `${id}: title is the plugin's name`);
    for (const h of [
      "File types",
      "What it asks for",
      "What it shows",
      "Limits",
      "How it is built",
      "Thumbnail",
    ]) {
      assert.match(text, new RegExp(`^## ${h}$`, "m"), `${id}: ${h}`);
    }
    for (const p of m.previews) {
      for (const ext of p.extensions) {
        assert.ok(text.includes(`\`.${ext}\``), `${id}: lists .${ext}`);
      }
    }
    for (const perm of m.permissions) assert.ok(text.includes(`\`${perm}\``), `${id}: ${perm}`);
    // The art is shown and linked, the file exists, and Penpot's place is named.
    for (const t of m.thumbnails) {
      assert.ok(existsSync(new URL(`${id}/${t.image}`, plugins)), `${id}: ${t.image} exists`);
      assert.ok(text.includes(`resources/plugins/${id}/${t.image}`), `${id}: links the art`);
      assert.ok(text.includes(t.color), `${id}: names the colour`);
    }
    assert.match(text, /!\[[^\]]*\]\(\.\.\/\.\.\/src-tauri\/resources\/plugins\//, id);
    assert.match(
      text,
      /Penpot, in Design System > Components > Thumbnails, in the row "Plugin thumbnails · bundled"/,
      `${id}: where the art is in Penpot`,
    );
  }
});

test("PLUGINS.md links every page, and the links lead somewhere", () => {
  const doc = readFileSync(new URL("docs/PLUGINS.md", repo), "utf8");
  for (const id of ids) assert.ok(doc.includes(`](plugins/${id}.md)`), `PLUGINS.md links ${id}`);
  // Every relative link in a page points at a file or folder that exists.
  for (const id of ids) {
    for (const [, target] of page(id).matchAll(/\]\((\.[^)#]*)\)/g)) {
      assert.ok(existsSync(new URL(target, pages)), `${id}: ${target}`);
    }
  }
});
