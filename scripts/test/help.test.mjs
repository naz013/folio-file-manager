import assert from "node:assert/strict";
import { test } from "node:test";
import { TOPICS, findTopics } from "../../src/lib/help.ts";
import { SHORTCUTS } from "../../src/lib/shortcuts.ts";

test("every topic has a unique id, a title and text", () => {
  const ids = new Set(TOPICS.map((t) => t.id));
  assert.equal(ids.size, TOPICS.length);
  for (const t of TOPICS) {
    assert.ok(t.title.trim() && t.body.length && t.body.every((p) => p.trim()));
  }
});

test("an empty query lists every topic", () => {
  assert.equal(findTopics("  ").length, TOPICS.length);
});

test("every word has to match, in the title or the text", () => {
  assert.deepEqual(
    findTopics("restore zzzz").map((t) => t.id),
    [],
  );
  assert.ok(findTopics("restore recycle").some((t) => t.id === "recycle-bin"));
  assert.deepEqual(findTopics("zzzz"), []);
});

test("the topic on deleting says files go to the Recycle Bin and can be undone", () => {
  const text = TOPICS.find((t) => t.id === "recycle-bin")?.body.join(" ") ?? "";
  assert.match(text, /Recycle Bin/);
  assert.match(text, /Ctrl\+Z/);
});

test("the keys the help quotes are in the shortcuts registry", () => {
  const registered = SHORTCUTS.map((s) => s.keys).join(" ");
  const quoted = TOPICS.flatMap(
    (t) => t.body.join(" ").match(/Ctrl\+(?:Shift\+)?(?:Tab|[A-Z0-9])/g) ?? [],
  );
  assert.ok(quoted.length > 5, "the help quotes keys");
  for (const keys of new Set(quoted)) {
    assert.ok(registered.includes(keys), `${keys} is not a registered shortcut`);
  }
});
