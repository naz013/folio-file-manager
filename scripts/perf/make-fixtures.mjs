// Creates benchmark folders with N small files of mixed types.
// Usage: node scripts/perf/make-fixtures.mjs <root> <count> [<count>...]
// Existing folders with the right number of entries are reused.

import { mkdirSync, readdirSync, writeFileSync, existsSync } from "node:fs";
import { join } from "node:path";

const [root, ...counts] = process.argv.slice(2);
if (!root || counts.length === 0) {
  console.error("usage: make-fixtures.mjs <root> <count> [<count>...]");
  process.exit(1);
}

const exts = ["txt", "pdf", "docx", "xlsx", "jpg", "png", "md", "zip", "mp4", "fig"];

for (const raw of counts) {
  const count = Number(raw);
  const dir = join(root, `items-${count}`);
  if (existsSync(dir) && readdirSync(dir).length === count) {
    console.log(`reuse ${dir}`);
    continue;
  }
  mkdirSync(dir, { recursive: true });
  const folders = Math.floor(count / 20); // ~5% folders, like real projects
  for (let i = 0; i < count; i++) {
    const name = i < folders ? `Folder ${String(i).padStart(6, "0")}` : `file-${String(i).padStart(6, "0")}.${exts[i % exts.length]}`;
    const path = join(dir, name);
    if (existsSync(path)) continue;
    if (i < folders) mkdirSync(path);
    else writeFileSync(path, "x");
  }
  console.log(`created ${dir}`);
}
