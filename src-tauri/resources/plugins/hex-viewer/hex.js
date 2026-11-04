"use strict";
// What the hex viewer needs to know that doesn't touch the page: reading what a person types
// (an offset, bytes or text to look for), finding bytes in a piece of a file, and mapping a scroll
// position to a row for files with more rows than a page can be made tall enough for.
(function (root) {
  const MAX_PATTERN = 64;

  /** A byte offset typed as `4096`, `0x1000` or `1000h`; null if it isn't one. */
  function parseOffset(text) {
    const s = String(text).trim();
    let n = null;
    if (/^0x[0-9a-f]+$/i.test(s)) n = parseInt(s.slice(2), 16);
    else if (/^[0-9a-f]+h$/i.test(s)) n = parseInt(s.slice(0, -1), 16);
    else if (/^[0-9]+$/.test(s)) n = parseInt(s, 10);
    return Number.isSafeInteger(n) ? n : null;
  }

  /**
   * Bytes to look for, from what a person typed: hex (`de ad be ef`, `DEADBEEF`, `0xde,0xad`) or
   * text (as UTF-8). `{ bytes }` or `{ error }`.
   */
  function parsePattern(text, mode) {
    if (mode === "text") {
      const bytes = new TextEncoder().encode(text);
      if (!bytes.length) return { error: "Type some text to look for" };
      if (bytes.length > MAX_PATTERN) return { error: `At most ${MAX_PATTERN} bytes` };
      return { bytes };
    }
    const hex = text.replace(/0x/gi, "").replace(/[\s,]+/g, "");
    if (!hex) return { error: "Type bytes to look for, like de ad be ef" };
    if (!/^[0-9a-f]+$/i.test(hex)) return { error: "Bytes are written in hex, like de ad be ef" };
    if (hex.length % 2) return { error: "Bytes come in pairs of hex digits" };
    if (hex.length / 2 > MAX_PATTERN) return { error: `At most ${MAX_PATTERN} bytes` };
    const bytes = new Uint8Array(hex.length / 2);
    for (let i = 0; i < bytes.length; i++) bytes[i] = parseInt(hex.slice(i * 2, i * 2 + 2), 16);
    return { bytes };
  }

  /** The first place `pattern` is found in `chunk` at or after `from`, or -1. */
  function indexOf(chunk, pattern, from = 0) {
    const m = pattern.length;
    for (
      let i = chunk.indexOf(pattern[0], from);
      i !== -1 && i + m <= chunk.length;
      i = chunk.indexOf(pattern[0], i + 1)
    ) {
      let k = 1;
      while (k < m && chunk[i + k] === pattern[k]) k++;
      if (k === m) return i;
    }
    return -1;
  }

  /** The last place `pattern` is found in `chunk` that starts at or before `from`, or -1. */
  function lastIndexOf(chunk, pattern, from = chunk.length) {
    const m = pattern.length;
    let i = chunk.lastIndexOf(pattern[0], Math.min(from, chunk.length - m));
    for (; i !== -1; i = i === 0 ? -1 : chunk.lastIndexOf(pattern[0], i - 1)) {
      let k = 1;
      while (k < m && chunk[i + k] === pattern[k]) k++;
      if (k === m) return i;
    }
    return -1;
  }

  const hex2 = (b) => (b < 16 ? "0" : "") + b.toString(16);

  /** A byte as the character shown beside the hex: itself if printable ASCII, else a dot. */
  const printable = (b) => (b >= 0x20 && b < 0x7f ? String.fromCharCode(b) : ".");

  /** How many hex digits the offsets of a file of `size` bytes are shown with. */
  const digits = (size) => (size > 0xffffffff ? 12 : 8);

  const pad = (n, d) => n.toString(16).toUpperCase().padStart(d, "0");

  /**
   * Rows in a file, and how the scroll bar maps onto them. A page can only be made so tall, so a
   * very large file has more rows than pixels: `scale` rows then go by for each row's height of
   * scrolling, and the viewer draws the rows from `rowAt(scrollTop)` on.
   */
  function rows(size, perRow, rowH, maxPx, viewport = 0) {
    const count = Math.max(1, Math.ceil(size / perRow));
    const full = count * rowH;
    const scale = Math.max(1, full / maxPx);
    const height = Math.ceil(full / scale);
    // The rows a screen holds, and the last scroll position: the bar's end must show the end.
    const visible = Math.ceil(viewport / rowH);
    const maxRow = Math.max(0, count - visible);
    const maxTop = Math.max(0, height - viewport);
    const squeezed = scale > 1 && maxTop > 0 && viewport > 0;
    return {
      count,
      scale,
      height,
      rowAt: (top) =>
        squeezed
          ? Math.min(maxRow, Math.max(0, Math.floor((top / maxTop) * maxRow)))
          : Math.min(count - 1, Math.max(0, Math.floor((top * scale) / rowH))),
      topOf: (row) =>
        squeezed && maxRow > 0
          ? Math.min(maxTop, Math.floor((row / maxRow) * maxTop))
          : Math.floor((row * rowH) / scale),
    };
  }

  /** What a byte and the ones after it read as, for the line under the table. */
  function inspect(bytes, i) {
    const out = [];
    const b = bytes[i];
    out.push(`u8 ${b}`, `i8 ${b > 127 ? b - 256 : b}`, `bin ${b.toString(2).padStart(8, "0")}`);
    if (i + 2 <= bytes.length) {
      const v = new DataView(bytes.buffer, bytes.byteOffset + i, 2);
      out.push(`u16 le ${v.getUint16(0, true)}`, `u16 be ${v.getUint16(0, false)}`);
    }
    if (i + 4 <= bytes.length) {
      const v = new DataView(bytes.buffer, bytes.byteOffset + i, 4);
      out.push(`u32 le ${v.getUint32(0, true)}`, `u32 be ${v.getUint32(0, false)}`);
    }
    return out;
  }

  root.Hex = {
    parseOffset,
    parsePattern,
    indexOf,
    lastIndexOf,
    hex2,
    printable,
    digits,
    pad,
    rows,
    inspect,
    MAX_PATTERN,
  };
})(typeof globalThis !== "undefined" ? globalThis : window);
