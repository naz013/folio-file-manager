import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { facts, hasDetails, placeText, sizeText } from "../../src/lib/imageinfo.ts";

const full = {
  width: 6000,
  height: 4000,
  camera: "Canon EOS R5",
  lens: "RF24-70mm F2.8 L IS USM",
  taken: "2026-11-04 09:12:30",
  exposure: "1/250 s · f/2.8 · ISO 100 · 35 mm",
  hasLocation: true,
  location: null,
};

test("the facts come in a fixed order, and what is missing is left out", () => {
  assert.deepEqual(
    facts(full).map((f) => f.label),
    ["Size", "Camera", "Lens", "Taken", "Exposure"],
  );
  assert.equal(facts(full)[0].text, "6000 × 4000 px");
  const sparse = { ...full, camera: null, lens: null, taken: null, exposure: null };
  assert.deepEqual(facts(sparse), [{ label: "Size", text: "6000 × 4000 px" }]);
  assert.deepEqual(facts({ ...sparse, width: null, height: null }), []);
  assert.equal(sizeText({ ...full, width: 0 }), null, "a zero size is not a size");
});

test("the place is never among the facts, and reads in the usual way", () => {
  const withPlace = { ...full, location: { lat: -33.85, lon: 151.21 } };
  assert.ok(!JSON.stringify(facts(withPlace)).includes("151"));
  assert.equal(placeText({ lat: -33.85, lon: 151.21 }), "33.85000° S, 151.21000° E");
  assert.equal(placeText({ lat: 48.8584, lon: -2.2945 }), "48.85840° N, 2.29450° W");
});

test("details are looked for in the pictures the core can read, by extension", () => {
  for (const name of ["a.jpg", "A.JPEG", "b.png", "c.webp", "d.heic", "e.HEIF", "f.tif"]) {
    assert.ok(hasDetails(name), name);
  }
  for (const name of ["a.gif", "b.svg", "c.txt", "jpg", ".png", "d"]) {
    assert.ok(!hasDetails(name), name);
  }
});

test("Quick Look and the inspector show the details, and the place only on request", () => {
  const comp = readFileSync(
    new URL("../../src/lib/components/ImageDetails.svelte", import.meta.url),
    "utf8",
  );
  // The place is asked for with a button, per file, and is not fetched before.
  assert.match(comp, /const withPlace = located;/);
  assert.match(comp, /Show location/);
  assert.match(comp, /Hide location/);
  // A failure shows nothing.
  assert.match(comp, /\.catch\(\(\) => current && \(info = null\)\)/);
  assert.match(comp, /\{#if info && \(rows\.length \|\| info\.hasLocation\)\}/);
  const ql = readFileSync(
    new URL("../../src/lib/components/QuickLook.svelte", import.meta.url),
    "utf8",
  );
  // Keyed on the file, so coming back to a file does not show its place again unasked.
  assert.match(ql, /\{#key path\}\s*<ImageDetails \{path\} layout="line" \/>/);
  const inspector = readFileSync(
    new URL("../../src/lib/components/Inspector.svelte", import.meta.url),
    "utf8",
  );
  assert.match(
    inspector,
    /\{#key item\.path\}\s*<ImageDetails path=\{item\.path\} layout="list" \/>/,
  );
  // The page asks the core, which never sends the place unless it is asked for.
  const api = readFileSync(new URL("../../src/lib/api/fs.ts", import.meta.url), "utf8");
  assert.match(api, /"read_image_info", \{ path, withLocation \}/);
  assert.match(api, /withLocation = false/);
});
