import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { buildTree, counts, flatten, folderKeys } from "../../src/lib/archive.ts";
import { listableArchive, previewKind } from "../../src/lib/preview.ts";

const e = (path, size = 0, isDir = false, extra = {}) => ({
  path,
  isDir,
  size,
  modified: "2026-11-04 09:12",
  encrypted: false,
  ...extra,
});

test("the tree has the folders an archive implies and sums what is inside them", () => {
  const tree = buildTree([
    e("src/lib/a.ts", 100),
    e("src/lib/b.ts", 50),
    e("src/main.ts", 10),
    e("README.md", 5),
    e("empty/", 0, true),
  ]);
  // Folders first, then files, each in name order.
  assert.deepEqual(
    tree.map((n) => [n.name, n.isDir]),
    [
      ["empty", true],
      ["src", true],
      ["README.md", false],
    ],
  );
  const src = tree.find((n) => n.name === "src");
  assert.deepEqual([src.size, src.files], [160, 3], "all levels");
  assert.deepEqual(
    src.children.map((n) => n.name),
    ["lib", "main.ts"],
  );
  assert.deepEqual(counts(tree), { files: 4, folders: 3, bytes: 165 });
});

test("names with odd slashes make sense, and numbers sort as numbers", () => {
  const tree = buildTree([
    e("./a//b.txt", 1),
    e("f10.txt", 1),
    e("f2.txt", 1),
    e("/", 0, true),
    e("", 0),
  ]);
  assert.deepEqual(
    tree.map((n) => n.name),
    ["a", "f2.txt", "f10.txt"],
  );
  assert.equal(tree[0].children[0].name, "b.txt");
});

test("a folder is drawn open or closed, and only the open ones show their contents", () => {
  const tree = buildTree([e("a/b/c.txt", 1), e("a/d.txt", 1), e("z.txt", 1)]);
  assert.deepEqual(
    flatten(tree, new Set()).map((r) => r.node.name),
    ["a", "z.txt"],
  );
  const rows = flatten(tree, new Set(["a"]));
  assert.deepEqual(
    rows.map((r) => [r.node.name, r.depth]),
    [
      ["a", 0],
      ["b", 1],
      ["d.txt", 1],
      ["z.txt", 0],
    ],
  );
  assert.ok(rows[0].open && !rows[1].open);
  const all = flatten(tree, new Set(folderKeys(tree)));
  assert.equal(all.length, 5);
});

test("encrypted files mark the folders that hold them", () => {
  const tree = buildTree([e("a/x.txt", 1, false, { encrypted: true }), e("b/y.txt", 1)]);
  assert.equal(tree.find((n) => n.name === "a").encrypted, true);
  assert.equal(tree.find((n) => n.name === "b").encrypted, false);
});

test("zip, tar, tgz and tar.gz are listed by Quick Look; other archives are not", () => {
  for (const name of ["a.zip", "A.ZIP", "b.tar", "c.tgz", "d.tar.gz", "e.TAR.GZ"]) {
    assert.ok(listableArchive(name), name);
    assert.equal(previewKind(name, false), "archive", name);
  }
  for (const name of ["a.7z", "b.rar", "c.gz", "d.zipx", "zip"])
    assert.ok(!listableArchive(name), name);
  assert.equal(previewKind("folder.zip", true), "other");
});

test("Quick Look draws the list, and the list is a labelled tree a keyboard can use", () => {
  const ql = readFileSync(
    new URL("../../src/lib/components/QuickLook.svelte", import.meta.url),
    "utf8",
  );
  assert.match(ql, /kind === "archive"/);
  assert.match(ql, /\{#key path\}<ArchiveView \{path\} \/>\{\/key\}/);
  const view = readFileSync(
    new URL("../../src/lib/components/ArchiveView.svelte", import.meta.url),
    "utf8",
  );
  assert.match(view, /role="tree"/);
  assert.match(view, /role="treeitem"/);
  assert.match(view, /aria-expanded=/);
  assert.match(view, /aria-label="Contents of the archive"/);
  // Names are text, never markup.
  assert.ok(!/\{@html/.test(view), "no raw HTML in the list");
  for (const key of ["ArrowDown", "ArrowUp", "ArrowRight", "ArrowLeft", "Home", "End", "Enter"]) {
    assert.ok(view.includes(`"${key}"`), key);
  }
  // Only the rows in view are in the page.
  assert.match(view, /rows\.slice\(first, last\)/);
});

test("two things with one name each get a key of their own", () => {
  const keys = (nodes) => nodes.flatMap((n) => [n.key, ...keys(n.children)]);
  // A file and a folder of one name, a folder with an entry of its own, and the same file twice.
  const tree = buildTree([
    e("a", 1),
    e("a/b.txt", 2),
    e("x/", 0, true),
    e("x/y.txt", 1),
    e("dup.txt", 1),
    e("dup.txt", 2),
  ]);
  const all = keys(tree);
  assert.equal(new Set(all).size, all.length, all.join(" | "));
  assert.equal(tree.filter((n) => n.name === "dup.txt").length, 2, "both are shown");
  assert.equal(tree.filter((n) => n.name === "a").length, 2, "the file and the folder");
  // Opening one folder opens only that one.
  const a = tree.find((n) => n.name === "a" && n.isDir);
  const rows = flatten(tree, new Set([a.key]));
  assert.equal(rows.filter((r) => r.open).length, 1);
});

test("a big archive's tree is built fast, however its names are spread", () => {
  const timed = (entries) => {
    const start = performance.now();
    const tree = buildTree(entries);
    return { ms: performance.now() - start, tree };
  };
  // 10,000 folders side by side, each with two files: the worst case for finding a folder.
  const wide = [];
  for (let i = 0; i < 10000; i++) wide.push(e(`d${i}/a.txt`, 1), e(`d${i}/b.txt`, 1));
  const a = timed(wide);
  assert.equal(a.tree.length, 10000);
  // 20,000 files in one folder, which sorts a lot of names.
  const flat = Array.from({ length: 20000 }, (_, i) => e(`f${i}.txt`, 1));
  const b = timed(flat);
  assert.equal(b.tree.length, 20000);
  // Generous: the aim is to catch a walk over the siblings per entry (over a second here).
  assert.ok(a.ms < 600, `10k folders took ${Math.round(a.ms)} ms`);
  assert.ok(b.ms < 600, `20k files took ${Math.round(b.ms)} ms`);
});
