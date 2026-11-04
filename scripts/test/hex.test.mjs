import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import vm from "node:vm";

// The helpers are a plain script in the plugin's folder; it is loaded the way a page loads it.
const file = new URL("../../src-tauri/resources/plugins/hex-viewer/hex.js", import.meta.url);
const ctx = vm.createContext({ TextEncoder, DataView, Uint8Array, Math });
vm.runInContext(readFileSync(file, "utf8"), ctx);
const { Hex } = ctx;
const plain = (v) => JSON.parse(JSON.stringify(v));
const bytes = (...b) => new Uint8Array(b);

test("an offset is read as decimal, 0x hex or hex with an h", () => {
  assert.equal(Hex.parseOffset("4096"), 4096);
  assert.equal(Hex.parseOffset("0x1000"), 4096);
  assert.equal(Hex.parseOffset("0X1f"), 31);
  assert.equal(Hex.parseOffset("1000h"), 4096);
  assert.equal(Hex.parseOffset("  42  "), 42);
  assert.equal(Hex.parseOffset("0"), 0);
  for (const bad of [
    "",
    "abc",
    "0x",
    "-1",
    "1.5",
    "12g",
    "0xZZ",
    "h",
    "1e3",
    "99999999999999999999",
  ]) {
    assert.equal(Hex.parseOffset(bad), null, bad);
  }
});

test("bytes to look for are read from hex or text, within limits", () => {
  const hex = (s) => plain(Hex.parsePattern(s, "hex"));
  assert.deepEqual(hex("de ad be ef").bytes, { 0: 222, 1: 173, 2: 190, 3: 239 });
  assert.deepEqual(Array.from(Hex.parsePattern("DEADBEEF", "hex").bytes), [222, 173, 190, 239]);
  assert.deepEqual(Array.from(Hex.parsePattern("0xde, 0xAD", "hex").bytes), [222, 173]);
  assert.deepEqual(
    Array.from(Hex.parsePattern("7f 45 4c 46", "hex").bytes),
    [0x7f, 0x45, 0x4c, 0x46],
  );
  for (const bad of ["", "   ", "xyz", "abc", "12 3", "de ad zz"]) assert.ok(hex(bad).error, bad);
  assert.ok(hex("00 ".repeat(65)).error, "too long");
  assert.equal(Hex.parsePattern("00 ".repeat(64), "hex").bytes.length, 64);
  // Text is UTF-8.
  assert.deepEqual(Array.from(Hex.parsePattern("Aé", "text").bytes), [0x41, 0xc3, 0xa9]);
  assert.ok(Hex.parsePattern("", "text").error);
  assert.ok(Hex.parsePattern("x".repeat(65), "text").error);
});

test("a pattern is found forwards and backwards in a piece, and not where it only starts", () => {
  const hay = bytes(1, 2, 3, 1, 2, 3, 4, 1, 2, 3);
  const p = bytes(1, 2, 3);
  assert.equal(Hex.indexOf(hay, p), 0);
  assert.equal(Hex.indexOf(hay, p, 1), 3);
  assert.equal(Hex.indexOf(hay, p, 4), 7);
  assert.equal(Hex.indexOf(hay, p, 8), -1, "a match that would run past the end");
  assert.equal(Hex.indexOf(hay, bytes(9)), -1);
  assert.equal(Hex.indexOf(hay, bytes(3, 4, 1)), 5);
  assert.equal(Hex.indexOf(bytes(), p), -1);
  assert.equal(Hex.lastIndexOf(hay, p), 7);
  assert.equal(Hex.lastIndexOf(hay, p, 6), 3);
  assert.equal(Hex.lastIndexOf(hay, p, 2), 0);
  assert.equal(Hex.lastIndexOf(hay, bytes(9)), -1);
  assert.equal(Hex.lastIndexOf(hay, bytes(1, 2, 3, 4, 1)), 3);
  // A single byte, at the start and at the end.
  assert.equal(Hex.indexOf(bytes(5, 0, 5), bytes(5), 1), 2);
  assert.equal(Hex.lastIndexOf(bytes(5, 0, 5), bytes(5), 1), 0);
});

test("a long file's rows map onto a page that can only be so tall", () => {
  const small = Hex.rows(1000, 16, 20, 1_000_000);
  assert.deepEqual([small.count, small.scale, small.height], [63, 1, 1260]);
  assert.equal(small.rowAt(0), 0);
  assert.equal(small.rowAt(1259), 62, "never past the last row");
  assert.equal(small.topOf(10), 200);
  // 4 GB: 268 million rows at 20 px is far over the limit, so rows go by faster than pixels.
  const huge = Hex.rows(4 * 1024 ** 3, 16, 20, 10_000_000);
  assert.equal(huge.count, 268_435_456);
  assert.ok(huge.scale > 500 && huge.height <= 10_000_000);
  assert.equal(huge.rowAt(0), 0);
  assert.equal(huge.rowAt(huge.height), huge.count - 1);
  // The middle of the bar is the middle of the file, and a row maps back close to where it came from.
  const mid = huge.rowAt(huge.height / 2);
  assert.ok(Math.abs(mid / huge.count - 0.5) < 0.001);
  assert.ok(Math.abs(huge.rowAt(huge.topOf(123_456_789)) - 123_456_789) <= Math.ceil(huge.scale));
  // An empty file still has a row to draw.
  assert.equal(Hex.rows(0, 16, 20, 1000).count, 1);
});

test("bytes are shown as hex digits, a printable character or a dot", () => {
  assert.equal(Hex.hex2(0), "00");
  assert.equal(Hex.hex2(255), "ff");
  assert.equal(Hex.hex2(10), "0a");
  assert.deepEqual([0x1f, 0x20, 0x41, 0x7e, 0x7f, 0x80, 0xff].map(Hex.printable), [
    ".",
    " ",
    "A",
    "~",
    ".",
    ".",
    ".",
  ]);
  assert.equal(Hex.digits(1000), 8);
  assert.equal(Hex.digits(0xffffffff), 8);
  assert.equal(Hex.digits(0x100000000), 12);
  assert.equal(Hex.pad(255, 8), "000000FF");
});

test("a byte is described as numbers, with wider ones only when the bytes are there", () => {
  const d = (b, i) => plain(Hex.inspect(bytes(...b), i));
  assert.deepEqual(d([0xff], 0), ["u8 255", "i8 -1", "bin 11111111"]);
  assert.deepEqual(d([1, 2, 3, 4], 0), [
    "u8 1",
    "i8 1",
    "bin 00000001",
    "u16 le 513",
    "u16 be 258",
    "u32 le 67305985",
    "u32 be 16909060",
  ]);
  // Near the end of what is loaded, only what fits.
  assert.equal(d([1, 2, 3], 1).length, 5);
  assert.equal(d([1, 2, 3], 2).length, 3);
});

test("the hex viewer plugin is a valid preview that asks only to read", async () => {
  const { parseManifest } = await import("../../src/lib/plugins.ts");
  const dir = new URL("../../src-tauri/resources/plugins/hex-viewer/", import.meta.url);
  const r = parseManifest("hex-viewer", readFileSync(new URL("plugin.json", dir), "utf8"));
  assert.ok(r.ok, r.error);
  assert.deepEqual(r.manifest.permissions, ["read"]);
  const page = readFileSync(new URL("index.html", dir), "utf8");
  assert.match(page, /<script src="hex\.js"><\/script>/);
  assert.match(page, /m\.preview === "hex"/);
  assert.match(page, /readBytes/);
});
