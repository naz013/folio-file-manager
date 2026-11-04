"use strict";
// Reading CSV and TSV text into rows of strings: quotes ("a,b", "say ""hi"""), line breaks inside
// quotes, CR LF or LF, a byte-order mark, and rows of different lengths. And ordering and
// filtering many rows without holding up the page: both work in slices of a few milliseconds and
// let the page breathe between them. Nothing here touches the page, so it is tested on its own.
(function (root) {
  const CANDIDATES = [",", "\t", ";", "|"];
  const NAMES = { ",": "Comma", "\t": "Tab", ";": "Semicolon", "|": "Pipe" };

  /**
   * Reads rows from `text` split at the delimiter `d` (one character). Blank lines are skipped. A
   * quote only opens a quoted field at the start of one; a quote inside an unquoted field is
   * text. A quote that is never closed takes the rest of the text.
   *
   * `final: false` is for a piece of a longer text: a last row that the piece ends in the middle
   * of is not returned, and `consumed` says where it starts, so the next piece can begin there.
   * `first: false` says the piece is not the start of the text, so a byte-order mark is not
   * looked for. `maxRows` stops early (and `consumed` is where reading stopped).
   */
  function parseChunk(text, d, opts = {}) {
    const { final = true, first = true, maxRows = Infinity } = opts;
    const rows = [];
    const n = text.length;
    const dc = d.charCodeAt(0);
    let row = [];
    let i = first && text.charCodeAt(0) === 0xfeff ? 1 : 0;
    let consumed = i;
    let rowStart = i;
    const push = () => {
      if (!(row.length === 1 && row[0] === "")) rows.push(row);
      row = [];
      consumed = i;
      rowStart = i;
    };
    let cut = false;
    while (i < n && rows.length < maxRows) {
      let field;
      if (text.charCodeAt(i) === 34) {
        let j = i + 1;
        let out = "";
        for (;;) {
          const k = text.indexOf('"', j);
          if (k === -1) {
            if (!final) cut = true;
            out += text.slice(j);
            i = n;
            break;
          }
          out += text.slice(j, k);
          if (text.charCodeAt(k + 1) === 34) {
            out += '"';
            j = k + 2;
          } else {
            i = k + 1;
            break;
          }
        }
        if (cut) break;
        // Anything between the closing quote and the next delimiter is kept as written.
        let k = i;
        while (k < n) {
          const c = text.charCodeAt(k);
          if (c === dc || c === 10 || c === 13) break;
          k++;
        }
        field = out + text.slice(i, k);
        i = k;
      } else {
        let k = i;
        while (k < n) {
          const c = text.charCodeAt(k);
          if (c === dc || c === 10 || c === 13) break;
          k++;
        }
        field = text.slice(i, k);
        i = k;
      }
      row.push(field);
      if (i >= n) {
        // The text ends in a row with no line break after it: a whole row only if the text is.
        if (!final) {
          cut = true;
          break;
        }
        push();
        break;
      }
      const c = text.charCodeAt(i);
      if (c === dc) {
        i++;
        if (i >= n) {
          if (!final) {
            cut = true;
            break;
          }
          row.push("");
          push();
        }
        continue;
      }
      i += c === 13 && text.charCodeAt(i + 1) === 10 ? 2 : 1;
      push();
    }
    // Where the next piece starts: the row the piece ended in, or the end of what was read.
    return { rows, consumed: cut ? rowStart : final && i >= n ? n : consumed };
  }

  /** The rows of `text`; see `parseChunk`. `maxRows` stops early. */
  function parse(text, d, maxRows = Infinity) {
    return parseChunk(text, d, { maxRows }).rows;
  }

  /** The delimiter that splits the start of `text` into the most steady columns. */
  function detect(text) {
    let sample = text.slice(0, 8192);
    if (text.length > sample.length)
      sample = sample.slice(0, Math.max(sample.lastIndexOf("\n"), 0) || sample.length);
    let best = ",";
    let bestScore = 0;
    for (const d of CANDIDATES) {
      const rows = parse(sample, d, 50);
      if (!rows.length) continue;
      const counts = new Map();
      for (const r of rows) counts.set(r.length, (counts.get(r.length) || 0) + 1);
      let mode = 0;
      let freq = 0;
      for (const [len, f] of counts) {
        if (f > freq || (f === freq && len > mode)) {
          mode = len;
          freq = f;
        }
      }
      // Several columns, held by most rows. Steadiness counts twice: a delimiter that gives every
      // row the same number of columns beats one that gives more columns to some of them.
      const steady = freq / rows.length;
      const score = mode > 1 ? steady * steady * mode : 0;
      if (score > bestScore) {
        best = d;
        bestScore = score;
      }
    }
    return best;
  }

  /** A column's letters, spreadsheet style: A, B, … Z, AA. */
  function letters(i) {
    let s = "";
    for (let n = i + 1; n > 0; n = Math.floor((n - 1) / 26))
      s = String.fromCharCode(65 + ((n - 1) % 26)) + s;
    return s;
  }

  /** Whether `s` reads as a plain number (digits, one point, a sign, an exponent). */
  const isNumber = (s) => /^\s*[-+]?(\d+\.?\d*|\.\d+)([eE][-+]?\d+)?\s*$/.test(s);

  // ---- Work in slices ----

  /** How long one slice of work may run before the page is given a turn, in milliseconds. */
  const SLICE_MS = 10;
  const now = () => (typeof performance !== "undefined" ? performance.now() : Date.now());
  const breathe = () => new Promise((resolve) => setTimeout(resolve, 0));

  /**
   * The indices `0..count-1` in the order `cmp` gives, found without holding the page up: runs
   * are sorted one at a time and merged two at a time, with a turn for the page between slices.
   * `opts.cancelled()` ends it early (the answer is `null`); `opts.breathe` is the turn to give.
   */
  async function sortOrder(count, cmp, opts = {}) {
    const give = opts.breathe || breathe;
    const cancelled = opts.cancelled || (() => false);
    const run = opts.run || 4000;
    let started = now();
    const turn = async () => {
      if (now() - started < SLICE_MS) return cancelled();
      await give();
      started = now();
      return cancelled();
    };
    let runs = [];
    for (let from = 0; from < count; from += run) {
      const part = [];
      for (let i = from; i < Math.min(count, from + run); i++) part.push(i);
      part.sort(cmp);
      runs.push(part);
      if (await turn()) return null;
    }
    if (!runs.length) return [];
    while (runs.length > 1) {
      const next = [];
      for (let r = 0; r < runs.length; r += 2) {
        if (r + 1 === runs.length) {
          next.push(runs[r]);
          continue;
        }
        const a = runs[r];
        const b = runs[r + 1];
        const out = new Array(a.length + b.length);
        let i = 0;
        let j = 0;
        let k = 0;
        while (i < a.length && j < b.length) {
          out[k++] = cmp(a[i], b[j]) <= 0 ? a[i++] : b[j++];
          if ((k & 8191) === 0 && now() - started >= SLICE_MS) {
            if (await turn()) return null;
          }
        }
        while (i < a.length) out[k++] = a[i++];
        while (j < b.length) out[k++] = b[j++];
        next.push(out);
        if (await turn()) return null;
      }
      runs = next;
    }
    return runs[0];
  }

  /**
   * The members of `order` that `match` accepts, found in slices. `opts.progress(found, done,
   * total)` is called at each turn so the page can show what is found so far; `opts.cancelled()`
   * ends it early (the answer is `null`).
   */
  async function filterOrder(order, match, opts = {}) {
    const give = opts.breathe || breathe;
    const cancelled = opts.cancelled || (() => false);
    const out = [];
    let started = now();
    for (let k = 0; k < order.length; k++) {
      if (match(order[k])) out.push(order[k]);
      if ((k & 1023) === 1023 && now() - started >= SLICE_MS) {
        if (opts.progress) opts.progress(out, k + 1, order.length);
        await give();
        if (cancelled()) return null;
        started = now();
      }
    }
    return out;
  }

  root.CSV = {
    parse,
    parseChunk,
    detect,
    letters,
    isNumber,
    sortOrder,
    filterOrder,
    NAMES,
    CANDIDATES,
  };
})(typeof globalThis !== "undefined" ? globalThis : window);
