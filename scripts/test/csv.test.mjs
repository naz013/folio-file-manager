import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import vm from "node:vm";

// The reader is a plain script in the plugin's folder; it is loaded the way a page loads it.
const file = new URL("../../src-tauri/resources/plugins/csv-viewer/csv.js", import.meta.url);
const ctx = vm.createContext({});
vm.runInContext(readFileSync(file, "utf8"), ctx);
const { CSV } = ctx;
const plain = (v) => JSON.parse(JSON.stringify(v));
const parse = (t, d = ",", max) => plain(CSV.parse(t, d, max));

test("plain rows, line endings and a missing final newline", () => {
  assert.deepEqual(parse("a,b,c\n1,2,3\n"), [
    ["a", "b", "c"],
    ["1", "2", "3"],
  ]);
  assert.deepEqual(parse("a,b\r\n1,2\r\n3,4"), [
    ["a", "b"],
    ["1", "2"],
    ["3", "4"],
  ]);
  assert.deepEqual(parse("a,b\r1,2"), [
    ["a", "b"],
    ["1", "2"],
  ]);
  assert.deepEqual(parse(""), []);
  assert.deepEqual(parse("\n\n"), []);
});

test("empty fields, trailing delimiters and ragged rows are kept as they are", () => {
  assert.deepEqual(parse("a,,c\n,,\n1"), [["a", "", "c"], ["", "", ""], ["1"]]);
  assert.deepEqual(parse("a,b,"), [["a", "b", ""]]);
  assert.deepEqual(parse("a,b,\n"), [["a", "b", ""]]);
  // Blank lines between rows are skipped, and don't end the file.
  assert.deepEqual(parse("a\n\nb\n"), [["a"], ["b"]]);
});

test("quoted fields hold delimiters, quotes and line breaks", () => {
  assert.deepEqual(parse('"a,b",c\n"say ""hi""",d'), [
    ["a,b", "c"],
    ['say "hi"', "d"],
  ]);
  assert.deepEqual(parse('"line one\nline two",x\r\n"cr\r\nlf",y'), [
    ["line one\nline two", "x"],
    ["cr\r\nlf", "y"],
  ]);
  assert.deepEqual(parse('"",x\n" padded ",y'), [
    ["", "x"],
    [" padded ", "y"],
  ]);
  // A quote in the middle of an unquoted field is text; stray text after a closing quote stays.
  assert.deepEqual(parse('5" pipe,x\n"a"b,c'), [
    ['5" pipe', "x"],
    ["ab", "c"],
  ]);
  // A quote that never closes takes the rest.
  assert.deepEqual(parse('a,"open\nstill open,x'), [["a", "open\nstill open,x"]]);
});

test("a byte-order mark is dropped and other delimiters work", () => {
  assert.deepEqual(parse("﻿a,b\n1,2"), [
    ["a", "b"],
    ["1", "2"],
  ]);
  assert.deepEqual(parse("a;b\n1;2", ";"), [
    ["a", "b"],
    ["1", "2"],
  ]);
  assert.deepEqual(parse("a\tb c\n1\t2", "\t"), [
    ["a", "b c"],
    ["1", "2"],
  ]);
  assert.deepEqual(parse("a|b\n1|2", "|"), [
    ["a", "b"],
    ["1", "2"],
  ]);
  // A tab delimiter isn't confused by commas in the text.
  assert.deepEqual(parse("a,b\tc", "\t"), [["a,b", "c"]]);
});

test("maxRows stops early", () => {
  assert.equal(parse("1\n2\n3\n4\n", ",", 2).length, 2);
});

test("the delimiter is found from the start of the text", () => {
  const d = (t) => CSV.detect(t);
  assert.equal(d("a,b,c\n1,2,3\n4,5,6"), ",");
  assert.equal(d("a;b;c\n1;2;3\n4;5;6"), ";");
  assert.equal(d("a\tb\tc\n1\t2\t3"), "\t");
  assert.equal(d("a|b|c\n1|2|3"), "|");
  // Commas in the prose of a semicolon file, or in quotes, don't win.
  assert.equal(d('name;note\nann;"likes a, b, c"\nbob;"x, y, z"'), ";");
  // Commas in the text of a tab file.
  assert.equal(d("name\tnote\nann\tone, two, three\nbob\tfour, five"), "\t");
  // One column, or nothing: comma.
  assert.equal(d("just\nwords\nhere"), ",");
  assert.equal(d(""), ",");
});

test("letters name columns like a spreadsheet and numbers are told apart", () => {
  assert.deepEqual(
    [0, 1, 25, 26, 27, 51, 52, 701, 702].map((i) => CSV.letters(i)),
    ["A", "B", "Z", "AA", "AB", "AZ", "BA", "ZZ", "AAA"],
  );
  for (const ok of ["1", "-2.5", "+3", ".5", "1e9", " 42 ", "1.", "-0.5E-3"])
    assert.ok(CSV.isNumber(ok), ok);
  for (const bad of [
    "",
    "abc",
    "1,000",
    "12abc",
    "1.2.3",
    "--1",
    "$5",
    "0x10",
    "NaN",
    "Infinity",
  ]) {
    assert.ok(!CSV.isNumber(bad), bad);
  }
});

test("large and hostile input is read quickly and without throwing", () => {
  const started = Date.now();
  const big = Array.from(
    { length: 200000 },
    (_, i) => `${i},name ${i},"q ""${i}""",${i * 1.5}`,
  ).join("\n");
  const rows = CSV.parse(big, ",");
  assert.equal(rows.length, 200000);
  assert.equal(rows[199999][1], "name 199999");
  const inputs = [
    '"'.repeat(100000),
    ",".repeat(100000),
    "\n".repeat(100000),
    '"a\n'.repeat(20000),
    "\r".repeat(50000),
    "a,".repeat(200000),
    "\u0000\ud800,￾\n".repeat(1000),
  ];
  for (const t of inputs) {
    assert.ok(Array.isArray(CSV.parse(t, ",")));
    assert.equal(typeof CSV.detect(t), "string");
  }
  assert.ok(Date.now() - started < 6000, `took ${Date.now() - started} ms`);
});

test("the CSV viewer plugin is a valid preview that asks only to read", async () => {
  const { parseManifest } = await import("../../src/lib/plugins.ts");
  const dir = new URL("../../src-tauri/resources/plugins/csv-viewer/", import.meta.url);
  const r = parseManifest("csv-viewer", readFileSync(new URL("plugin.json", dir), "utf8"));
  assert.ok(r.ok, r.error);
  assert.deepEqual(r.manifest.permissions, ["read"]);
  assert.deepEqual(r.manifest.previews[0].extensions, ["csv", "tsv"]);
  const page = readFileSync(new URL("index.html", dir), "utf8");
  assert.match(page, /<script src="csv\.js"><\/script>/);
  assert.match(page, /m\.preview === "csv"/);
});

// ---- Reading in pieces, and ordering and filtering in slices ----

/** Reads `text` in pieces of `size` characters the way the page does: a row a piece ends in waits. */
function inPieces(text, size, d = ",") {
  const rows = [];
  let rest = "";
  let first = true;
  for (let at = 0; at < text.length; at += size) {
    const last = at + size >= text.length;
    const chunk = CSV.parseChunk(rest + text.slice(at, at + size), d, { final: last, first });
    rows.push(...plain(chunk.rows));
    rest = (rest + text.slice(at, at + size)).slice(chunk.consumed);
    first = false;
  }
  return rows;
}

test("a text read in pieces gives the rows it gives whole, wherever the pieces end", () => {
  const text =
    '\ufeffid,name,note\r\n1,"Ann ""A"" Lee","two\nlines"\n2,Bo,\n\n3,"unclosed,\nand more\r4,Cy,"x"y\n5,,\n6';
  const whole = parse(text);
  for (let size = 1; size <= text.length + 1; size++) {
    assert.deepEqual(inPieces(text, size), whole, `pieces of ${size}`);
  }
  // A piece that ends inside a quoted field keeps the whole row for the next piece.
  const cut = CSV.parseChunk('a,b\n1,"two\nli', ",", { final: false });
  assert.deepEqual(plain(cut.rows), [["a", "b"]]);
  assert.equal(cut.consumed, 4);
  // A piece that ends on a row's end gives every row and consumes it all.
  const whole2 = CSV.parseChunk("a,b\n1,2\n", ",", { final: false });
  assert.equal(whole2.rows.length, 2);
  assert.equal(whole2.consumed, 8);
});

test("pieces of random text match the whole, for any delimiter", () => {
  let seed = 7;
  const rnd = () => (seed = (seed * 1103515245 + 12345) & 0x7fffffff) / 0x7fffffff;
  const alphabet = ["a", "b", ",", ";", '"', "\n", "\r", " ", "1"];
  for (let t = 0; t < 200; t++) {
    let text = "";
    for (let i = 0, n = Math.floor(rnd() * 60); i < n; i++)
      text += alphabet[Math.floor(rnd() * alphabet.length)];
    for (const d of [",", ";"]) {
      const whole = parse(text, d);
      for (const size of [1, 2, 3, 5, 8, 13]) {
        assert.deepEqual(
          inPieces(text, size, d),
          whole,
          `${JSON.stringify(text)} / ${d} / ${size}`,
        );
      }
    }
  }
});

test("sortOrder gives the same order as sorting, keeps ties in file order, and gives turns", async () => {
  const keys = Array.from({ length: 50000 }, (_, i) => (i * 7919) % 97);
  const cmp = (a, b) => keys[a] - keys[b];
  let turns = 0;
  const got = await CSV.sortOrder(keys.length, cmp, {
    run: 1000,
    breathe: async () => {
      turns++;
    },
  });
  const want = Array.from(keys.keys()).sort(cmp);
  assert.deepEqual(plain(got), want);
  assert.ok(turns > 0, "the page was given turns");
  assert.deepEqual(plain(await CSV.sortOrder(0, cmp)), []);
  assert.deepEqual(plain(await CSV.sortOrder(1, cmp)), [0]);
});

test("a sort or filter that is replaced stops early", async () => {
  const cmp = (a, b) => a - b;
  let calls = 0;
  const stopped = await CSV.sortOrder(100000, cmp, {
    run: 500,
    breathe: async () => {},
    cancelled: () => ++calls > 3,
  });
  assert.equal(stopped, null);
  let seen = 0;
  const filtered = await CSV.filterOrder(
    Array.from({ length: 200000 }, (_, i) => i),
    () => true,
    { breathe: async () => {}, cancelled: () => ++seen > 2 },
  );
  // Slices are time-based, so a fast run may finish before a turn: both ends are right.
  assert.ok(filtered === null || filtered.length === 200000);
});

test("filterOrder keeps the order, reports what it has found, and covers every row", async () => {
  const order = Array.from({ length: 30000 }, (_, i) => 29999 - i);
  const progress = [];
  const found = await CSV.filterOrder(order, (i) => i % 1000 === 0, {
    breathe: async () => {},
    progress: (list, done, total) => progress.push([list.length, done, total]),
  });
  assert.deepEqual(
    plain(found),
    order.filter((i) => i % 1000 === 0),
  );
  for (const [, done, total] of progress) assert.ok(done <= total && total === 30000);
});

test("a 50 MB file is read in pieces, each quick, and ordered without one long block", async () => {
  const line = (i) =>
    `${i},"Name ${(i * 7919) % 100003}",City ${(i * 31) % 977},${((i * 104729) % 100000) / 100},free text number ${i} to fill the row\n`;
  const parts = ["id,name,city,amount,note\n"];
  let bytes = parts[0].length;
  for (let i = 0; bytes < 50 * 1048576; i++) {
    const l = line(i);
    parts.push(l);
    bytes += l.length;
  }
  const text = parts.join("");
  const PIECE = 2 * 1048576;
  const rows = [];
  let rest = "";
  let slowest = 0;
  for (let at = 0; at < text.length; at += PIECE) {
    const last = at + PIECE >= text.length;
    const t = performance.now();
    const piece = rest + text.slice(at, at + PIECE);
    const chunk = CSV.parseChunk(piece, ",", { final: last, first: at === 0 });
    for (const r of chunk.rows) rows.push(r);
    rest = piece.slice(chunk.consumed);
    slowest = Math.max(slowest, performance.now() - t);
  }
  assert.ok(rows.length > 400000, `${rows.length} rows`);
  assert.equal(rows[1][0], "0");
  assert.equal(rows.at(-1)[0], String(rows.length - 2));
  // One piece of 2 MB is a short block for the page (generous: CI machines vary).
  assert.ok(slowest < 400, `slowest piece ${Math.round(slowest)} ms`);
  // A numeric sort of all of them, in slices.
  const amounts = new Float64Array(rows.length - 1);
  for (let i = 1; i < rows.length; i++) amounts[i - 1] = Number(rows[i][3]);
  let longest = 0;
  let last = performance.now();
  const sorted = await CSV.sortOrder(amounts.length, (a, b) => amounts[a] - amounts[b], {
    breathe: async () => {
      longest = Math.max(longest, performance.now() - last);
      await new Promise((r) => setTimeout(r, 0));
      last = performance.now();
    },
  });
  assert.equal(sorted.length, amounts.length);
  assert.ok(
    amounts[sorted[0]] <= amounts[sorted[1]] && amounts[sorted.at(-2)] <= amounts[sorted.at(-1)],
  );
  assert.ok(longest < 250, `longest slice ${Math.round(longest)} ms`);
});

test("the page reads in pieces, says what it covers, and bounds what it holds", () => {
  const html = readFileSync(
    new URL("../../src-tauri/resources/plugins/csv-viewer/index.html", import.meta.url),
    "utf8",
  );
  assert.match(html, /const PAGE = 4 \* 1024 \* 1024;/);
  assert.match(html, /const AUTO_LOAD = 16 \* 1024 \* 1024;/);
  assert.match(html, /const MAX_LOAD = 64 \* 1024 \* 1024;/);
  assert.match(html, /const MAX_ROWS = 750000;/);
  // Progress, a way to stop, a way to read on, and the coverage of a sort and a filter.
  assert.match(html, /Reading… \$\{fmtSize\(at\)\} of/);
  assert.match(html, /"Stop"/);
  assert.match(html, /Read more \(up to/);
  assert.match(
    html,
    /Sorting and filtering cover the \$\{fmtNum\(data\.length\)\} rows read so far/,
  );
  assert.match(html, /Sorting and filtering cover these rows only/);
  // The slow parts run in slices, and the order is kept while only the filter changes.
  assert.match(html, /CSV\.sortOrder\(/);
  assert.match(html, /CSV\.filterOrder\(/);
  assert.match(html, /ordered !== key/);
  // Nothing from the file is ever markup.
  assert.ok(!/innerHTML|insertAdjacentHTML|document\.write/.test(html));
});

test("maxRows stops a piece where it says, and the rest is the text after the last row read", () => {
  const text = "a,b\n1,2\n3,4\n5,6\n";
  const chunk = CSV.parseChunk(text, ",", { final: false, maxRows: 2 });
  assert.deepEqual(plain(chunk.rows), [
    ["a", "b"],
    ["1", "2"],
  ]);
  assert.equal(text.slice(chunk.consumed), "3,4\n5,6\n");
  // Blank lines after the last row are not rows: nothing is left to read.
  const tail = CSV.parseChunk("a,b\n\n\n", ",", { final: true, maxRows: 5 });
  assert.equal(tail.rows.length, 1);
  assert.equal(tail.consumed, 6);
});

test("the page waits out the host's read limit, holds one row at most 8 MB, and names a stop", () => {
  const html = readFileSync(
    new URL("../../src-tauri/resources/plugins/csv-viewer/index.html", import.meta.url),
    "utf8",
  );
  assert.match(html, /const PAGE = 4 \* 1024 \* 1024;/);
  assert.match(html, /const MAX_ROW_TEXT = 8 \* 1024 \* 1024;/);
  // "Too many reads" is waited out, not shown as the end of the file.
  assert.match(html, /\/Too many\/\.test\(e\.message\)/);
  assert.match(html, /async function readPiece\(/);
  // Any other failure is said in the notice, with the rows that were read.
  assert.match(html, /reading stopped: \$\{loadError\}/);
  // Rows of the last piece are covered by an active sort or filter.
  assert.match(html, /function flushGrow\(\)/);
  assert.match(html, /if \(dirty && !loading && !busy\) refresh\(false\);/);
  // The notice is a status, and the bar has a name.
  assert.match(html, /<div id="top" role="status"><\/div>/);
  assert.match(html, /ariaLabel: "Reading the file"/);
});
