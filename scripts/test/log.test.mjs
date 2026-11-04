import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import vm from "node:vm";

// The helpers are a plain script in the plugin's folder; they are loaded the way a page loads it.
const file = new URL("../../src-tauri/resources/plugins/log-viewer/log.js", import.meta.url);
const ctx = vm.createContext({ TextEncoder });
vm.runInContext(readFileSync(file, "utf8"), ctx);
const { Log } = ctx;
const plain = (v) => JSON.parse(JSON.stringify(v));

test("a line's level is the first level word near its start", () => {
  const cases = [
    ["2026-11-04 10:00:00 [ERROR] boom", "error"],
    ["2026-11-04T10:00:00Z WARN disk almost full", "warn"],
    ["Nov  4 10:00:00 host app[12]: info: started", "info"],
    ["10:00:00.123 | DEBUG | worker | tick", "debug"],
    ["[trace] entering main", "trace"],
    ['level=warning msg="slow"', "warn"],
    ["2026/11/04 10:00:00 [crit] x", ""],
    ["FATAL: out of memory", "error"],
    ["[err] timeout", "error"],
    ["VERBOSE: details", "debug"],
    // The first word wins, so a message that mentions another level doesn't change it.
    ["INFO the error page was shown", "info"],
    // A word inside another word, or far from the start, is not a level.
    ["interrogating the information", ""],
    ["x".repeat(120) + " ERROR late", ""],
    ["no level at all", ""],
    ["", ""],
  ];
  for (const [line, want] of cases) assert.equal(Log.levelOf(line), want, line);
});

test("timestamps at the start of a line are found in the usual shapes", () => {
  const len = (s) => Log.stampLength(s);
  assert.equal(len("2026-11-04 10:00:00 rest"), 19);
  assert.equal(len("2026-11-04T10:00:00.123Z rest"), 24);
  assert.equal(len("[2026-11-04 10:00:00,123] rest"), 25);
  assert.equal(len("2026-11-04T10:00:00+02:00 rest"), 25);
  assert.equal(len("2026/11/04 10:00:00 rest"), 19);
  assert.equal(len("10:00:00.5 rest"), 10);
  assert.equal(len("Nov  4 10:00:00 host"), 15);
  assert.equal(len("no stamp 10:00:00"), 0);
  assert.equal(len(""), 0);
});

test("stack frames and details take the level of the line above them", () => {
  const lines = [
    "2026-11-04 10:00:00 INFO start",
    "2026-11-04 10:00:01 ERROR failed",
    "\tat com.x.Main.run(Main.java:10)",
    "Caused by: java.io.IOException",
    "... 12 more",
    "plain line after",
    "2026-11-04 10:00:02 WARN slow",
    "    detail of the warning",
  ];
  assert.deepEqual(plain(Log.levels(lines)), [
    "info",
    "error",
    "error",
    "error",
    "error",
    "",
    "warn",
    "warn",
  ]);
  // A page that starts in the middle of a trace carries its level in.
  assert.deepEqual(plain(Log.levels(["    at x", "plain"], "error")), ["error", ""]);
  assert.ok(
    Log.continues("\tat x") &&
      Log.continues("Caused by: y") &&
      !Log.continues("at x") &&
      !Log.continues(""),
  );
});

test("a piece of a file is cut down to whole lines, with their place in the file", () => {
  const whole = (t, s, a, b) => plain(Log.wholeLines(t, s, a, b));
  // The start of the file, not its end: the last, cut line goes.
  assert.deepEqual(whole("one\ntwo\nthr", 0, true, false), {
    text: "one\ntwo\n",
    start: 0,
    end: 8,
  });
  // The middle: the first partial line and the last cut line both go.
  assert.deepEqual(whole("ne\ntwo\nthree\nfo", 100, false, false), {
    text: "two\nthree\n",
    start: 103,
    end: 113,
  });
  // The end of the file: its last line is kept even without a line break.
  assert.deepEqual(whole("ne\ntwo\nlast", 100, false, true), {
    text: "two\nlast",
    start: 103,
    end: 111,
  });
  // Whole lines already: nothing is lost.
  assert.deepEqual(whole("a\nb\n", 10, true, true), { text: "a\nb\n", start: 10, end: 14 });
  // A piece with no line break is kept, so one huge line can't stall the reading.
  assert.deepEqual(whole("no breaks here", 0, false, false), {
    text: "no breaks here",
    start: 0,
    end: 14,
  });
  // Positions count bytes: a character of 2, 3 and 4 bytes before the cut.
  const t = "é€\u{1f600}x\nnext\ncu";
  const r = whole(t, 0, true, false);
  assert.equal(r.text, "é€\u{1f600}x\nnext\n");
  assert.equal(r.end, Buffer.byteLength(r.text));
  // CR LF breaks count their two bytes.
  assert.deepEqual(whole("a\r\nb\r\nc", 0, true, false), { text: "a\r\nb\r\n", start: 0, end: 6 });
});

test("pieces read one after another rejoin into the lines of the file", () => {
  // Read a file in pieces the way the viewer does, from the end towards the start, and from the
  // start towards the end: the lines come out the same as the file's.
  const lines = Array.from({ length: 400 }, (_, i) => `line ${i} é€ ${"x".repeat(i % 37)}`);
  const text = lines.join("\n") + "\n";
  const bytes = Buffer.from(text, "utf8");
  const read = (off, len) => {
    // What the core gives: bytes from `off`, moved on to a character start, cut before a character.
    let s = off;
    while (s < bytes.length && (bytes[s] & 0xc0) === 0x80) s++;
    let e = Math.min(bytes.length, off + len);
    if (e < bytes.length) while (e > s && (bytes[e] & 0xc0) === 0x80) e--;
    return { text: bytes.subarray(s, e).toString("utf8"), start: s, end: e, size: bytes.length };
  };
  // Backwards from the tail.
  const piece = 700;
  let got = [];
  let end = bytes.length;
  let atStartOfFile = false;
  while (!atStartOfFile) {
    const off = Math.max(0, end - piece);
    const r = read(off, end - off);
    const w = Log.wholeLines(r.text, r.start, off === 0, true);
    got = [...Log.linesOf(w.text), ...got];
    end = w.start;
    atStartOfFile = off === 0;
    assert.ok(end >= 0);
  }
  assert.deepEqual(got, lines);
  // Forwards from the start.
  got = [];
  let at = 0;
  for (;;) {
    const r = read(at, piece);
    const w = Log.wholeLines(r.text, r.start, true, r.end >= r.size);
    got.push(...Log.linesOf(w.text));
    if (r.end >= r.size) break;
    assert.ok(w.end > at, "makes progress");
    at = w.end;
  }
  assert.deepEqual(got, lines);
});

test("lines of a piece lose their breaks and the empty last line", () => {
  assert.deepEqual(plain(Log.linesOf("a\r\nb\nc\rd\n")), ["a", "b", "c", "d"]);
  assert.deepEqual(plain(Log.linesOf("a\n\nb")), ["a", "", "b"]);
  assert.deepEqual(plain(Log.linesOf("")), []);
});

test("the log viewer plugin is a valid preview that asks only to read", async () => {
  const { parseManifest } = await import("../../src/lib/plugins.ts");
  const dir = new URL("../../src-tauri/resources/plugins/log-viewer/", import.meta.url);
  const r = parseManifest("log-viewer", readFileSync(new URL("plugin.json", dir), "utf8"));
  assert.ok(r.ok, r.error);
  assert.deepEqual(r.manifest.permissions, ["read"]);
  assert.deepEqual(r.manifest.previews[0].extensions, ["log", "out", "err"]);
  const page = readFileSync(new URL("index.html", dir), "utf8");
  assert.match(page, /<script src="log\.js"><\/script>/);
  assert.match(page, /m\.preview === "log"/);
});

test("a byte position is found in the text as a character index, or not at all inside a character", () => {
  const t = "a\u00e9\u20ac\u{1f600}z";
  // Bytes: a=1, e-acute=2, euro=3, the emoji=4, z=1.
  const at = (b) => Log.splitAt(t, b);
  assert.deepEqual([0, 1, 3, 6, 10, 11].map(at), [0, 1, 2, 3, 5, 6]);
  // Inside a character, or past the end.
  assert.deepEqual([2, 4, 5, 7, 8, 9, 12].map(at), [-1, -1, -1, -1, -1, -1, -1]);
  assert.equal(Log.splitAt("", 0), 0);
});
