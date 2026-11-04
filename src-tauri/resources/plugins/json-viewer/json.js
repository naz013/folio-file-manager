"use strict";
// What the JSON viewer does with a document that is already parsed: the lines of the tree that are
// open (so only the ones in view need to be drawn), finding text anywhere in it without an index
// of every node, and following a path to a value. Nothing here touches the page, so it is tested
// on its own.
(function (root) {
  const kind = (v) => (v === null ? "null" : Array.isArray(v) ? "array" : typeof v);
  const isContainer = (v) => v !== null && typeof v === "object";
  /** A path as one string, to say which nodes are open. */
  const pk = (path) => JSON.stringify(path);
  /** `$.a.b[3]["odd key"]` */
  const pathText = (path) =>
    path.reduce(
      (s, k) =>
        typeof k === "number"
          ? `${s}[${k}]`
          : /^[A-Za-z_$][\w$]*$/.test(k)
            ? `${s}.${k}`
            : `${s}[${JSON.stringify(k)}]`,
      "$",
    );

  /**
   * The lines of the tree, in order. An open container shows one window of its members: the
   * `chunk` members from `windows.get(id) ?? 0`, with a line before them if members come
   * earlier, a line after them if more follow, and then its closing bracket. So an open
   * container never makes more than `chunk` lines, however many members it has. Returns the lines
   * and, for each node line, where it is (`index`: id to position), so a search can scroll to one.
   * A line is `{ id, path, key, v, depth, kind, container, count, open, last }`, or `{ earlier,
   * of, left, depth }` or `{ more, of, left, depth }` for the lines that move the window, or
   * `{ close, depth, kind, last }`.
   */
  function flatten(doc, expanded, windows, chunk) {
    const list = [];
    const index = new Map();
    const walk = (key, v, path, depth, last) => {
      const id = pk(path);
      const k = kind(v);
      const container = isContainer(v);
      const array = k === "array";
      const keys = container && !array ? Object.keys(v) : null;
      const count = container ? (array ? v.length : keys.length) : 0;
      const open = container && count > 0 && expanded.has(id);
      index.set(id, list.length);
      list.push({ id, path, key, v, depth, kind: k, container, count, open, last });
      if (!open) return;
      const from = Math.min(windows.get(id) ?? 0, Math.max(0, count - 1));
      const to = Math.min(count, from + chunk);
      if (from > 0) list.push({ earlier: true, of: id, left: from, depth: depth + 1 });
      for (let i = from; i < to; i++) {
        const ck = array ? null : keys[i];
        const cv = array ? v[i] : v[ck];
        walk(ck, cv, path.concat(array ? i : ck), depth + 1, i === count - 1);
      }
      if (count > to) list.push({ more: true, of: id, left: count - to, depth: depth + 1 });
      list.push({ close: true, depth, kind: k, last });
    };
    walk(null, doc, [], 0, true);
    return { list, index };
  }

  /** How long one slice of work may run before the page is given a turn, in milliseconds. */
  const SLICE_MS = 10;
  const now = () => (typeof performance !== "undefined" ? performance.now() : Date.now());
  const breathe = () => new Promise((resolve) => setTimeout(resolve, 0));

  /**
   * The paths of the nodes whose key or value holds `needle` (lower case), in document order,
   * found without an index of every node and in slices, so the page stays usable. Stops at `max`
   * (`capped` says so). `opts.cancelled()` ends it early (the answer is `null`); `opts.breathe` is
   * the turn to give between slices; `opts.visited(n)` hears how many nodes have been looked at.
   */
  async function search(doc, needle, max, opts = {}) {
    const give = opts.breathe || breathe;
    const cancelled = opts.cancelled || (() => false);
    const found = [];
    let started = now();
    let n = 0;
    const matches = (key, v) => {
      if (key !== null && String(key).toLowerCase().includes(needle)) return true;
      return !isContainer(v) && String(v).toLowerCase().includes(needle);
    };
    // The frames hold where each open container is; a path is only built for a match.
    const stack = [];
    const pathNow = () => stack.map((f) => f.at);
    const enter = (v) => {
      if (!isContainer(v)) return;
      const array = Array.isArray(v);
      const keys = array ? null : Object.keys(v);
      stack.push({ v, keys, i: 0, n: array ? v.length : keys.length, at: 0 });
    };
    if (matches(null, doc)) found.push([]);
    enter(doc);
    while (stack.length && found.length < max) {
      const f = stack[stack.length - 1];
      if (f.i >= f.n) {
        stack.pop();
        continue;
      }
      const at = f.keys ? f.keys[f.i] : f.i;
      f.i++;
      f.at = at;
      const child = f.v[at];
      n++;
      if (matches(f.keys ? at : null, child)) found.push(pathNow());
      enter(child);
      if ((n & 511) === 0 && now() - started >= SLICE_MS) {
        if (opts.visited) opts.visited(n);
        await give();
        if (cancelled()) return null;
        started = now();
      }
    }
    // Frames with nothing left in them are not "more".
    while (stack.length && stack[stack.length - 1].i >= stack[stack.length - 1].n) stack.pop();
    return { found, capped: found.length >= max && stack.length > 0 };
  }

  /** The value at `path` in `doc`, or `undefined` if there is none. */
  function valueAt(doc, path) {
    let v = doc;
    for (const k of path) {
      if (!isContainer(v)) return undefined;
      v = v[k];
    }
    return v;
  }

  /**
   * Containers to open for "expand all", without opening everything of a huge document: no deeper
   * than `depth`, at most `max` of them, looking at no more than `visit` nodes, and no more than
   * `rows` lines in all (an open container shows up to `chunk` members).
   */
  function openable(doc, depth, max, visit, rows = Infinity, chunk = 10000) {
    const ids = [];
    let seen = 0;
    let lines = 0;
    const walk = (v, path) => {
      if (!isContainer(v) || ids.length >= max || seen++ >= visit || lines >= rows) return;
      if (path.length > depth) return;
      const array = Array.isArray(v);
      const keys = array ? null : Object.keys(v);
      const count = array ? v.length : keys.length;
      if (count) {
        ids.push(pk(path));
        lines += Math.min(count, chunk) + 2;
      }
      const shown = Math.min(count, chunk);
      for (let i = 0; i < shown; i++) {
        if (ids.length >= max || seen >= visit || lines >= rows) break;
        const k = array ? i : keys[i];
        walk(v[k], path.concat(k));
      }
    };
    walk(doc, []);
    return ids;
  }

  /**
   * The first `n` lines of `str` (CR LF, LF or CR), each cut at `width` characters (a minified
   * document is one enormous line), and whether there is more after them and whether any line
   * was cut.
   */
  function firstLines(str, n, width = Infinity) {
    const lines = [];
    let clipped = false;
    let at = 0;
    while (lines.length < n && at <= str.length) {
      let end = at;
      while (end < str.length) {
        const c = str.charCodeAt(end);
        if (c === 10 || c === 13) break;
        end++;
      }
      if (end - at > width) {
        clipped = true;
        lines.push(str.slice(at, at + width));
      } else {
        lines.push(str.slice(at, end));
      }
      if (end >= str.length) return { lines, more: false, clipped };
      at = end + (str.charCodeAt(end) === 13 && str.charCodeAt(end + 1) === 10 ? 2 : 1);
    }
    return { lines, more: at < str.length, clipped };
  }

  root.Json = { kind, isContainer, pk, pathText, flatten, search, valueAt, openable, firstLines };
})(typeof globalThis !== "undefined" ? globalThis : window);
