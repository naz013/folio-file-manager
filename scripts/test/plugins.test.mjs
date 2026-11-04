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
  pageCsp,
  pageDoc,
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
  const grant = { version: "1.0.0", permissions: ["selection", "read", "notify"], digest: "aa" };
  assert.ok(isGranted(manifest, grant, "aa"));
  assert.ok(!isGranted(manifest, undefined, "aa"));
  assert.ok(!isGranted(manifest, { ...grant, version: "1.0.1" }, "aa"));
  assert.ok(!isGranted(manifest, { ...grant, permissions: ["selection", "read"] }, "aa"));
  // The same version with other files is not what was agreed to; unreadable files are never on.
  assert.ok(!isGranted(manifest, grant, "bb"));
  assert.ok(!isGranted(manifest, grant, null));
  assert.ok(!isGranted(manifest, { ...grant, digest: "" }, ""), "an approval from before hashes");
  // A plugin that asks for less than was granted stays on.
  const small = parse({ ...good, permissions: ["selection"] }).manifest;
  assert.ok(isGranted(small, grant, "aa"));
});

test("saved grants are cleaned of anything that isn't one", () => {
  assert.deepEqual(cleanGrants(null), {});
  assert.deepEqual(
    cleanGrants({
      a: { version: "1", permissions: ["read", "root"], digest: "d" },
      b: { version: 2, permissions: [] },
      c: "x",
    }),
    { a: { version: "1", permissions: ["read"], digest: "d" } },
  );
});

test("the list shows enabled, changed, broken and manifest-less plugins", () => {
  const grants = {
    demo: { version: "1.0.0", permissions: ["selection", "read", "notify"], digest: "d1" },
  };
  const text = JSON.stringify(good);
  const [on, changed, broken, none, swapped] = load(
    [
      { id: "demo", manifest: text, error: null, digest: "d1" },
      {
        id: "old",
        manifest: text.replace('"demo"', '"old"').replace("1.0.0", "2.0.0"),
        error: null,
        digest: "d1",
      },
      { id: "bad", manifest: "{}", error: null, digest: "d1" },
      { id: "empty", manifest: null, error: "No plugin.json", digest: null },
      { id: "swapped", manifest: text.replace('"demo"', '"swapped"'), error: null, digest: "d2" },
    ],
    {
      ...grants,
      old: { version: "1.0.0", permissions: [], digest: "d1" },
      swapped: { version: "1.0.0", permissions: ["selection", "read", "notify"], digest: "d1" },
    },
  );
  assert.ok(on.enabled && !on.changed);
  assert.ok(!changed.enabled && changed.changed);
  assert.ok(!swapped.enabled && swapped.changed, "same version, other files: off until approved");
  assert.ok(broken.error && !broken.enabled);
  assert.equal(none.error, "No plugin.json");
});

function plugins() {
  const grants = {
    demo: { version: "1.0.0", permissions: ["selection", "read", "notify"], digest: "d1" },
  };
  return load([{ id: "demo", manifest: JSON.stringify(good), error: null, digest: "d1" }], grants);
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
  const off = load([{ id: "demo", manifest: JSON.stringify(good), error: null, digest: "d1" }], {});
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
    {
      id: "constructor",
      manifest: JSON.stringify({ ...good, id: "constructor" }),
      error: null,
      digest: "d",
    },
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
  assert.equal(shown[0].length, "Demo (plugin): ".length + 200);
  assert.match((await b.handle(call("notify", { message: 5 }))).error, /text/);
  for (let i = 0; i < 4; i++)
    assert.equal((await b.handle(call("notify", { message: "m" }))).error, undefined);
  assert.match((await b.handle(call("notify", { message: "m" }))).error, /Too many/);
  tick(6000);
  assert.equal((await b.handle(call("notify", { message: "m" }))).error, undefined);
});

test("a name or text can't hide characters or pass for Folio or Windows", async () => {
  for (const name of [
    "Folio",
    "Windows Security",
    "W\u200Bindows",
    "ＦＯＬＩＯ updates",
    "fol-io",
  ]) {
    assert.ok(!parse({ ...good, name }).ok, name);
  }
  assert.ok(parse({ ...good, name: "Text tools" }).ok);
  assert.ok(!parse({ ...good, description: "a\u202Eb" }).ok, "bidi override");
  const { b, shown } = bridge(["notify"]);
  await b.handle(call("notify", { message: "line one\n\u202Eline\ttwo\u0007" }));
  assert.equal(shown[0], "Demo (plugin): line one line two");
  assert.match((await b.handle(call("notify", { message: "\n \u200B" }))).error, /text/);
});

test("files are readable for a minute after a run, not for ever", async () => {
  const { b, tick } = bridge(["read"]);
  const read = () => b.handle(call("read", { path: "C:\\a" }));
  b.allow(["C:\\a"]);
  tick(59_000);
  assert.equal((await read()).error, undefined);
  tick(2_000);
  assert.match((await read()).error, /not given/);
  b.allow(["C:\\a"]);
  assert.equal((await read()).error, undefined);
});

test("reads are limited in number at once and over time", async () => {
  let release;
  const gate = new Promise((r) => (release = r));
  const { b, tick } = bridge(["read"], {
    readText: () => gate.then(() => ({ text: "", truncated: false })),
  });
  const read = (id) => b.handle(call("read", { path: "C:\\a" }, id));
  b.allow(["C:\\a"]);
  const pending = [read(1), read(2)];
  assert.match((await read(3)).error, /Too many reads/);
  release();
  await Promise.all(pending);
  for (let i = 0; i < 18; i++) assert.equal((await read(10 + i)).error, undefined);
  assert.match((await read(99)).error, /Too many reads/);
  tick(11_000);
  assert.equal((await read(100)).error, undefined);
});

test("a page that floods the bridge is refused, then reported once", async () => {
  let reports = 0;
  const { b, tick } = bridge(["notify"], { abuse: () => reports++ });
  let refused = 0;
  for (let i = 0; i < 600; i++) {
    const r = await b.handle(i % 2 ? call("nope", {}, i) : "junk");
    if (r?.error?.startsWith("Too many requests")) refused++;
  }
  assert.ok(refused > 0 && reports === 1, `${refused} refused, ${reports} reports`);
  tick(2000);
  const next = await b.handle(call("nope", {}));
  assert.match(next.error, /Unknown method/, "a new second starts afresh");
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

test("the IPC probe plugin is a valid preview that asks for nothing", () => {
  const dir = new URL("../../examples/plugins/ipc-probe/", import.meta.url);
  const r = parseManifest("ipc-probe", readFileSync(new URL("plugin.json", dir), "utf8"));
  assert.ok(r.ok, r.error);
  assert.deepEqual(r.manifest.permissions, []);
  assert.deepEqual(r.manifest.previews[0].extensions, ["probe"]);
});

test("a page is given as a document with our base and policy ahead of its own markup", () => {
  const origin = "http://plugin.localhost";
  const html = "<!doctype html><html><head><title>x</title></head><body>hi</body></html>";
  const doc = pageDoc(html, `${origin}/demo/sub/`, origin);
  assert.ok(
    doc.startsWith('<!doctype html><html><head><base href="http://plugin.localhost/demo/sub/">'),
  );
  assert.ok(doc.indexOf("<base") < doc.indexOf("Content-Security-Policy"));
  assert.ok(doc.indexOf("Content-Security-Policy") < doc.indexOf(html));
  assert.ok(doc.endsWith(html));
  // A quote in the address can't close the attribute.
  assert.ok(!pageDoc("", 'http://x/"><script>1</script>', origin).includes("<script>"));
  const csp = pageCsp(origin);
  for (const part of [
    "default-src 'none'",
    "connect-src 'none'",
    "frame-src 'none'",
    "base-uri 'none'",
    "form-action 'none'",
    `script-src ${origin} 'unsafe-inline'`,
    `img-src ${origin} data:`,
  ]) {
    assert.ok(csp.includes(part), part);
  }
  assert.ok(!csp.includes("*"));
});

test("the app page never lets a frame go to a plugin address, and frames are not links", () => {
  // A page at plugin.localhost counts as Folio's own to Tauri and could run its commands, so the
  // plugin frame must be a document (srcdoc) and the app page must refuse a frame going there.
  const app = readFileSync(new URL("../../src/app.html", import.meta.url), "utf8");
  const policy = app.match(/http-equiv="Content-Security-Policy"\s+content="([^"]*)"/)?.[1];
  assert.equal(policy, "frame-src about:");
  const frame = readFileSync(
    new URL("../../src/lib/components/PluginFrame.svelte", import.meta.url),
    "utf8",
  );
  assert.match(frame, /srcdoc=\{doc\}/);
  assert.ok(!/<iframe[^>]*\ssrc=/.test(frame), "an iframe with a src link");
});
