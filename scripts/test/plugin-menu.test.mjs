import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import {
  actionsFor,
  groupActions,
  load,
  parseManifest,
  PLUGIN_HEADING,
} from "../../src/lib/plugins.ts";

const manifest = (id, name, actions) =>
  JSON.stringify({
    id,
    name,
    version: "1.0.0",
    description: "d",
    permissions: ["selection"],
    actions,
  });

const list = load(
  [
    {
      id: "a",
      manifest: manifest("a", "Alpha tools", [
        { id: "x", title: "Move to Recycle Bin", extensions: ["txt"] },
      ]),
      error: null,
      digest: "d1",
    },
    {
      id: "b",
      manifest: manifest("b", "Beta kit", [{ id: "y", title: "Rename", extensions: ["txt"] }]),
      error: null,
      digest: "d2",
    },
  ],
  {
    a: { version: "1.0.0", permissions: ["selection"], digest: "d1" },
    b: { version: "1.0.0", permissions: ["selection"], digest: "d2" },
  },
);
const file = { path: "C:\\a.txt", name: "a.txt", isDir: false };

test("plugin actions form one group under a heading, each with its plugin's name", () => {
  const group = groupActions(actionsFor(list, [file]));
  assert.equal(group.heading, PLUGIN_HEADING);
  assert.equal(group.heading, "Plugins");
  assert.deepEqual(
    group.entries.map((e) => [e.title, e.source]),
    [
      // An action named like a built-in command is still listed as the plugin's.
      ["Move to Recycle Bin", "Alpha tools"],
      ["Rename", "Beta kit"],
    ],
  );
});

test("there is no group when no action fits", () => {
  assert.equal(groupActions([]), null);
  assert.equal(groupActions(actionsFor(list, [])), null);
  assert.equal(groupActions(actionsFor(list, [{ ...file, name: "a.png" }])), null);
});

test("the menu puts plugin actions last, after a divider, and labels the group", () => {
  const page = readFileSync(new URL("../../src/routes/+page.svelte", import.meta.url), "utf8");
  const menu = page.slice(
    page.indexOf("function itemMenu()"),
    page.indexOf("function backgroundMenu()"),
  );
  const more = menu.indexOf('label: "More options"');
  const plugins = menu.indexOf("...pluginItems()");
  assert.ok(
    more > 0 && plugins > more,
    "after Folio's own items, including the bin and More options",
  );
  assert.ok(menu.indexOf("Move to Recycle Bin") < plugins);
  const helper = page.slice(
    page.indexOf("function pluginItems()"),
    page.indexOf("// ---- Second pane"),
  );
  assert.match(helper, /"separator",\s*\{\s*group:/, "a divider, then the group");
  const component = readFileSync(
    new URL("../../src/lib/components/ContextMenu.svelte", import.meta.url),
    "utf8",
  );
  assert.match(component, /role="group" aria-label=\{item\.group\}/);
  assert.match(
    component,
    /aria-label=\{item\.source \? `\$\{item\.label\}, from \$\{item\.source\}`/,
  );
});

test("a name or title with line breaks or direction marks is refused by the manifest", () => {
  const m = (name, title) =>
    parseManifest("b", manifest("b", name, [{ id: "y", title, extensions: ["txt"] }]));
  assert.ok(m("Beta kit", "Rename").ok);
  assert.equal(
    m("Beta" + String.fromCharCode(10) + String.fromCharCode(0x202e) + " kit", "Rename").ok,
    false,
  );
  assert.equal(m("Beta kit", "Rename" + String.fromCharCode(10) + "Delete").ok, false);
});
