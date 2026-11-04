import assert from "node:assert/strict";
import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { test } from "node:test";
import { load, parseManifest, thumbnailFor } from "../../src/lib/plugins.ts";

const base = {
  id: "demo",
  name: "Demo",
  version: "1.0.0",
  description: "x",
  permissions: [],
};
const parse = (extra) => parseManifest("demo", JSON.stringify({ ...base, ...extra }));
const good = { extensions: ["JSON", ".geojson"], image: "art/thumb.svg", color: "#F59F00" };

test("a thumbnail names the kinds of file, an image in the folder and a badge colour", () => {
  const r = parse({ thumbnails: [good] });
  assert.ok(r.ok, r.error);
  assert.deepEqual(r.manifest.thumbnails, [
    { extensions: ["json", "geojson"], image: "art/thumb.svg", color: "#f59f00" },
  ]);
  // The colour is optional, and a plugin may have none at all.
  assert.equal(
    parse({ thumbnails: [{ ...good, color: undefined }] }).manifest.thumbnails[0].color,
    "#8e8e93",
  );
  assert.deepEqual(parse({}).manifest.thumbnails, []);
});

test("a thumbnail that is not one is refused, and says what is wrong", () => {
  const bad = (t, text) => {
    const r = parse({ thumbnails: [t] });
    assert.ok(!r.ok, text);
    assert.match(r.error, new RegExp(text), r.error);
  };
  bad({ ...good, extensions: [] }, "extensions");
  bad({ ...good, extensions: ["a b"] }, "extensions");
  bad({ ...good, extensions: undefined }, "extensions");
  bad("json", "needs");
  for (const image of [
    "../x.svg",
    "/x.svg",
    "a\\x.svg",
    "C:/x.svg",
    "x.html",
    "x.js",
    "x",
    "x.svg.exe",
    5,
    undefined,
  ]) {
    bad({ ...good, image }, "image");
  }
  for (const color of ["red", "#fff", "#12345", "#1234567", "f59f00", "rgb(0,0,0)", 5, null]) {
    bad({ ...good, color }, "color");
  }
  assert.ok(!parse({ thumbnails: "json" }).ok);
  assert.ok(!parse({ thumbnails: Array.from({ length: 11 }, () => good) }).ok);
  const many = Array.from({ length: 41 }, (_, i) => `a${i}`);
  assert.ok(!parse({ thumbnails: [{ ...good, extensions: many.slice(0, 41) }] }).ok);
  assert.ok(parse({ thumbnails: [{ ...good, extensions: many.slice(0, 40) }] }).ok);
  for (const image of ["a.SVG", "a/b.png", "x.webp"])
    assert.ok(parse({ thumbnails: [{ ...good, image }] }).ok, image);
});

test("only a plugin that is on gives its thumbnail, to the kinds it names", () => {
  const found = (id, thumbnails) => ({
    id,
    manifest: JSON.stringify({ ...base, id, thumbnails }),
    error: null,
    digest: `d-${id}`,
    bundled: false,
  });
  const grants = { a: { version: "1.0.0", permissions: [], digest: "d-a" } };
  const list = load(
    [
      found("a", [{ extensions: ["json"], image: "a.svg", color: "#112233" }]),
      found("b", [{ extensions: ["json", "csv"], image: "b.svg", color: "#445566" }]),
    ],
    grants,
  );
  // Only `a` is approved: `b` is not asked, though it names the same kind.
  assert.equal(thumbnailFor(list, "json")?.plugin.id, "a");
  assert.equal(thumbnailFor(list, "json")?.thumb.image, "a.svg");
  assert.equal(thumbnailFor(list, "csv"), null);
  assert.equal(thumbnailFor(list, "txt"), null);
  assert.equal(thumbnailFor(list, ""), null);
});

test("every thumbnail a bundled plugin gives exists, is small, and can't run or fetch anything", () => {
  const root = new URL("../../src-tauri/resources/plugins/", import.meta.url);
  let seen = 0;
  for (const d of readdirSync(root, { withFileTypes: true }).filter((d) => d.isDirectory())) {
    const dir = new URL(`${d.name}/`, root);
    const r = parseManifest(d.name, readFileSync(new URL("plugin.json", dir), "utf8"));
    assert.ok(r.ok, r.error);
    for (const t of r.manifest.thumbnails) {
      seen++;
      const file = new URL(t.image, dir);
      assert.ok(existsSync(file), `${d.name}/${t.image} is missing`);
      assert.ok(statSync(file).size < 16 * 1024, `${d.name}/${t.image} is large`);
      if (!t.image.endsWith(".svg")) continue;
      const svg = readFileSync(file, "utf8");
      assert.match(svg, /^<svg[^>]*viewBox="0 0 56 72"/, `${d.name}: the page is 56 x 72`);
      // An image can't run code, but it may not reach for anything either.
      for (const bad of [
        /<script/i,
        /\son[a-z]+\s*=/i,
        /<foreignObject/i,
        /<image/i,
        /<use/i,
        /href\s*=/i,
        /url\(/i,
        /@import/i,
        /<style/i,
        /<!ENTITY/i,
      ]) {
        assert.ok(!bad.test(svg), `${d.name}/${t.image} contains ${bad}`);
      }
      assert.ok(
        !/https?:\/\/(?!www\.w3\.org\/2000\/svg)/.test(svg),
        `${d.name}/${t.image} names an address`,
      );
    }
  }
  assert.ok(seen >= 4, "the four bundled viewers each give a thumbnail");
});

test("a plugin's picture is chosen before Windows' own, which for plain files is only an icon", () => {
  const src = readFileSync(
    new URL("../../src/lib/components/Thumbnail.svelte", import.meta.url),
    "utf8",
  );
  const art = src.indexOf("{#if pluginArt}");
  const failed = src.indexOf("{:else if failed}");
  const img = src.indexOf("{:else if shown}");
  assert.ok(
    art > 0 && art < failed && failed < img,
    "plugin art, then the failed fallback, then the system picture",
  );
  // A caller that gives no name (a list row) still gets the picture: the name comes from the path.
  assert.match(src, /name \|\| path\.split\(/);
});
