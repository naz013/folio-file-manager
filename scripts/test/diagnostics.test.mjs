import assert from "node:assert/strict";
import { test } from "node:test";
import { diagnosticsText, reportUrl } from "../../src/lib/diagnostics.ts";

const line = "Folio 1.2.3 (abcdef1234, 2026-01-02) · Tauri 2.0.0 · WebView 130.0 · windows x86_64";
const env = { locale: "en-GB", monitors: [{ width: 2560, height: 1440, scale: 1.5 }] };

test("the diagnostics name the build, system, locale and screens", () => {
  const text = diagnosticsText(line, env, { consent: true, sessions: 9, crashed: 1 });
  assert.match(text, /Folio 1\.2\.3/);
  assert.match(text, /WebView 130\.0/);
  assert.match(text, /Locale: en-GB/);
  assert.match(text, /2560×1440 at 150%/);
  assert.match(text, /Sessions: 9, ended without a clean exit: 1/);
});

test("without consent the counts are not passed off as zero crashes", () => {
  const text = diagnosticsText(line, env, { consent: false, sessions: 0, crashed: 0 });
  assert.doesNotMatch(text, /Sessions: /);
  assert.match(text, /Crash reports: off/);
});

test("what is not known is said so", () => {
  const text = diagnosticsText("Folio 1.2.3", { locale: "", monitors: [] }, null);
  assert.match(text, /Locale: unknown/);
  assert.match(text, /Monitors: unknown/);
  assert.doesNotMatch(text, /Sessions: /);
});

test("the diagnostics hold no path", () => {
  const text = diagnosticsText(line, env, null);
  assert.doesNotMatch(text, /[A-Za-z]:\|\\|\/Users\//);
});

test("the report address opens a new issue with the diagnostics in it", () => {
  const url = new URL(reportUrl("https://example.org/r", "Folio 1.2.3 · windows"));
  assert.equal(url.pathname, "/r/issues/new");
  assert.match(url.searchParams.get("body") ?? "", /Folio 1\.2\.3 · windows/);
});
