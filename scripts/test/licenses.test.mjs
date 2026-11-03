import assert from "node:assert/strict";
import { test } from "node:test";
import { filterPackages } from "../../src/lib/licenses.ts";
import { collect, plainText } from "../licenses.mjs";

const pkg = (name, license) => ({
  name,
  version: "1.0.0",
  source: "crate",
  license,
  repo: "",
  text: null,
});
const list = [pkg("serde", "MIT OR Apache-2.0"), pkg("zip", "MIT"), pkg("inter", "OFL-1.1")];

test("search matches the package name, in any case", () => {
  assert.deepEqual(
    filterPackages(list, "SER").map((p) => p.name),
    ["serde"],
  );
});

test("search also matches the license, and needs every word", () => {
  assert.deepEqual(
    filterPackages(list, "ofl").map((p) => p.name),
    ["inter"],
  );
  assert.deepEqual(
    filterPackages(list, "mit zip").map((p) => p.name),
    ["zip"],
  );
  assert.equal(filterPackages(list, "nothing").length, 0);
});

test("an empty search shows everything", () => {
  assert.equal(filterPackages(list, "  ").length, 3);
});

test("a dependency with a license that is not allowed is reported", () => {
  const { problems } = collect([
    {
      name: "gpl-thing",
      version: "2.0.0",
      source: "npm",
      license: "GPL-3.0-only",
      repo: "",
      dir: "",
    },
    {
      name: "fine",
      version: "1.0.0",
      source: "npm",
      license: "MIT",
      repo: "",
      authors: ["A"],
      dir: "",
    },
  ]);
  assert.equal(problems.length, 1);
  assert.match(problems[0], /gpl-thing 2\.0\.0/);
});

test("a package with no license at all is reported", () => {
  const { problems } = collect([
    { name: "nolic", version: "0.1.0", source: "crate", license: "", repo: "", dir: "" },
  ]);
  assert.equal(problems.length, 1);
  assert.match(problems[0], /no license declared/);
});

test("a package with no license file gets the standard text with its authors", () => {
  const data = collect([
    {
      name: "nofile",
      version: "0.2.0",
      source: "crate",
      license: "MIT",
      repo: "",
      authors: ["Ada Lovelace"],
      dir: "",
    },
  ]);
  assert.deepEqual(data.problems, []);
  const text = plainText(data);
  assert.match(text, /nofile 0\.2\.0 \(MIT\)/);
  assert.match(text, /Copyright \(c\) Ada Lovelace/);
  assert.match(text, /ships no license file/);
});

test("a missing license file with no authors to name fails the check", () => {
  const { problems } = collect([
    {
      name: "anon",
      version: "1.0.0",
      source: "crate",
      license: "MIT",
      repo: "",
      authors: [],
      dir: "",
    },
  ]);
  assert.equal(problems.length, 1);
  assert.match(problems[0], /ships no license file/);
});
