"use strict";
// Reading CSV and TSV text into rows of strings: quotes ("a,b", "say ""hi"""), line breaks inside
// quotes, CR LF or LF, a byte-order mark, and rows of different lengths. Nothing here touches the
// page, so it is tested on its own.
(function (root) {
  const CANDIDATES = [",", "\t", ";", "|"];
  const NAMES = { ",": "Comma", "\t": "Tab", ";": "Semicolon", "|": "Pipe" };

  /**
   * The rows of `text` split at the delimiter `d` (one character). Blank lines are skipped. A
   * quote only opens a quoted field at the start of one; a quote inside an unquoted field is
   * text. A quote that is never closed takes the rest of the text. `maxRows` stops early.
   */
  function parse(text, d, maxRows = Infinity) {
    const rows = [];
    const n = text.length;
    const dc = d.charCodeAt(0);
    let row = [];
    let i = text.charCodeAt(0) === 0xfeff ? 1 : 0;
    const push = () => {
      if (!(row.length === 1 && row[0] === "")) rows.push(row);
      row = [];
    };
    while (i < n && rows.length < maxRows) {
      let field;
      if (text.charCodeAt(i) === 34) {
        let j = i + 1;
        let out = "";
        for (;;) {
          const k = text.indexOf('"', j);
          if (k === -1) {
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
        push();
        break;
      }
      const c = text.charCodeAt(i);
      if (c === dc) {
        i++;
        if (i >= n) {
          row.push("");
          push();
        }
        continue;
      }
      i += c === 13 && text.charCodeAt(i + 1) === 10 ? 2 : 1;
      push();
    }
    return rows;
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

  root.CSV = { parse, detect, letters, isNumber, NAMES, CANDIDATES };
})(typeof globalThis !== "undefined" ? globalThis : window);
