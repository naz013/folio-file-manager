import assert from "node:assert/strict";
import { test } from "node:test";
import { judge } from "../licenses-policy.mjs";

test("a single allowed license passes", () => {
  assert.deepEqual(judge("MIT"), { ok: true, ids: ["MIT"], problems: [] });
  assert.equal(judge("OFL-1.1").ok, true);
});

test("an OR passes when one side is allowed, and names that side", () => {
  const r = judge("GPL-3.0-only OR MIT");
  assert.equal(r.ok, true);
  assert.deepEqual(r.ids, ["MIT"]);
  assert.equal(judge("Apache-2.0 OR MIT").ok, true);
});

test("the old slash form of an OR is understood", () => {
  assert.equal(judge("MIT/Apache-2.0").ok, true);
  assert.equal(judge("GPL-3.0-only/LGPL-3.0-only").ok, false);
});

test("an AND needs every side", () => {
  assert.equal(judge("MIT AND Zlib").ok, true);
  const r = judge("MIT AND GPL-3.0-only");
  assert.equal(r.ok, false);
  assert.deepEqual(r.problems, ["GPL-3.0-only"]);
});

test("parentheses group", () => {
  assert.equal(judge("(MIT OR GPL-3.0-only) AND Zlib").ok, true);
  assert.equal(judge("MIT AND (GPL-3.0-only OR LGPL-3.0-only)").ok, false);
  assert.equal(judge("(MIT").ok, false);
});

test("only a known exception widens a license", () => {
  assert.equal(judge("Apache-2.0 WITH LLVM-exception").ok, true);
  assert.equal(judge("Apache-2.0 WITH Made-up-exception").ok, false);
});

test("unknown, missing or copyleft licenses fail", () => {
  assert.equal(judge("SEE LICENSE IN LICENSE.txt").ok, false);
  assert.equal(judge("GPL-3.0-only").ok, false);
  assert.equal(judge("").ok, false);
  assert.equal(judge(undefined).ok, false);
  assert.deepEqual(judge(undefined).problems, ["no license declared"]);
  assert.equal(judge("MIT OR").ok, false);
  assert.equal(judge("MIT MIT").ok, false);
});
