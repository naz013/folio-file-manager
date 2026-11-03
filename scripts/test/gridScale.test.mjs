import assert from "node:assert/strict";
import { test } from "node:test";
import {
  DEFAULT_SCALE,
  MAX_SCALE,
  MIN_SCALE,
  SCALE_STOPS,
  clampScale,
  fetchSize,
  nearestStop,
  readScale,
  scrollToKeep,
  stepScale,
  tileSize,
} from "../../src/lib/gridScale.ts";

test("the default scale is the design's tile", () => {
  assert.deepEqual(tileSize(1), {
    w: 132,
    h: 150,
    thumbW: 112,
    thumbH: 88,
    picture: 72,
    folder: 64,
  });
});

test("tiles grow and shrink with the scale and stay inside the limits", () => {
  assert.ok(tileSize(2).w > tileSize(1).w && tileSize(0.7).h < tileSize(1).h);
  assert.deepEqual(tileSize(100), tileSize(MAX_SCALE));
  assert.deepEqual(tileSize(-3), tileSize(MIN_SCALE));
  // The name always fits under the picture, however small.
  for (const s of SCALE_STOPS) assert.ok(tileSize(s).h - tileSize(s).thumbH >= 62);
});

test("a saved value is used only if it is a number, and is held to the limits", () => {
  assert.equal(readScale(1.3), 1.3);
  assert.equal(readScale(9), MAX_SCALE);
  for (const bad of ["1", null, undefined, NaN, Infinity, {}]) {
    assert.equal(readScale(bad), DEFAULT_SCALE, String(bad));
  }
  assert.equal(clampScale(Infinity), DEFAULT_SCALE);
});

test("a key press steps up or down, stops at the limits and resets to 1", () => {
  assert.ok(stepScale(1, 1) > 1 && stepScale(1, -1) < 1);
  assert.equal(stepScale(MAX_SCALE, 1), MAX_SCALE);
  assert.equal(stepScale(MIN_SCALE, -1), MIN_SCALE);
  assert.equal(stepScale(1.7, 0), DEFAULT_SCALE);
  // Up then down comes back.
  assert.ok(Math.abs(stepScale(stepScale(1, 1), -1) - 1) < 1e-12);
});

test("picture requests come in steps, so a small change asks for nothing new", () => {
  // The usual size asks for what it always has.
  assert.equal(fetchSize(72), 72);
  assert.equal(fetchSize(73), 80);
  assert.equal(fetchSize(80), 80);
  assert.equal(fetchSize(81), 88);
  assert.equal(fetchSize(1), 16);
  // Within a step, the same request.
  assert.equal(fetchSize(tileSize(1.05).picture), fetchSize(tileSize(1.1).picture));
});

test("the slider's stops are found for any scale", () => {
  assert.equal(SCALE_STOPS[nearestStop(1)], 1);
  assert.equal(SCALE_STOPS[nearestStop(0.1)], MIN_SCALE);
  assert.equal(SCALE_STOPS[nearestStop(50)], MAX_SCALE);
  assert.equal(SCALE_STOPS[nearestStop(1.3)], 1.2);
  assert.deepEqual(
    [...SCALE_STOPS].sort((a, b) => a - b),
    [...SCALE_STOPS],
  );
});

const geo = (scale, cols) => {
  const t = tileSize(scale);
  return { padX: 24, padY: 20, tileW: t.w, gapX: 6, rowH: t.h + 8, cols };
};

test("the item under the pointer stays under it when the tiles change size", () => {
  const before = geo(1, 6);
  const after = geo(1.5, 4);
  // The pointer is on the 14th item (row 2, column 1), a third of the way down its row.
  const row = 2;
  const y = before.padY + (row + 1 / 3) * before.rowH;
  const x = before.padX + 1 * (before.tileW + before.gapX) + 10;
  const viewY = 200;
  const top = scrollToKeep(before, after, { x, y }, viewY, 500, 100_000, 600);
  const index = row * 6 + 1;
  const newTop = after.padY + Math.floor(index / 4) * after.rowH;
  // That item's row is at the same distance from the top of the window, give or take the
  // part of the row the point was in.
  const fromTop = newTop + after.rowH / 3 - top;
  assert.ok(Math.abs(fromTop - viewY) < 1, String(fromTop));
});

test("the scroll position stays inside the list and an empty one stays at the top", () => {
  const before = geo(1, 6);
  const after = geo(0.6, 9);
  assert.equal(scrollToKeep(before, after, { x: 30, y: 30 }, 500, 0, 100, 100), 0);
  // Near the top, the item would sit above the window's top: not below zero.
  assert.equal(scrollToKeep(before, after, { x: 30, y: 30 }, 500, 50, 10_000, 600), 0);
  // At the end of a short list: not past what can scroll.
  const end = scrollToKeep(before, after, { x: 30, y: 900 }, 10, 50, 1000, 600);
  assert.ok(end <= 400 && end >= 0, String(end));
  // A pointer left of the first column or past the last row picks the nearest item.
  assert.ok(Number.isFinite(scrollToKeep(before, after, { x: -50, y: 99_999 }, 10, 50, 5000, 600)));
});
