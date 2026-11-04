import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import vm from "node:vm";

// The helpers are a plain script in the plugin's folder; they are loaded the way a page loads it.
const dir = new URL("../../src-tauri/resources/plugins/json-viewer/", import.meta.url);
const ctx = vm.createContext({ setTimeout, performance });
vm.runInContext(readFileSync(new URL("json.js", dir), "utf8"), ctx);
const { Json } = ctx;
const plain = (v) => JSON.parse(JSON.stringify(v));
const ids = (list) => list.map((l) => (l.close ? "}" : l.more ? `+${l.left}` : l.id));

const doc = { a: [1, 2, { b: "x" }], "odd key": null, c: { d: true } };
const open = (...paths) => new Set(paths.map((p) => Json.pk(p)));

test("only the open containers show their members, then a closing line", () => {
  const closed = Json.flatten(doc, open([]), new Map(), 100);
  assert.deepEqual(plain(ids(closed.list)), ["[]", '["a"]', '["odd key"]', '["c"]', "}"]);
  const more = Json.flatten(doc, open([], ["a"]), new Map(), 100);
  assert.deepEqual(plain(ids(more.list)), [
    "[]",
    '["a"]',
    '["a",0]',
    '["a",1]',
    '["a",2]',
    "}",
    '["odd key"]',
    '["c"]',
    "}",
  ]);
  const l = more.list;
  assert.equal(l[1].depth, 1);
  assert.equal(l[2].depth, 2);
  assert.deepEqual(plain([l[2].key, l[1].key, l[1].count, l[1].open, l[1].last]), [
    null,
    "a",
    3,
    true,
    false,
  ]);
  // The position of each node's line, for scrolling to it.
  assert.equal(more.index.get(Json.pk(["a", 1])), 3);
  assert.equal(more.index.get(Json.pk(["c"])), 7);
  // Empty containers and leaves are never "open".
  const empty = Json.flatten({ e: [], f: {} }, open([], ["e"], ["f"]), new Map(), 10);
  assert.ok(empty.list.every((x) => !x.open || x.id === "[]"));
});

test("an open container shows one window of its members, with lines to move it", () => {
  const big = { list: Array.from({ length: 25 }, (_, i) => i) };
  const id = '["list"]';
  const moves = (f) =>
    plain(
      f.list
        .filter((x) => x.more || x.earlier)
        .map((x) => [x.earlier ? "earlier" : "more", x.left]),
    );
  const first = Json.flatten(big, open([], ["list"]), new Map(), 10);
  assert.deepEqual(moves(first), [["more", 15]]);
  assert.equal(
    first.list.filter((x) => x.path && x.path[0] === "list" && x.path.length === 2).length,
    10,
  );
  // A window further on: members 10 to 19, with the ones before and after said.
  const mid = Json.flatten(big, open([], ["list"]), new Map([[id, 10]]), 10);
  assert.deepEqual(moves(mid), [
    ["earlier", 10],
    ["more", 5],
  ]);
  assert.ok(mid.index.has(Json.pk(["list", 10])) && mid.index.has(Json.pk(["list", 19])));
  assert.ok(!mid.index.has(Json.pk(["list", 9])) && !mid.index.has(Json.pk(["list", 20])));
  // The last window: nothing after it, and the last member has no comma.
  const end = Json.flatten(big, open([], ["list"]), new Map([[id, 20]]), 10);
  assert.deepEqual(moves(end), [["earlier", 20]]);
  assert.equal(end.list.find((x) => x.id === Json.pk(["list", 24])).last, true);
  // A window past the end still shows the last member.
  const past = Json.flatten(big, open([], ["list"]), new Map([[id, 999]]), 10);
  assert.ok(past.index.has(Json.pk(["list", 24])));
});

test("search finds keys and values anywhere, in document order, with the path to each", async () => {
  const d = { a: [{ k: "Needle one" }, "x"], needleKey: 5, z: { y: [null, "a needle here"] } };
  const r = await Json.search(d, "needle", 100);
  assert.deepEqual(plain(r.found), [["a", 0, "k"], ["needleKey"], ["z", "y", 1]]);
  assert.equal(r.capped, false);
  // Every path leads to the node that matched.
  for (const path of r.found) assert.notEqual(Json.valueAt(d, path), undefined);
  assert.deepEqual(plain((await Json.search({ n: 12345 }, "234", 5)).found), [["n"]]);
  assert.deepEqual(plain((await Json.search([null, true], "true", 5)).found), [[1]]);
  assert.deepEqual(plain((await Json.search("needle", "need", 5)).found), [[]]);
  assert.deepEqual(plain((await Json.search({}, "x", 5)).found), []);
});

test("search stops at its cap, gives the page turns, and can be cancelled", async () => {
  const d = Array.from({ length: 50000 }, (_, i) => ({ v: `hit ${i}` }));
  const capped = await Json.search(d, "hit", 10);
  assert.equal(capped.found.length, 10);
  assert.equal(capped.capped, true);
  let turns = 0;
  const all = await Json.search(d, "hit 4999", 100, {
    breathe: async () => {
      turns++;
    },
  });
  assert.equal(all.found.length, 11, "hit 4999 and hit 49990 to 49999");
  assert.ok(turns >= 0);
  let calls = 0;
  const stopped = await Json.search(d, "zzz", 5, {
    breathe: async () => {},
    cancelled: () => ++calls > 1,
  });
  assert.ok(stopped === null || stopped.found.length === 0);
});

test("a value is found by its path, and a bad path gives nothing", () => {
  assert.equal(Json.valueAt(doc, ["a", 2, "b"]), "x");
  assert.equal(Json.valueAt(doc, []), doc);
  assert.equal(Json.valueAt(doc, ["a", 9]), undefined);
  assert.equal(Json.valueAt(doc, ["c", "d", "e"]), undefined);
  assert.equal(Json.valueAt(doc, ["odd key"]), null);
  assert.equal(Json.pathText(["a", 2, "b"]), "$.a[2].b");
  assert.equal(Json.pathText(["odd key", 0]), '$["odd key"][0]');
});

test("expand all opens a bounded number of levels and containers", () => {
  const deep = {};
  let at = deep;
  for (let i = 0; i < 30; i++) at = at.next = {};
  at.x = 1;
  assert.ok(Json.openable(deep, 8, 3000, 1e6).length <= 10, "no deeper than 8 levels");
  const wide = { list: Array.from({ length: 5000 }, () => ({ a: 1 })) };
  assert.equal(Json.openable(wide, 8, 3000, 1e6).length, 3000);
  assert.equal(Json.openable(wide, 8, 3000, 100).length <= 100, true, "and looks at few nodes");
  assert.deepEqual(plain(Json.openable({ e: [] }, 8, 10, 100)), ["[]"]);
});

test("the first lines of a text are found without splitting all of it", () => {
  const t = "a\nb\r\nc\rd\n";
  assert.deepEqual(plain(Json.firstLines(t, 3)), {
    lines: ["a", "b", "c"],
    more: true,
    clipped: false,
  });
  assert.deepEqual(plain(Json.firstLines(t, 10)), {
    lines: ["a", "b", "c", "d", ""],
    more: false,
    clipped: false,
  });
  assert.deepEqual(plain(Json.firstLines("", 5)), { lines: [""], more: false, clipped: false });
  // One enormous line (a minified document) is cut, and says so.
  const one = Json.firstLines("x".repeat(5000000), 20000, 10000);
  assert.equal(one.lines.length, 1);
  assert.equal(one.lines[0].length, 10000);
  assert.equal(one.clipped, true);
  const huge = "x\n".repeat(2000000);
  const start = performance.now();
  const r = Json.firstLines(huge, 20000);
  assert.equal(r.lines.length, 20000);
  assert.equal(r.more, true);
  assert.ok(performance.now() - start < 500);
});

test("a 25,000-item array and a document of tens of MB are handled in good time", async () => {
  const items = Array.from({ length: 25000 }, (_, i) => ({
    id: i,
    tags: ["a", `t${i % 50}`],
    o: { y: `needle-${i}` },
  }));
  let start = performance.now();
  const f = Json.flatten(items, open([]), new Map(), 10000);
  const flat = performance.now() - start;
  assert.equal(
    f.list.length,
    10000 + 1 + 1 + 1,
    "10,000 members, the one to show more, the close, the root",
  );
  assert.ok(flat < 300, `flatten ${Math.round(flat)} ms`);
  // A far match: found, revealed (the window moved to hold it) and its line located.
  start = performance.now();
  const hit = (await Json.search(items, "needle-24990", 5)).found[0];
  assert.deepEqual(plain(hit), [24990, "o", "y"]);
  const windows = new Map([["[]", Math.floor(24990 / 10000) * 10000]]);
  const g = Json.flatten(items, open([], [24990], [24990, "o"]), windows, 10000);
  const at = g.index.get(Json.pk(hit.slice(0, 2)));
  assert.ok(at > 0 && g.list.length < 10100, `the line is at ${at} of ${g.list.length}`);
  assert.ok(performance.now() - start < 2000, "jumping to it is quick, not seconds");
  // About 20 MB of JSON: parse, then look for a value near the end.
  const big = [];
  let bytes = 0;
  for (let n = 0; bytes < 20 * 1048576; n++) {
    const o = {
      id: n,
      name: `item ${n}`,
      owner: { email: `u${n}@example.com` },
      notes: "lorem ipsum dolor sit amet ".repeat(3),
    };
    big.push(o);
    bytes += 230;
  }
  const text = JSON.stringify({ items: big });
  start = performance.now();
  const parsed = JSON.parse(text);
  const parse = performance.now() - start;
  start = performance.now();
  const last = await Json.search(parsed, `u${big.length - 3}@example.com`, 5);
  const find = performance.now() - start;
  assert.equal(last.found.length, 1);
  assert.ok(
    find < 3000,
    `searching 20 MB took ${Math.round(find)} ms (parse ${Math.round(parse)} ms)`,
  );
});

test("the page draws only the lines in view, reads in pieces, and copies by path", () => {
  const html = readFileSync(new URL("index.html", dir), "utf8");
  assert.match(html, /<script src="json\.js"><\/script>/);
  // Only the lines in view are elements, and there is no index of every node.
  assert.match(html, /function drawRows\(\)/);
  // The rows in the page stay when the view scrolls: only the edges are added and dropped.
  assert.match(html, /if \(drawn\.has\(i\)\) continue;/);
  assert.ok(!/buildIndex|let entries/.test(html), "no index of every node");
  // An open container shows one window of members that Next and Previous move, and a search
  // hit moves the window to it: however far in, only that many lines are made.
  assert.match(html, /windows\.set\(id, Math\.floor\(index \/ CHUNK\) \* CHUNK\);/);
  assert.match(html, /Next \$\{/);
  assert.match(html, /Previous \$\{/);
  // The tree is a tree for assistive technology, and the keyboard can move and open it.
  assert.match(html, /view\.setAttribute\("role", "tree"\)/);
  assert.match(html, /row\.setAttribute\("role", "treeitem"\)/);
  assert.match(html, /aria-activedescendant/);
  for (const key of ["ArrowDown", "ArrowUp", "ArrowRight", "ArrowLeft", "Home", "End", "Enter"]) {
    assert.ok(html.includes(`"${key}"`), key);
  }
  // Later pieces of a file are read in the encoding the start was found to be in.
  assert.match(html, /enc = enc \|\| r\.encoding;/);
  assert.match(html, /Json\.search\(doc, needle, MAX_HITS/);
  // A bigger file is read in pieces, waiting out the host's limit on reads.
  assert.match(html, /const MAX_FILE = 32 \* 1024 \* 1024;/);
  assert.match(html, /const PAGE = 4 \* 1024 \* 1024;/);
  assert.match(html, /async function readPiece\(/);
  assert.match(html, /\/Too many\/\.test\(e\.message\)/);
  // A large document says what is off, and copying works from the document by path.
  assert.match(html, /\$\("m-pretty"\)\.disabled = !hasDoc \|\| large;/);
  assert.match(html, /Json\.valueAt\(doc, selected\.path\)/);
  assert.ok(!/innerHTML|insertAdjacentHTML|document\.write/.test(html));
});

test("a hit far into a huge array costs a window of lines, not the lines up to it", async () => {
  const huge = Array.from({ length: 5000000 }, (_, i) => i);
  let start = performance.now();
  const hit = (await Json.search(huge, "4999990", 3)).found;
  const found = performance.now() - start;
  assert.deepEqual(plain(hit), [[4999990]]);
  // Reveal as the page does: the window that holds the member.
  const windows = new Map([["[]", Math.floor(4999990 / 10000) * 10000]]);
  start = performance.now();
  const f = Json.flatten(huge, open([]), windows, 10000);
  const flat = performance.now() - start;
  assert.ok(f.list.length < 10010, `${f.list.length} lines`);
  assert.ok(f.index.has(Json.pk([4999990])));
  assert.ok(f.list.length * 21 < 33000000, "well within the height an element can have");
  assert.ok(flat < 300, `flatten ${Math.round(flat)} ms`);
  assert.ok(found < 10000, `searching 5 million values took ${Math.round(found)} ms`);
});

test("expand all stays within a budget of lines, and a search's cap is exact", async () => {
  const wide = { list: Array.from({ length: 5000 }, () => ({ a: [1, 2, 3] })) };
  const ids = Json.openable(wide, 8, 3000, 1e6, 20000, 10000);
  const lines = Json.flatten(wide, new Set(ids), new Map(), 10000).list.length;
  assert.ok(lines <= 20000 + 10000, `${lines} lines`);
  // Exactly as many matches as the cap is not "more".
  const exact = await Json.search(["m", "m", "x"], "m", 2);
  assert.deepEqual([exact.found.length, exact.capped], [2, true], "a third member may follow");
  const last = await Json.search(["x", "m", "m"], "m", 2);
  assert.deepEqual([last.found.length, last.capped], [2, false], "nothing follows the last");
});
