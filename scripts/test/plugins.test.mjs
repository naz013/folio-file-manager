import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import {
  actionsFor,
  cleanGrants,
  createBridge,
  isGranted,
  isReady,
  load,
  parseManifest,
  previewFor,
} from "../../src/lib/plugins.ts";

const good = {
  id: "demo",
  name: "Demo",
  version: "1.0.0",
  description: "Does things",
  permissions: ["selection", "read", "notify"],
  actions: [
    { id: "count", title: "Count words", extensions: ["TXT", ".md"], multiple: true },
    { id: "tidy", title: "Tidy", kinds: ["folder"] },
  ],
  previews: [{ id: "csv", title: "Table", extensions: ["csv"] }],
};

const parse = (obj, id = "demo") => parseManifest(id, JSON.stringify(obj));

test("a good manifest is read, with defaults and normalised extensions", () => {
  const r = parse(good);
  assert.ok(r.ok);
  assert.equal(r.manifest.entry, "index.html");
  assert.deepEqual(r.manifest.actions[0].extensions, ["txt", "md"]);
  assert.deepEqual(r.manifest.actions[0].kinds, ["file"]);
  assert.equal(r.manifest.actions[1].multiple, false);
  assert.deepEqual(r.manifest.previews[0].extensions, ["csv"]);
});

test("a manifest with something wrong says what, and unknown fields are ignored", () => {
  const bad = (change, text) => {
    const r = parse({ ...good, ...change });
    assert.ok(!r.ok, text);
    assert.match(r.error, new RegExp(text), r.error);
  };
  assert.ok(parse({ ...good, future: { thing: 1 } }).ok);
  assert.ok(!parseManifest("demo", "{nope").ok);
  assert.ok(!parseManifest("demo", "[]").ok);
  bad({ id: "other" }, "folder");
  bad({ name: "" }, "name");
  bad({ name: "x".repeat(61) }, "name");
  bad({ version: undefined }, "version");
  bad({ api: 2 }, "newer Folio");
  bad({ api: "1" }, "api");
  bad({ entry: "../x.html" }, "entry");
  bad({ entry: "/x.html" }, "entry");
  bad({ entry: "a\\b.html" }, "entry");
  bad({ entry: "main.js" }, "entry");
  bad({ permissions: ["write"] }, "Unknown permission");
  bad({ permissions: "read" }, "permissions");
  bad({ permissions: ["read", "notify"] }, "selection");
  bad({ actions: [{ id: "A b", title: "x" }], permissions: ["selection"] }, "id");
  bad(
    {
      actions: [
        { id: "a", title: "x" },
        { id: "a", title: "y" },
      ],
      permissions: ["selection"],
    },
    "Two actions",
  );
  bad({ actions: [{ id: "a", title: "x", kinds: ["disk"] }], permissions: ["selection"] }, "kinds");
  bad(
    { actions: [{ id: "a", title: "x", extensions: ["a b"] }], permissions: ["selection"] },
    "extensions",
  );
  bad({ previews: [{ id: "p", title: "x", extensions: [] }] }, "extensions");
  bad({ previews: [{ id: "p", title: "x\ny", extensions: ["a"] }] }, "title");
});

test("a plugin is on only for the version and permissions that were agreed to", () => {
  const { manifest } = parse(good);
  const grant = { version: "1.0.0", permissions: ["selection", "read", "notify"] };
  assert.ok(isGranted(manifest, grant));
  assert.ok(!isGranted(manifest, undefined));
  assert.ok(!isGranted(manifest, { ...grant, version: "1.0.1" }));
  assert.ok(!isGranted(manifest, { ...grant, permissions: ["selection", "read"] }));
  // A plugin that asks for less than was granted stays on.
  const small = parse({ ...good, permissions: ["selection"] }).manifest;
  assert.ok(isGranted(small, grant));
});

test("saved grants are cleaned of anything that isn't one", () => {
  assert.deepEqual(cleanGrants(null), {});
  assert.deepEqual(
    cleanGrants({
      a: { version: "1", permissions: ["read", "root"] },
      b: { version: 2, permissions: [] },
      c: "x",
    }),
    { a: { version: "1", permissions: ["read"] } },
  );
});

test("the list shows enabled, changed, broken and manifest-less plugins", () => {
  const grants = { demo: { version: "1.0.0", permissions: ["selection", "read", "notify"] } };
  const text = JSON.stringify(good);
  const [on, changed, broken, none] = load(
    [
      { id: "demo", manifest: text, error: null },
      {
        id: "old",
        manifest: text.replace('"demo"', '"old"').replace("1.0.0", "2.0.0"),
        error: null,
      },
      { id: "bad", manifest: "{}", error: null },
      { id: "empty", manifest: null, error: "No plugin.json" },
    ],
    { ...grants, old: { version: "1.0.0", permissions: [] } },
  );
  assert.ok(on.enabled && !on.changed);
  assert.ok(!changed.enabled && changed.changed);
  assert.ok(broken.error && !broken.enabled);
  assert.equal(none.error, "No plugin.json");
});

function plugins() {
  const grants = { demo: { version: "1.0.0", permissions: ["selection", "read", "notify"] } };
  return load([{ id: "demo", manifest: JSON.stringify(good), error: null }], grants);
}

const file = (name) => ({ path: `C:\\x\\${name}`, name, isDir: false });

test("an action is offered for matching files, folders and counts only", () => {
  const p = plugins();
  const ids = (items) => actionsFor(p, items).map((h) => h.action.id);
  assert.deepEqual(ids([file("a.txt")]), ["count"]);
  assert.deepEqual(ids([file("a.MD"), file("b.txt")]), ["count"]);
  assert.deepEqual(ids([file("a.png")]), []);
  assert.deepEqual(ids([file("a.txt"), file("b.png")]), []);
  assert.deepEqual(ids([{ path: "C:\\x", name: "x", isDir: true }]), ["tidy"]);
  // "tidy" is one folder at a time.
  assert.deepEqual(
    ids([
      { path: "C:\\x", name: "x", isDir: true },
      { path: "C:\\y", name: "y", isDir: true },
    ]),
    [],
  );
  assert.deepEqual(ids([]), []);
  assert.deepEqual(ids([file(".txt")]), [], "a name that is only an extension has none");
});

test("a plugin that is off offers nothing", () => {
  const off = load([{ id: "demo", manifest: JSON.stringify(good), error: null }], {});
  assert.deepEqual(actionsFor(off, [file("a.txt")]), []);
  assert.equal(previewFor(off, file("a.csv")), null);
});

test("a preview is found by extension, for files only", () => {
  const p = plugins();
  assert.equal(previewFor(p, file("data.CSV"))?.preview.id, "csv");
  assert.equal(previewFor(p, file("data.txt")), null);
  assert.equal(previewFor(p, { path: "C:\\d.csv", name: "d.csv", isDir: true }), null);
});

function bridge(perms, extra = {}) {
  const shown = [];
  const read = [];
  let clock = 0;
  const manifest = parse({ ...good, permissions: perms, actions: [] }).manifest;
  const b = createBridge(manifest, {
    notify: (m) => shown.push(m),
    readText: async (p) => {
      read.push(p);
      return { text: `text of ${p}`, truncated: false };
    },
    now: () => clock,
    ...extra,
  });
  return { b, shown, read, tick: (ms) => (clock += ms) };
}
const call = (method, params, id = 1) => ({ folio: 1, id, method, params });

test("a call is answered with its own id; what is not a call is ignored", async () => {
  const { b } = bridge(["selection"]);
  b.allow(["C:\\a"]);
  assert.deepEqual(await b.handle(call("selection", undefined, 7)), {
    folio: 1,
    id: 7,
    result: ["C:\\a"],
  });
  for (const junk of [
    null,
    5,
    "x",
    {},
    { id: 1, method: "selection" },
    { folio: 1, id: "1", method: "x" },
  ]) {
    assert.equal(await b.handle(junk), null);
  }
  assert.ok(isReady({ folio: 1, event: "ready" }));
  assert.ok(!isReady({ event: "ready" }));
});

test("a method needs its permission, and an unknown one is an error", async () => {
  const { b, shown, read } = bridge([]);
  b.allow(["C:\\a"]);
  for (const [method, params] of [
    ["selection"],
    ["notify", { message: "hi" }],
    ["read", { path: "C:\\a" }],
  ]) {
    const r = await b.handle(call(method, params));
    assert.match(r.error, /not given/, method);
  }
  assert.match((await b.handle(call("write", { path: "C:\\a" }))).error, /Unknown method/);
  assert.deepEqual([shown, read], [[], []]);
});

test("a plugin reads the files it was handed and no others", async () => {
  const { b, read } = bridge(["read"]);
  b.allow(["C:\\a.txt"]);
  const ok = await b.handle(call("read", { path: "C:\\a.txt" }));
  assert.deepEqual(ok.result, { text: "text of C:\\a.txt", truncated: false });
  for (const path of ["C:\\b.txt", "C:\\a.txt\\..\\b.txt", "c:\\a.txt", 5, undefined]) {
    const r = await b.handle(call("read", { path }));
    assert.match(r.error, /not given|was not/, String(path));
  }
  assert.deepEqual(read, ["C:\\a.txt"]);
  // Runs that overlap don't undo each other, but old ones are forgotten in the end.
  b.allow(["C:\\b.txt"]);
  assert.equal((await b.handle(call("read", { path: "C:\\a.txt" }))).error, undefined);
  for (let i = 0; i < 5; i++) b.allow([`C:\\later${i}`]);
  assert.ok((await b.handle(call("read", { path: "C:\\a.txt" }))).error);
});

test("a plugin id like an object property is not taken for a saved grant", () => {
  const found = [
    { id: "constructor", manifest: JSON.stringify({ ...good, id: "constructor" }), error: null },
  ];
  const [p] = load(found, {});
  assert.ok(!p.enabled && !p.changed);
  assert.deepEqual(
    cleanGrants(JSON.parse('{"__proto__": {"version": "1", "permissions": []}}')),
    {},
  );
});

test("a failed read is an error answer, not a crash", async () => {
  const { b } = bridge(["read"], {
    readText: async () => {
      throw "Only in the cloud";
    },
  });
  b.allow(["C:\\a"]);
  assert.equal((await b.handle(call("read", { path: "C:\\a" }))).error, "Only in the cloud");
});

test("messages carry the plugin's name, are cut short, and come at a limited rate", async () => {
  const { b, shown, tick } = bridge(["notify"]);
  await b.handle(call("notify", { message: "x".repeat(500) }));
  assert.equal(shown[0].length, "Demo: ".length + 200);
  assert.match((await b.handle(call("notify", { message: 5 }))).error, /text/);
  for (let i = 0; i < 4; i++)
    assert.equal((await b.handle(call("notify", { message: "m" }))).error, undefined);
  assert.match((await b.handle(call("notify", { message: "m" }))).error, /Too many/);
  tick(6000);
  assert.equal((await b.handle(call("notify", { message: "m" }))).error, undefined);
});

test("the example plugin's manifest is valid and does what its page claims", () => {
  const dir = new URL("../../examples/plugins/text-tools/", import.meta.url);
  const r = parseManifest("text-tools", readFileSync(new URL("plugin.json", dir), "utf8"));
  assert.ok(r.ok, r.error);
  assert.deepEqual(r.manifest.permissions, ["selection", "read", "notify"]);
  assert.equal(r.manifest.actions[0].id, "count");
  assert.equal(r.manifest.previews[0].id, "csv");
  // The page answers the events and methods the manifest and the API define.
  const page = readFileSync(new URL(r.manifest.entry, dir), "utf8");
  assert.match(page, /m\.action === "count"/);
  assert.match(page, /m\.preview === "csv"/);
  assert.match(page, /event: "ready"/);
});
