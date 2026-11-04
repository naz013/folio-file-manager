import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import vm from "node:vm";

const file = new URL("../../src-tauri/resources/plugins/hex-viewer/hex.js", import.meta.url);
const ctx = vm.createContext({ TextEncoder, DataView, Uint8Array, Math });
vm.runInContext(readFileSync(file, "utf8"), ctx);
const { Hex } = ctx;

test("the end of the scroll bar shows the end of a huge file, and a row maps back to its place", () => {
  const view = 700;
  const visible = Math.ceil(view / 20);
  const huge = Hex.rows(4 * 1024 ** 3, 16, 20, 8_000_000, view);
  assert.ok(huge.scale > 600);
  // The bar's last position shows the last screenful: the last row is on it.
  const last = huge.rowAt(huge.height - view);
  assert.equal(last, huge.count - visible);
  assert.ok(last + visible >= huge.count, "the last row is in view at the end");
  // Past the end of the bar changes nothing; the start is the start.
  assert.equal(huge.rowAt(huge.height + 5000), last);
  assert.equal(huge.rowAt(0), 0);
  assert.equal(huge.rowAt(-10), 0);
  // The middle of the bar is about the middle of the file.
  const mid = huge.rowAt((huge.height - view) / 2);
  assert.ok(Math.abs(mid / huge.count - 0.5) < 0.001);
  // A row goes to a bar position and back to within what one pixel covers.
  for (const row of [0, 1, 1000, 123_456_789, huge.count - visible]) {
    const back = huge.rowAt(huge.topOf(row));
    assert.ok(Math.abs(back - row) <= Math.ceil(huge.scale), `row ${row} came back as ${back}`);
  }
  // The bar position for the last row is the end of the bar, never beyond it.
  assert.ok(huge.topOf(huge.count - 1) <= huge.height - view);
});

test("a file that fits keeps one row per row of pixels, with or without a view height given", () => {
  for (const view of [0, 400, 700]) {
    const m = Hex.rows(10_000, 16, 20, 1_000_000, view);
    assert.equal(m.scale, 1);
    assert.equal(m.rowAt(0), 0);
    assert.equal(m.rowAt(200), 10);
    assert.equal(m.topOf(10), 200);
  }
  // A short file with a tall view: nothing to scroll, the first row is at the top.
  const short = Hex.rows(100, 16, 20, 1_000_000, 700);
  assert.equal(short.rowAt(0), 0);
  assert.equal(short.rowAt(50), 2);
});
