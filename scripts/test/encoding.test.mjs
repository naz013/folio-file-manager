import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import vm from "node:vm";
import { createBridge, ENCODINGS, parseManifest } from "../../src/lib/plugins.ts";

const dir = new URL("../../src-tauri/resources/plugins/", import.meta.url);

/** A fake page: just enough of the DOM for the menu to be built and read back. */
function page() {
  const make = () => ({
    children: [],
    attrs: {},
    append(...c) {
      this.children.push(...c);
      this.options = this.children;
    },
    setAttribute(k, v) {
      this.attrs[k] = v;
    },
    value: "",
  });
  const ctx = vm.createContext({ document: { createElement: make } });
  ctx.self = ctx;
  vm.runInContext(readFileSync(new URL("text-viewer/encoding.js", dir), "utf8"), ctx);
  return { Enc: ctx.Enc, select: make() };
}

test("the menu lists Auto first and then each encoding the bridge accepts", () => {
  const { Enc, select } = page();
  Enc.fill(select);
  assert.deepEqual(
    select.children.map((o) => o.value),
    ["", ...ENCODINGS],
  );
  assert.equal(select.children[0].textContent, "Auto");
  assert.equal(select.attrs["aria-label"], "Encoding");
});

test("it says which encoding was used, and when the file had a mark", () => {
  const { Enc, select } = page();
  Enc.fill(select);
  assert.equal(Enc.name({ encoding: "utf-16le", bom: true }), "UTF-16 LE (BOM)");
  assert.equal(Enc.name({ encoding: "windows-1252", bom: false }), "Windows-1252");
  assert.equal(Enc.name({ encoding: "utf-8", bom: false }), "UTF-8");
  Enc.show(select, { encoding: "utf-16be", bom: true }, "");
  assert.equal(select.children[0].textContent, "Auto: UTF-16 BE (BOM)");
  assert.equal(select.value, "");
  // A choice shows as the selection, and Auto is plain again.
  Enc.show(select, { encoding: "windows-1252", bom: false }, "windows-1252");
  assert.equal(select.children[0].textContent, "Auto");
  assert.equal(select.value, "windows-1252");
  assert.match(select.title, /Windows-1252/);
});

test("the four viewers carry the menu and pass the choice on every read", () => {
  for (const id of ["text", "json", "markdown", "csv"]) {
    const folder = `${id}-viewer/`;
    const html = readFileSync(new URL(`${folder}index.html`, dir), "utf8");
    assert.match(html, /<script src="encoding\.js"><\/script>/, id);
    assert.match(html, /<select id="enc"><\/select>/, id);
    const reads = html.match(/call\("readRange", \{[^}]*\}/g) ?? [];
    assert.ok(reads.length > 0, id);
    for (const r of reads)
      assert.match(r, /encoding: (chosen|enc|loadEnc) \|\| undefined/, `${id}: ${r}`);
    // One shared helper, the same in each folder.
    assert.equal(
      readFileSync(new URL(`${folder}encoding.js`, dir), "utf8"),
      readFileSync(new URL("text-viewer/encoding.js", dir), "utf8"),
      id,
    );
  }
});

function bridge() {
  const seen = [];
  const manifest = parseManifest(
    "demo",
    JSON.stringify({
      id: "demo",
      name: "Demo",
      version: "1.0.0",
      description: "d",
      permissions: ["read"],
      actions: [],
    }),
  ).manifest;
  const answer = { text: "x", start: 0, end: 1, size: 1, encoding: "utf-8", bom: false };
  const b = createBridge(manifest, {
    notify() {},
    readText: async (p, enc) => {
      seen.push(["read", enc]);
      return { ...answer, truncated: false };
    },
    readRange: async (p, o, l, enc) => {
      seen.push(["range", enc]);
      return answer;
    },
    readBytes: async () => ({ data: new Uint8Array(), start: 0, end: 0, size: 0 }),
  });
  b.allow(["C:\\a"]);
  return { b, seen };
}
const call = (method, params) => ({ folio: 1, id: 1, method, params });

test("read and readRange pass a known encoding to the host and refuse any other", async () => {
  const { b, seen } = bridge();
  for (const enc of ENCODINGS) {
    await b.handle(call("readRange", { path: "C:\\a", offset: 0, length: 10, encoding: enc }));
    await b.handle(call("read", { path: "C:\\a", encoding: enc }));
  }
  assert.deepEqual(
    seen.map((s) => s[1]),
    ENCODINGS.flatMap((e) => [e, e]),
  );
  // Without one, or with null, the host is asked to find it.
  seen.length = 0;
  await b.handle(call("readRange", { path: "C:\\a", offset: 0, length: 10 }));
  await b.handle(call("readRange", { path: "C:\\a", offset: 0, length: 10, encoding: null }));
  assert.deepEqual(
    seen.map((s) => s[1]),
    [undefined, undefined],
  );
  for (const bad of ["latin1", "", 5, {}, "UTF-8"]) {
    const r = await b.handle(
      call("readRange", { path: "C:\\a", offset: 0, length: 10, encoding: bad }),
    );
    assert.match(r.error, /"encoding" must be one of/, String(bad));
  }
  const bytes = await b.handle(
    call("readBytes", { path: "C:\\a", offset: 0, length: 10, encoding: "utf-8" }),
  );
  assert.match(bytes.error, /"encoding" must be one of/, "bytes have no encoding");
});

test("each viewer drops a read that returns after a newer open began", () => {
  for (const id of ["text", "json", "markdown", "csv"]) {
    const html = readFileSync(new URL(`${id}-viewer/index.html`, dir), "utf8");
    assert.match(html, /const mine = \+\+generation;/, id);
    assert.match(html, /if \(mine !== generation\) return;/, id);
    assert.match(html, /Enc\.reset\(encSelect\)/, id);
  }
  // The CSV pages of one open share one encoding, whatever is chosen meanwhile.
  const csv = readFileSync(new URL("csv-viewer/index.html", dir), "utf8");
  assert.match(csv, /loadEnc = chosen;/);
  assert.match(csv, /encoding: loadEnc \|\| undefined/);
});
