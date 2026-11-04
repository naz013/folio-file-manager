import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

const read = (p) => readFileSync(new URL(`../../${p}`, import.meta.url), "utf8");
const ql = read("src/lib/components/QuickLook.svelte");

test("a plugin's preview is always labelled by a bar Folio draws outside the plugin's page", () => {
  // The bar is in Quick Look's own template, after the stage that holds the plugin's frame and
  // before the footer, and only for a plugin preview.
  const stageEnd = ql.indexOf("<PluginFrame");
  const bar = ql.indexOf('class="source"');
  const footer = ql.indexOf("<footer>");
  assert.ok(
    stageEnd > 0 && bar > stageEnd && footer > bar,
    "plugin frame, then the bar, then the footer",
  );
  const block = ql.slice(ql.lastIndexOf("{#if", bar), footer);
  assert.match(block, /shown\.kind === "plugin"/);
  // It names the plugin, says whether it is built in, and offers Folio's own view of the file.
  assert.match(block, /shown\.hit\.plugin\.manifest\.name/);
  assert.match(block, /shown\.hit\.plugin\.bundled/);
  assert.match(block, /Preview by/);
  assert.match(block, /Built-in plugin/);
  assert.match(block, /Show normally/);
  assert.match(block, /skipped = path/);
  // The name is text, never markup, and the bar is a labelled group for screen readers.
  assert.ok(!/\{@html/.test(ql), "no raw HTML in Quick Look");
  assert.match(block, /role="group"/);
  assert.match(block, /aria-label="Preview by \{who\}/);
});

test("the bar takes its own row of the window only while a plugin draws the preview", () => {
  assert.match(ql, /class:with-source=\{shown\.kind === "plugin"\}/);
  assert.match(ql, /\.window\.with-source\s*\{\s*grid-template-rows: auto 1fr auto auto;/);
  // Colours and sizes come from the tokens, never written out.
  const css = ql.slice(ql.indexOf(".source {"), ql.indexOf("</style>"));
  assert.ok(!/#[0-9a-f]{3,8}\b/i.test(css), "a colour written out in the bar's styles");
  assert.match(css, /var\(--status-warning\)/);
});
