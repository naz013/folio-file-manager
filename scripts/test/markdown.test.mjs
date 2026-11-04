import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import vm from "node:vm";

// The parser is a plain script in the plugin's folder; it is loaded the way a page loads it.
const file = new URL(
  "../../src-tauri/resources/plugins/markdown-viewer/markdown.js",
  import.meta.url,
);
const ctx = vm.createContext({});
vm.runInContext(readFileSync(file, "utf8"), ctx);
const { Markdown } = ctx;

/** Plain-object copy, so deepEqual doesn't trip over the sandbox's prototypes. */
const plain = (v) => JSON.parse(JSON.stringify(v));
const blocks = (md) => plain(Markdown.parse(md).blocks);
const inl = (s, defs = new Map()) => plain(Markdown.inline(s, defs, 0, false));
const text = (v) => ({ t: "text", v });

test("headings of both kinds, rules and paragraphs", () => {
  assert.deepEqual(blocks("# One\n\n## Two ##\n\n###### Six\n\n#nospace"), [
    { t: "h", level: 1, text: "One" },
    { t: "h", level: 2, text: "Two" },
    { t: "h", level: 6, text: "Six" },
    { t: "p", text: "#nospace" },
  ]);
  assert.deepEqual(blocks("Title\n=====\n\nSub\n---\n\n---\n***\n- - -"), [
    { t: "h", level: 1, text: "Title" },
    { t: "h", level: 2, text: "Sub" },
    { t: "hr" },
    { t: "hr" },
    { t: "hr" },
  ]);
  assert.deepEqual(blocks("one\ntwo\n\nthree"), [
    { t: "p", text: "one\ntwo" },
    { t: "p", text: "three" },
  ]);
});

test("code blocks keep their text as it is", () => {
  assert.deepEqual(
    blocks("```js\nlet a = <b>1</b>;\n# not a heading\n```\n\n    indented\n\n~~~\nx\n~~~"),
    [
      { t: "code", lang: "js", text: "let a = <b>1</b>;\n# not a heading" },
      { t: "code", lang: "", text: "indented" },
      { t: "code", lang: "", text: "x" },
    ],
  );
  // An unclosed fence runs to the end of the file rather than swallowing nothing.
  assert.deepEqual(blocks("```\nopen"), [{ t: "code", lang: "", text: "open" }]);
  // A longer fence is not closed by a shorter one.
  assert.deepEqual(blocks("````\n```\n````"), [{ t: "code", lang: "", text: "```" }]);
});

test("lists: nesting, numbering, tight and loose, tasks", () => {
  const [list] = blocks("- a\n  - b\n  - c\n- d");
  assert.equal(list.t, "list");
  assert.equal(list.tight, true);
  assert.equal(list.items.length, 2);
  assert.deepEqual(
    list.items[0].kids.map((k) => k.t),
    ["p", "list"],
  );
  assert.equal(list.items[0].kids[1].items.length, 2);

  const [ol] = blocks("3. x\n4. y");
  assert.deepEqual([ol.ordered, ol.start, ol.items.length], [true, 3, 2]);

  const [loose] = blocks("- a\n\n- b");
  assert.equal(loose.tight, false);

  const [tasks] = blocks("- [ ] open\n- [x] done\n- plain");
  assert.deepEqual(
    tasks.items.map((i) => i.task),
    [false, true, null],
  );
  assert.deepEqual(tasks.items[1].kids, [{ t: "p", text: "done" }]);

  // A different bullet starts another list; text indented under an item belongs to it.
  assert.equal(blocks("- a\n* b").length, 2);
  const [para] = blocks("- one\n\n  more\n- two");
  assert.deepEqual(
    para.items[0].kids.map((k) => k.text),
    ["one", "more"],
  );
});

test("quotes, tables and link definitions", () => {
  const [q] = blocks("> a\n> b\nlazy\n\n> > deep");
  assert.equal(q.t, "quote");
  assert.deepEqual(q.kids, [{ t: "p", text: "a\nb\nlazy" }]);
  assert.equal(blocks("> > deep")[0].kids[0].t, "quote");

  const [table] = blocks("| a | b | c |\n|:--|:-:|--:|\n| 1 | 2 |\n| x \\| y | 3 | 4 | 5 |");
  assert.deepEqual(table.align, ["left", "center", "right"]);
  assert.deepEqual(table.head, ["a", "b", "c"]);
  assert.deepEqual(table.rows, [
    ["1", "2", ""],
    ["x | y", "3", "4"],
  ]);
  // Pipes without a separator row are just text.
  assert.equal(blocks("a | b\nc | d")[0].t, "p");

  const parsed = Markdown.parse('[home]: https://example.com "Home"\n\nsee [home]');
  assert.deepEqual(plain(parsed.blocks), [{ t: "p", text: "see [home]" }]);
  assert.deepEqual(
    inl("see [home] and [x][home]", parsed.defs).filter((n) => n.t === "link").length,
    2,
  );
});

test("inline: emphasis, code, breaks, escapes", () => {
  assert.deepEqual(inl("a *b* **c** ***d*** ~~e~~"), [
    text("a "),
    { t: "em", kids: [text("b")] },
    text(" "),
    { t: "strong", kids: [text("c")] },
    text(" "),
    { t: "strong", kids: [{ t: "em", kids: [text("d")] }] },
    text(" "),
    { t: "del", kids: [text("e")] },
  ]);
  assert.deepEqual(inl("**a *b* c**"), [
    { t: "strong", kids: [text("a "), { t: "em", kids: [text("b")] }, text(" c")] },
  ]);
  // Snake case and stray stars stay as they are.
  assert.deepEqual(inl("snake_case_name 2 * 3 * 4 **open"), [
    text("snake_case_name 2 * 3 * 4 **open"),
  ]);
  assert.deepEqual(inl("`a *b* <c>` and ``x ` y``"), [
    { t: "code", v: "a *b* <c>" },
    text(" and "),
    { t: "code", v: "x ` y" },
  ]);
  assert.deepEqual(inl("a  \nb\\\nc\nd"), [
    text("a"),
    { t: "br" },
    text("b"),
    { t: "br" },
    text("c d"),
  ]);
  assert.deepEqual(inl("\\*not\\* \\# \\q"), [text("*not* # \\q")]);
});

test("inline: links and images are data, never markup", () => {
  assert.deepEqual(inl('[a **b**](https://x.test/p(1) "T") ![alt](pic.png)'), [
    {
      t: "link",
      href: "https://x.test/p(1)",
      title: "T",
      kids: [text("a "), { t: "strong", kids: [text("b")] }],
    },
    text(" "),
    { t: "img", src: "pic.png", alt: "alt", title: "" },
  ]);
  assert.deepEqual(inl("<https://a.test> and <me@a.test>"), [
    { t: "link", href: "https://a.test", title: "", kids: [text("https://a.test")] },
    text(" and "),
    { t: "link", href: "mailto:me@a.test", title: "", kids: [text("me@a.test")] },
  ]);
  assert.deepEqual(inl("see https://a.test/x, ok (https://b.test/y)."), [
    text("see "),
    { t: "link", href: "https://a.test/x", title: "", kids: [text("https://a.test/x")] },
    text(", ok ("),
    { t: "link", href: "https://b.test/y", title: "", kids: [text("https://b.test/y")] },
    text(")."),
  ]);
  // A link inside a link is text.
  const nested = inl("[a [b](c) d](e)");
  assert.equal(nested[0].t, "link");
  assert.ok(!JSON.stringify(nested[0].kids).includes('"t":"link"'));
  // Unclosed brackets stay as typed.
  assert.deepEqual(inl("[a](b and [c"), [text("[a](b and [c")]);
});

test("HTML and script-like text is only ever text", () => {
  const evil = [
    "<script>alert(1)</script>",
    "<img src=x onerror=alert(1)>",
    "[x](javascript:alert(1))",
    '<a href="javascript:alert(1)">x</a>',
    "![x](javascript:alert(1))",
    "<!-- hidden --> shown",
  ].join("\n\n");
  const { blocks: bs } = Markdown.parse(evil);
  const walk = (nodes) =>
    nodes.flatMap((n) => [
      n,
      ...(n.kids ? walk(n.kids) : []),
      ...(n.items ? n.items.flatMap((i) => walk(i.kids)) : []),
    ]);
  const all = walk(plain(bs));
  // No node kind carries HTML: only the kinds the page draws with textContent.
  const kinds = new Set(all.map((n) => n.t));
  for (const k of kinds)
    assert.ok(["p", "h", "code", "quote", "list", "hr", "table"].includes(k), k);
  const texts = bs
    .flatMap((b) => inl(b.text))
    .filter((n) => n.t === "text")
    .map((n) => n.v)
    .join("|");
  assert.ok(texts.includes("<script>alert(1)</script>"), "the tag is text");
  assert.ok(!texts.includes("hidden") && texts.includes("shown"), "a comment is dropped");
  // A javascript: link is a link node to show, and nothing here can open it.
  const link = inl("[x](javascript:alert(1))")[0];
  assert.deepEqual([link.t, link.href], ["link", "javascript:alert(1)"]);
});

test("hostile or huge input finishes quickly and doesn't throw", () => {
  const started = Date.now();
  const inputs = [
    "*a ".repeat(5000),
    "[".repeat(5000),
    "`".repeat(5000),
    "> ".repeat(500) + "x",
    "- ".repeat(300) + "x",
    "- a\n" + "  ".repeat(1) + "- b\n".repeat(3000),
    "| a |\n|---|\n" + "| x |\n".repeat(5000),
    "**".repeat(3000),
    "_a".repeat(4000),
    "a\n".repeat(20000),
    "\u0000￾\ud800".repeat(100),
    "![".repeat(2000) + "](" + "(".repeat(2000),
  ];
  for (const md of inputs) {
    const { blocks: bs } = Markdown.parse(md);
    assert.ok(Array.isArray(bs));
    for (const b of bs) if (b.text) Markdown.inline(b.text, new Map(), 0, false);
  }
  assert.ok(Date.now() - started < 5000, `took ${Date.now() - started} ms`);
});

test("the Markdown viewer plugin is a valid preview that asks only to read", async () => {
  const { parseManifest } = await import("../../src/lib/plugins.ts");
  const dir = new URL("../../src-tauri/resources/plugins/markdown-viewer/", import.meta.url);
  const r = parseManifest("markdown-viewer", readFileSync(new URL("plugin.json", dir), "utf8"));
  assert.ok(r.ok, r.error);
  assert.deepEqual(r.manifest.permissions, ["read"]);
  const page = readFileSync(new URL("index.html", dir), "utf8");
  assert.match(page, /<script src="markdown\.js"><\/script>/);
  // Links are shown, never made into elements that can navigate; nothing is built from markup.
  assert.ok(!/createElement\(["']a["']\)|el\(["']a["']/.test(page), "an anchor element");
  assert.ok(!/\.href\s*=/.test(page), "sets an href");
});

test("front matter at the top is shown as metadata, and a rule elsewhere is a rule", () => {
  assert.deepEqual(blocks("---\ntitle: x\ntags: [a]\n---\n\n# Hi"), [
    { t: "code", lang: "front matter", text: "title: x\ntags: [a]" },
    { t: "h", level: 1, text: "Hi" },
  ]);
  // No closing line: it is a rule and then text.
  assert.equal(blocks("---\njust text")[0].t, "hr");
  assert.equal(blocks("intro\n\n---\n\nmore")[1].t, "hr");
});
