"use strict";
// A small Markdown reader: text in, a tree of plain objects out. It builds nothing for the page
// to run: no HTML is passed through, and a link is only ever an address to show. The page draws
// the tree with textContent. A subset of CommonMark and GitHub's tables, task lists, strikethrough
// and bare links.
//
// Blocks:  { t: "h", level, text } { t: "p", text } { t: "code", lang, text } { t: "quote", kids }
//          { t: "list", ordered, start, tight, items: [{ task: null | boolean, kids }] }
//          { t: "hr" } { t: "table", align, head, rows }    (text is Markdown for `inline`)
// Inlines: { t: "text", v } { t: "code", v } { t: "em" | "strong" | "del", kids }
//          { t: "link", href, title, kids } { t: "img", src, alt, title } { t: "br" }
(function (root) {
  const MAX_DEPTH = 16;
  const MAX_INLINE = 20000;

  const FENCE = /^( {0,3})(`{3,}|~{3,})(.*)$/;
  const ATX = /^ {0,3}(#{1,6})(?:[ ]+(.*?))?(?:[ ]+#+)?[ ]*$/;
  const HR = /^ {0,3}([-*_])(?:[ ]*\1){2,}[ ]*$/;
  const SETEXT = /^ {0,3}(=+|-+)[ ]*$/;
  const LIST = /^( {0,3})([-+*]|\d{1,9}[.)])( +|$)(.*)$/;
  const QUOTE = /^ {0,3}> ?(.*)$/;
  const TABLE_SEP = /^ *\|? *:?-+:?(?: *\| *:?-+:?)* *\|? *$/;
  const DEF =
    /^ {0,3}\[([^\]]+)\]:[ ]*<?([^\s>]+)>?(?:[ ]+(?:"([^"]*)"|'([^']*)'|\(([^)]*)\)))?[ ]*$/;

  const blank = (s) => /^\s*$/.test(s);
  const indentOf = (s) => /^ */.exec(s)[0].length;
  const norm = (s) => s.trim().replace(/\s+/g, " ").toLowerCase();

  function splitLines(md) {
    return md
      .replace(/\r\n?/g, "\n")
      .split("\n")
      .map((l) => l.replace(/^\t+/, (m) => "    ".repeat(m.length)));
  }

  function fenceOpen(line) {
    const m = FENCE.exec(line);
    if (!m) return null;
    if (m[2][0] === "`" && m[3].includes("`")) return null;
    return {
      indent: m[1].length,
      ch: m[2][0],
      len: m[2].length,
      info: m[3].trim().split(/\s+/)[0],
    };
  }

  /** Whether `line` starts something that ends a paragraph. */
  function startsBlock(line) {
    if (fenceOpen(line) || ATX.test(line) || HR.test(line) || QUOTE.test(line)) return true;
    const m = LIST.exec(line);
    return !!m && m[4] !== "" && (!/\d/.test(m[2][0]) || /^1[.)]$/.test(m[2]));
  }

  function splitRow(row) {
    let s = row.trim();
    if (s.startsWith("|")) s = s.slice(1);
    if (s.endsWith("|") && !s.endsWith("\\|")) s = s.slice(0, -1);
    return s.split(/(?<!\\)\|/).map((c) => c.trim().replace(/\\\|/g, "|"));
  }

  function blocks(ls, depth, defs) {
    const out = [];
    let i = 0;
    while (i < ls.length) {
      const line = ls[i];
      if (blank(line)) {
        i++;
        continue;
      }

      const fence = fenceOpen(line);
      if (fence) {
        const body = [];
        i++;
        while (i < ls.length) {
          const t = ls[i].trim();
          if (t.length >= fence.len && t === fence.ch.repeat(t.length)) break;
          body.push(ls[i].replace(new RegExp(`^ {0,${fence.indent}}`), ""));
          i++;
        }
        i++;
        out.push({ t: "code", lang: fence.info || "", text: body.join("\n") });
        continue;
      }

      let m = ATX.exec(line);
      if (m) {
        out.push({ t: "h", level: m[1].length, text: (m[2] || "").trim() });
        i++;
        continue;
      }
      if (HR.test(line)) {
        out.push({ t: "hr" });
        i++;
        continue;
      }

      if (QUOTE.test(line)) {
        const inner = [];
        while (i < ls.length) {
          const q = QUOTE.exec(ls[i]);
          if (q) inner.push(q[1]);
          else if (
            inner.length &&
            !blank(ls[i]) &&
            !blank(inner[inner.length - 1]) &&
            !startsBlock(ls[i])
          ) {
            inner.push(ls[i]);
          } else break;
          i++;
        }
        out.push({
          t: "quote",
          kids:
            depth < MAX_DEPTH
              ? blocks(inner, depth + 1, defs)
              : [{ t: "p", text: inner.join("\n") }],
        });
        continue;
      }

      m = LIST.exec(line);
      if (m) {
        const ordered = /\d/.test(m[2][0]);
        const kind = ordered ? m[2].slice(-1) : m[2];
        const list = {
          t: "list",
          ordered,
          start: ordered ? parseInt(m[2], 10) : 1,
          tight: true,
          items: [],
        };
        while (i < ls.length) {
          const lm = LIST.exec(ls[i]);
          if (!lm) break;
          const sameKind =
            /\d/.test(lm[2][0]) === ordered && (ordered ? lm[2].slice(-1) : lm[2]) === kind;
          if (!sameKind) break;
          const gap = lm[3].length;
          const content = lm[1].length + lm[2].length + (gap >= 1 && gap <= 4 ? gap : 1);
          const itemLines = [lm[4]];
          i++;
          let looseInside = false;
          while (i < ls.length) {
            const l = ls[i];
            if (blank(l)) {
              let k = i;
              while (k < ls.length && blank(ls[k])) k++;
              if (k < ls.length && indentOf(ls[k]) >= content) {
                for (; i < k; i++) itemLines.push("");
                looseInside = true;
                continue;
              }
              break;
            }
            if (indentOf(l) >= content) {
              itemLines.push(l.slice(content));
              i++;
            } else if (
              !blank(itemLines[itemLines.length - 1]) &&
              !LIST.test(l) &&
              !startsBlock(l)
            ) {
              itemLines.push(l.trim());
              i++;
            } else break;
          }
          let task = null;
          const tm = /^\[([ xX])\][ ]+/.exec(itemLines[0]);
          if (tm) {
            task = tm[1] !== " ";
            itemLines[0] = itemLines[0].slice(tm[0].length);
          }
          if (looseInside) list.tight = false;
          list.items.push({
            task,
            kids:
              depth < MAX_DEPTH
                ? blocks(itemLines, depth + 1, defs)
                : [{ t: "p", text: itemLines.join("\n") }],
          });
          // Blank lines, then another item of this list: the list is loose.
          let k = i;
          while (k < ls.length && blank(ls[k])) k++;
          if (k > i && k < ls.length) {
            const next = LIST.exec(ls[k]);
            const same =
              next &&
              /\d/.test(next[2][0]) === ordered &&
              (ordered ? next[2].slice(-1) : next[2]) === kind;
            if (same) {
              list.tight = false;
              i = k;
            }
          }
        }
        out.push(list);
        continue;
      }

      if (
        line.includes("|") &&
        i + 1 < ls.length &&
        ls[i + 1].includes("-") &&
        TABLE_SEP.test(ls[i + 1])
      ) {
        const head = splitRow(line);
        const sep = splitRow(ls[i + 1]);
        if (head.length === sep.length) {
          const align = sep.map((c) =>
            c.startsWith(":") && c.endsWith(":")
              ? "center"
              : c.endsWith(":")
                ? "right"
                : c.startsWith(":")
                  ? "left"
                  : "",
          );
          const rows = [];
          i += 2;
          while (i < ls.length && !blank(ls[i]) && !startsBlock(ls[i])) {
            const cells = splitRow(ls[i]);
            rows.push(head.map((_, c) => cells[c] || ""));
            i++;
          }
          out.push({ t: "table", align, head, rows });
          continue;
        }
      }

      if (indentOf(line) >= 4) {
        const body = [];
        while (i < ls.length && (indentOf(ls[i]) >= 4 || blank(ls[i]))) {
          body.push(ls[i].slice(4));
          i++;
        }
        while (body.length && blank(body[body.length - 1])) body.pop();
        out.push({ t: "code", lang: "", text: body.join("\n") });
        continue;
      }

      // A paragraph, or a heading underlined with = or -.
      let para = [line.replace(/^ +/, "")];
      i++;
      let heading = 0;
      while (i < ls.length && !blank(ls[i])) {
        const s = SETEXT.exec(ls[i]);
        if (s) {
          heading = s[1][0] === "=" ? 1 : 2;
          i++;
          break;
        }
        if (startsBlock(ls[i])) break;
        para.push(ls[i].replace(/^ +/, ""));
        i++;
      }
      if (!heading) {
        let d;
        while (para.length && (d = DEF.exec(para[0]))) {
          const key = norm(d[1]);
          if (!defs.has(key)) defs.set(key, { href: d[2], title: d[3] ?? d[4] ?? d[5] ?? "" });
          para.shift();
        }
      }
      if (!para.length) continue;
      const text = para.join("\n").replace(/ +$/, "");
      out.push(heading ? { t: "h", level: heading, text } : { t: "p", text });
    }
    return out;
  }

  // ---- Inline ----

  const PUNCT = /[!-/:-@[-`{-~]/;
  const isSpace = (c) => c === undefined || /\s/.test(c);
  const isWord = (c) => c !== undefined && /[\p{L}\p{N}]/u.test(c);

  /** Index of the `]` closing the `[` at `open`, or -1. Skips code spans and escapes. */
  function closeBracket(s, open) {
    let depth = 0;
    for (let i = open; i < s.length; i++) {
      const c = s[i];
      if (c === "\\") i++;
      else if (c === "`") {
        const run = /^`+/.exec(s.slice(i))[0].length;
        const end = s.indexOf("`".repeat(run), i + run);
        if (end !== -1) i = end + run - 1;
        else i += run - 1;
      } else if (c === "[") depth++;
      else if (c === "]" && --depth === 0) return i;
    }
    return -1;
  }

  /** The destination and title of `(dest "title")` starting at `at` (the "("), or null. */
  function destination(s, at) {
    let i = at + 1;
    while (s[i] === " " || s[i] === "\n") i++;
    let href = "";
    if (s[i] === "<") {
      const end = s.indexOf(">", i);
      if (end === -1 || s.slice(i, end).includes("\n")) return null;
      href = s.slice(i + 1, end);
      i = end + 1;
    } else {
      let depth = 0;
      const from = i;
      while (i < s.length && !/\s/.test(s[i])) {
        if (s[i] === "\\") i++;
        else if (s[i] === "(") depth++;
        else if (s[i] === ")") {
          if (depth === 0) break;
          depth--;
        }
        i++;
      }
      href = s.slice(from, i).replace(/\\(.)/g, "$1");
    }
    while (s[i] === " " || s[i] === "\n") i++;
    let title = "";
    const q = s[i];
    if (q === '"' || q === "'" || q === "(") {
      const close = q === "(" ? ")" : q;
      const end = s.indexOf(close, i + 1);
      if (end === -1) return null;
      title = s.slice(i + 1, end);
      i = end + 1;
      while (s[i] === " " || s[i] === "\n") i++;
    }
    return s[i] === ")" ? { href, title, end: i + 1 } : null;
  }

  /** A closing run of `ch` for an opener of `n`, searching from `from`; the run's index, or -1. */
  function closer(s, from, ch, n) {
    for (let i = from; i < s.length; i++) {
      const c = s[i];
      if (c === "\\") i++;
      else if (c === "`") {
        const run = /^`+/.exec(s.slice(i))[0].length;
        const end = s.indexOf("`".repeat(run), i + run);
        i = end !== -1 ? end + run - 1 : i + run - 1;
      } else if (c === ch) {
        let run = 1;
        while (s[i + run] === ch) run++;
        const before = s[i - 1];
        const after = s[i + run];
        const ok =
          !isSpace(before) && (ch !== "_" || !isWord(after)) && (n === 1 ? run === 1 : run >= n);
        if (ok) return i;
        i += run - 1;
      }
    }
    return -1;
  }

  function inline(s, defs, depth, inLink) {
    if (depth > 12 || s.length > MAX_INLINE) return [{ t: "text", v: s }];
    const out = [];
    let buf = "";
    const flush = () => {
      if (buf) out.push({ t: "text", v: buf });
      buf = "";
    };
    let i = 0;
    while (i < s.length) {
      const c = s[i];

      if (c === "\\" && i + 1 < s.length) {
        if (s[i + 1] === "\n") {
          flush();
          out.push({ t: "br" });
          i += 2;
          continue;
        }
        if (PUNCT.test(s[i + 1])) {
          buf += s[i + 1];
          i += 2;
          continue;
        }
      }
      if (c === "\n") {
        if (/ {2,}$/.test(buf)) {
          buf = buf.replace(/ +$/, "");
          flush();
          out.push({ t: "br" });
        } else buf = buf.replace(/ +$/, "") + " ";
        i++;
        continue;
      }
      if (c === "`") {
        const run = /^`+/.exec(s.slice(i))[0].length;
        const end = s.indexOf("`".repeat(run), i + run);
        if (end !== -1) {
          let v = s.slice(i + run, end).replace(/\n/g, " ");
          if (v.length > 2 && v.startsWith(" ") && v.endsWith(" ")) v = v.slice(1, -1);
          flush();
          out.push({ t: "code", v });
          i = end + run;
          continue;
        }
        buf += "`".repeat(run);
        i += run;
        continue;
      }

      if (c === "[" || (c === "!" && s[i + 1] === "[")) {
        const image = c === "!";
        const open = image ? i + 1 : i;
        const end = closeBracket(s, open);
        if (end !== -1 && !(inLink && !image)) {
          const label = s.slice(open + 1, end);
          let target = null;
          let next = end + 1;
          if (s[end + 1] === "(") {
            const d = destination(s, end + 1);
            if (d) {
              target = { href: d.href, title: d.title };
              next = d.end;
            }
          } else {
            let key = norm(label);
            let stop = end + 1;
            if (s[end + 1] === "[") {
              const close = s.indexOf("]", end + 2);
              if (close !== -1) {
                key = norm(s.slice(end + 2, close)) || key;
                stop = close + 1;
              }
            }
            if (defs.has(key)) {
              target = defs.get(key);
              next = stop;
            }
          }
          if (target) {
            flush();
            out.push(
              image
                ? { t: "img", src: target.href, alt: label, title: target.title }
                : {
                    t: "link",
                    href: target.href,
                    title: target.title,
                    kids: inline(label, defs, depth + 1, true),
                  },
            );
            i = next;
            continue;
          }
        }
        buf += c;
        i++;
        continue;
      }

      if (c === "<") {
        const am = /^<([a-zA-Z][a-zA-Z0-9+.-]{1,31}:[^\s<>]*|[^\s<>@]+@[^\s<>@]+\.[^\s<>@]+)>/.exec(
          s.slice(i, i + 2100),
        );
        if (am && !inLink) {
          flush();
          const href = am[1].includes(":") ? am[1] : `mailto:${am[1]}`;
          out.push({ t: "link", href, title: "", kids: [{ t: "text", v: am[1] }] });
          i += am[0].length;
          continue;
        }
        // An HTML comment is not shown; other tags are text.
        if (s.startsWith("<!--", i)) {
          const end = s.indexOf("-->", i + 4);
          if (end !== -1) {
            i = end + 3;
            continue;
          }
        }
      }

      if (
        c === "h" &&
        !inLink &&
        !isWord(s[i - 1]) &&
        (s.startsWith("http://", i) || s.startsWith("https://", i))
      ) {
        const bm = /^https?:\/\/[^\s<]+/.exec(s.slice(i, i + 2100));
        if (bm) {
          let url = bm[0].replace(/[.,;:!?'"]+$/, "");
          while (
            url.endsWith(")") &&
            (url.match(/\(/g) || []).length < (url.match(/\)/g) || []).length
          ) {
            url = url.slice(0, -1);
          }
          flush();
          out.push({ t: "link", href: url, title: "", kids: [{ t: "text", v: url }] });
          i += url.length;
          continue;
        }
      }

      if (c === "*" || c === "_" || c === "~") {
        let run = 1;
        while (s[i + run] === c) run++;
        const before = s[i - 1];
        const after = s[i + run];
        const canOpen = !isSpace(after) && (c !== "_" || !isWord(before));
        let used = false;
        if (canOpen && (c !== "~" || run >= 2)) {
          for (let n = c === "~" ? 2 : Math.min(run, 3); n >= (c === "~" ? 2 : 1) && !used; n--) {
            const j = closer(s, i + run, c, n);
            // The closer must close what was opened: the run's last n characters open it.
            if (j === -1) continue;
            const inner = s.slice(i + run, j);
            if (!inner) continue;
            const lead = c.repeat(run - n);
            buf += lead;
            flush();
            const kids = inline(inner, defs, depth + 1, inLink);
            if (c === "~") out.push({ t: "del", kids });
            else if (n === 3) out.push({ t: "strong", kids: [{ t: "em", kids }] });
            else out.push({ t: n === 2 ? "strong" : "em", kids });
            let after = j;
            while (s[after] === c && after < j + n) after++;
            i = after;
            used = true;
          }
        }
        if (!used) {
          buf += c.repeat(run);
          i += run;
        }
        continue;
      }

      buf += c;
      i++;
    }
    flush();
    return out;
  }

  /** The text of inline nodes with no formatting: for ids and tooltips. */
  function plain(nodes) {
    return nodes
      .map((n) =>
        n.t === "text" || n.t === "code"
          ? n.v
          : n.t === "img"
            ? n.alt
            : n.kids
              ? plain(n.kids)
              : n.t === "br"
                ? " "
                : "",
      )
      .join("");
  }

  function parse(md) {
    const defs = new Map();
    const ls = splitLines(md);
    const head = [];
    // Front matter (--- ... ---) at the very top is metadata, not a rule and a heading.
    if (ls[0] === "---") {
      const end = ls.findIndex((l, i) => i > 0 && i <= 200 && (l === "---" || l === "..."));
      if (end > 0) {
        head.push({ t: "code", lang: "front matter", text: ls.slice(1, end).join("\n") });
        ls.splice(0, end + 1);
      }
    }
    return { blocks: [...head, ...blocks(ls, 0, defs)], defs };
  }

  root.Markdown = { parse, inline, plain };
})(typeof globalThis !== "undefined" ? globalThis : window);
