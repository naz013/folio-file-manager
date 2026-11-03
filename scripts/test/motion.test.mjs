import assert from "node:assert/strict";
import { test } from "node:test";
import { LIST_ANIMATION_MAX, animatesList, isMotion, motionReduced } from "../../src/lib/motion.ts";

test("the setting decides, and system defers to the OS", () => {
  assert.equal(motionReduced("reduced", false), true);
  assert.equal(motionReduced("full", true), false);
  assert.equal(motionReduced("system", true), true);
  assert.equal(motionReduced("system", false), false);
});

test("only the three choices are accepted", () => {
  assert.equal(isMotion("system"), true);
  assert.equal(isMotion("full"), true);
  assert.equal(isMotion("reduced"), true);
  assert.equal(isMotion("fast"), false);
  assert.equal(isMotion(undefined), false);
});

test("a long list or reduced motion skips list animation", () => {
  assert.equal(animatesList(10, false), true);
  assert.equal(animatesList(LIST_ANIMATION_MAX, false), true);
  assert.equal(animatesList(LIST_ANIMATION_MAX + 1, false), false);
  assert.equal(animatesList(10, true), false);
});
