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
