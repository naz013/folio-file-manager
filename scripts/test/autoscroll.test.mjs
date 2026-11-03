import assert from "node:assert/strict";
import { test } from "node:test";
import { dragScroll, edgeStep, scrollableAxes } from "../../src/lib/autoscroll.ts";

const box = { left: 100, right: 500, top: 50, bottom: 350 };

test("the middle of the box does not scroll, the edges do, faster toward the edge", () => {
  assert.equal(edgeStep(300, 100, 500, 48, 24), 0);
  assert.equal(edgeStep(100, 100, 500, 48, 24), -24);
  assert.equal(edgeStep(500, 100, 500, 48, 24), 24);
  assert.ok(edgeStep(124, 100, 500, 48, 24) > -24, "slower further from the edge");
  assert.ok(edgeStep(124, 100, 500, 48, 24) < 0);
});

test("a pointer past the edge never scrolls faster than the maximum", () => {
  assert.equal(edgeStep(0, 100, 500, 48, 24), -24);
  assert.equal(edgeStep(900, 100, 500, 48, 24), 24);
});

test("a filmstrip scrolls sideways near its left and right edges", () => {
  const strip = { x: true, y: false };
  assert.ok(dragScroll(110, 200, box, strip, 48, 24).dx < 0);
  assert.ok(dragScroll(495, 200, box, strip, 48, 24).dx > 0);
  assert.equal(dragScroll(300, 200, box, strip, 48, 24).dx, 0);
});

test("a filmstrip does not scroll up or down, and a list does not scroll sideways", () => {
  assert.equal(dragScroll(300, 55, box, { x: true, y: false }, 48, 24).dy, 0);
  assert.equal(dragScroll(105, 200, box, { x: false, y: true }, 48, 24).dx, 0);
  assert.ok(dragScroll(300, 55, box, { x: false, y: true }, 48, 24).dy < 0);
});

test("a pointer outside the box scrolls the axis it is not between the sides of", () => {
  // Left of the box: the vertical zones do not count, the horizontal one does.
  assert.equal(dragScroll(20, 55, box, { x: true, y: true }, 48, 24).dy, 0);
  assert.equal(dragScroll(300, 20, box, { x: true, y: true }, 48, 24).dx, 0);
});

test("a list that hides its sideways overflow is not scrolled sideways", () => {
  const list = { scrollWidth: 803, clientWidth: 800, scrollHeight: 4000, clientHeight: 600 };
  assert.deepEqual(scrollableAxes(list, "hidden"), { x: false, y: true });
  // A rounding pixel is not an overflow, even where sideways scrolling is allowed.
  const pixel = { scrollWidth: 801, clientWidth: 800, scrollHeight: 4000, clientHeight: 600 };
  assert.deepEqual(scrollableAxes(pixel, "auto"), { x: false, y: true });
});

test("an element scrolls along an axis only where it overflows", () => {
  assert.deepEqual(
    scrollableAxes({ scrollWidth: 5000, clientWidth: 800, scrollHeight: 160, clientHeight: 160 }),
    { x: true, y: false },
  );
  assert.deepEqual(
    scrollableAxes({ scrollWidth: 800, clientWidth: 800, scrollHeight: 4000, clientHeight: 600 }),
    { x: false, y: true },
  );
});
