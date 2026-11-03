import assert from "node:assert/strict";
import { test } from "node:test";
import { meaningful } from "../release-notes.mjs";

test("generated notes with only a changelog link say nothing", () => {
  assert.equal(meaningful("## What's Changed\n\n**Full Changelog**: https://x/compare/a...b"), "");
  assert.match(meaningful("## What's Changed\n* Fix a thing in https://x/pull/1\n"), /Fix a thing/);
});
