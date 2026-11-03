import assert from "node:assert/strict";
import { test } from "node:test";
import { isPinned, movedBy, movedTo, slotOver, withPinned } from "../../src/lib/favorites.ts";

const [a, b, c] = ["C:/a", "C:/b", "C:/c"];
const list = [a, b, c];

test("a folder is added at the slot, and not twice", () => {
  assert.deepEqual(withPinned(list, ["C:/x"], 1), [a, "C:/x", b, c]);
  assert.deepEqual(withPinned(list, ["C:/x"], 0), ["C:/x", a, b, c]);
  assert.deepEqual(withPinned(list, ["C:/x"]), [a, b, c, "C:/x"]);
  assert.deepEqual(withPinned(list, ["c:/B/", "C:/x", "C:/x"], 2), [a, b, "C:/x", c]);
  assert.deepEqual(withPinned(list, ["C:/x"], 99), [a, b, c, "C:/x"]);
  assert.deepEqual(withPinned(list, ["C:/x"], -3), ["C:/x", a, b, c]);
});

test("pinned is judged without case or a trailing separator", () => {
  assert.equal(isPinned(list, "c:/A/"), true);
  assert.equal(isPinned(list, "C:/z"), false);
  // A Windows path with backslashes and a trailing one.
  assert.equal(isPinned(["C:\\Users\\me\\Docs"], "c:\\users\\ME\\docs\\"), true);
});

test("a favorite moves to a slot counted in the list as it was", () => {
  assert.deepEqual(movedTo(list, a, 3), [b, c, a]);
  assert.deepEqual(movedTo(list, c, 0), [c, a, b]);
  assert.deepEqual(movedTo(list, b, 0), [b, a, c]);
  assert.deepEqual(movedTo(list, a, 2), [b, a, c]);
});

test("a slot next to the favorite itself changes nothing", () => {
  assert.deepEqual(movedTo(list, b, 1), list);
  assert.deepEqual(movedTo(list, b, 2), list);
  assert.deepEqual(movedTo(list, "C:/gone", 1), list);
});

test("move up and down stop at the ends", () => {
  assert.deepEqual(movedBy(list, b, -1), [b, a, c]);
  assert.deepEqual(movedBy(list, a, -1), list);
  assert.deepEqual(movedBy(list, c, 1), list);
});

test("the edges of an item are gaps and the middle is the folder", () => {
  const item = { index: 1 };
  assert.equal(slotOver(item, 100, 100, 30), 1);
  assert.equal(slotOver(item, 115, 100, 30), null);
  assert.equal(slotOver(item, 128, 100, 30), 2);
  assert.equal(slotOver({ fixed: true }, 128, 100, 30), null);
  assert.equal(slotOver({ fixed: true, last: true }, 128, 100, 30), 0);
  assert.equal(slotOver({ fixed: true, last: true }, 105, 100, 30), null);
});
