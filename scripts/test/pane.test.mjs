import assert from "node:assert/strict";
import { test } from "node:test";
import { lastSegments, sideFolder, sideTarget, stepCursor } from "../../src/lib/pane.ts";

test("a spring target for the second pane carries its folder and nothing else does", () => {
  assert.equal(sideFolder(sideTarget("D:/Photos")), "D:/Photos");
  assert.equal(sideFolder("D:/Photos"), null);
  assert.equal(sideFolder("tab:3"), null);
  assert.equal(sideFolder("new"), null);
});

test("the cursor stays inside the list and starts at the near end", () => {
  assert.equal(stepCursor(-1, 1, 5), 0);
  assert.equal(stepCursor(-1, -1, 5), 4);
  assert.equal(stepCursor(2, 1, 5), 3);
  assert.equal(stepCursor(4, 1, 5), 4);
  assert.equal(stepCursor(0, -3, 5), 0);
  assert.equal(stepCursor(1, 99, 5), 4);
  assert.equal(stepCursor(-1, 1, 0), -1);
});

test("a long path shows its last folders and says it left some out", () => {
  assert.deepEqual(lastSegments([1, 2, 3, 4], 3), { shown: [2, 3, 4], more: true });
  assert.deepEqual(lastSegments([1, 2], 3), { shown: [1, 2], more: false });
});
