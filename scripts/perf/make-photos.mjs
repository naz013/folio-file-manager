// Creates a benchmark folder of N distinct pictures, for the gallery (docs/PERFORMANCE.md).
// Usage: node scripts/perf/make-photos.mjs <root> <count>
// Every picture is different, because the shell caches thumbnails by content: one file copied
// 10,000 times would draw once and measure nothing. Existing folders with the right number of
// files are reused.

import { existsSync, mkdirSync, readdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { deflateSync } from "node:zlib";

const [root, raw] = process.argv.slice(2);
const count = Number(raw);
if (!root || !Number.isInteger(count) || count < 1) {
  console.error("usage: make-photos.mjs <root> <count>");
  process.exit(1);
}

const W = 240;
const H = 160;

const TABLE = Array.from({ length: 256 }, (_, n) => {
  let c = n;
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c >>> 0;
});

function crc32(buf) {
  let c = 0xffffffff;
  for (const b of buf) c = TABLE[(c ^ b) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

function chunk(type, data) {
  const head = Buffer.alloc(8);
  head.writeUInt32BE(data.length, 0);
  head.write(type, 4, "ascii");
  const tail = Buffer.alloc(4);
  tail.writeUInt32BE(crc32(Buffer.concat([head.subarray(4), data])), 0);
  return Buffer.concat([head, data, tail]);
}

/** A PNG whose colours and stripes depend on `n`, so no two are alike. */
function picture(n) {
  const rows = Buffer.alloc((W * 3 + 1) * H);
  const hue = (n * 47) % 256;
  const stripe = 8 + (n % 29);
  for (let y = 0; y < H; y++) {
    const row = y * (W * 3 + 1);
    rows[row] = 0; // filter: none
    for (let x = 0; x < W; x++) {
      const at = row + 1 + x * 3;
      const band = Math.floor((x + y + n) / stripe) % 2;
      rows[at] = (hue + (x * 255) / W) & 255;
      rows[at + 1] = (((n * 13) % 256) + (y * 255) / H + band * 40) & 255;
      rows[at + 2] = (255 - hue + band * 60 + ((n >> 4) & 31)) & 255;
    }
  }
  const header = Buffer.alloc(13);
  header.writeUInt32BE(W, 0);
  header.writeUInt32BE(H, 4);
  header[8] = 8; // bit depth
  header[9] = 2; // RGB
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk("IHDR", header),
    chunk("IDAT", deflateSync(rows, { level: 1 })),
    chunk("IEND", Buffer.alloc(0)),
  ]);
}

const dir = join(root, `photos-${count}`);
if (existsSync(dir) && readdirSync(dir).length === count) {
  console.log(`reuse ${dir}`);
  process.exit(0);
}
mkdirSync(dir, { recursive: true });
for (let i = 0; i < count; i++) {
  writeFileSync(join(dir, `IMG_${String(i).padStart(6, "0")}.png`), picture(i));
}
console.log(`created ${dir}`);
