import assert from "node:assert/strict";
import { test } from "node:test";
import { parseMarkdown } from "../../src/lib/markdown.ts";
import { notesFor, shouldShowNotes } from "../../src/lib/whatsnew.ts";

test("headings, bullets and paragraphs", () => {
  const blocks = parseMarkdown("# New\n\nSome text\nmore\n\n- one\n- **two**\n\n## Fixes\n* three");
  assert.deepEqual(
    blocks.map((b) => b.kind),
    ["heading", "paragraph", "list", "heading", "list"],
  );
  assert.deepEqual(blocks[1].spans, [{ text: "Some text more" }]);
  assert.deepEqual(blocks[2].items[1], [{ text: "two", bold: true }]);
});

test("only web links become links, and tags are never markup", () => {
  const [p] = parseMarkdown("[ok](https://a.example/x) [bad](javascript:alert(1)) <b>x</b>");
  assert.equal(p.spans[0].href, "https://a.example/x");
  assert.ok(p.spans.every((s) => !s.href || s.href.startsWith("https://")));
  assert.ok(p.spans.some((s) => s.text.includes("<b>x</b>")));
});

test("a bare address is a link and code is code", () => {
  const [p] = parseMarkdown("See https://example.org/a and `x`");
  assert.equal(p.spans[1].href, "https://example.org/a");
  assert.deepEqual(p.spans[3], { text: "x", code: true });
});

test("bundled notes are only for the version they were made for", () => {
  const from = { version: "1.2.0", notes: "  Hello \n" };
  assert.equal(notesFor("1.2.0", from), "Hello");
  assert.equal(notesFor("1.3.0", from), "");
});

test("notes show once for an update, never on a fresh install", () => {
  const base = { onboarded: true, seen: "1.1.0", version: "1.2.0", notes: "x" };
  assert.equal(shouldShowNotes(base), true);
  assert.equal(shouldShowNotes({ ...base, seen: "1.2.0" }), false);
  assert.equal(shouldShowNotes({ ...base, onboarded: false, seen: "" }), false);
  assert.equal(shouldShowNotes({ ...base, notes: "" }), false);
});

test("a trailing full stop is not part of the address", () => {
  const [p] = parseMarkdown("Read https://example.org/notes.");
  assert.equal(p.spans[1].href, "https://example.org/notes");
  assert.equal(p.spans[2].text, ".");
});

test("numbered lists are lists, rules and fences are dropped", () => {
  const blocks = parseMarkdown("1. a\n2. b\n\n---\n\n```\n\ntext");
  assert.deepEqual(
    blocks.map((b) => b.kind),
    ["list", "paragraph"],
  );
});
