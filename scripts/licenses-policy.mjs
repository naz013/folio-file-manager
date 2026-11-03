// Which licenses Folio may ship, and how an SPDX expression is judged against that.
// Folio is AGPL-3.0-only, so everything here is permissive or a weak (file-level) copyleft that
// the AGPL can include. Adding a license is a decision: change the list, with a reason.

export const ALLOWED = new Set([
  "0BSD",
  "Apache-2.0",
  "BSD-2-Clause",
  "BSD-3-Clause",
  "BSL-1.0",
  "CDLA-Permissive-2.0", // the root certificate data in webpki-root-certs
  "CC0-1.0",
  "ISC",
  "MIT",
  "MIT-0",
  "MPL-2.0", // weak copyleft: only changes to the MPL files themselves must be shared
  "OFL-1.1", // the Inter font
  "Unicode-3.0",
  "Unicode-DFS-2016",
  "Unlicense",
  "Zlib",
  "AGPL-3.0-only", // Folio itself
]);

/** Exceptions that only widen what a license allows. */
export const ALLOWED_EXCEPTIONS = new Set(["LLVM-exception"]);

const TOKEN = /\(|\)|[^\s()]+/g;

/** Splits `MIT OR (Apache-2.0 AND ISC)`. The old `MIT/Apache-2.0` of some crates means OR. */
function tokens(expr) {
  return (expr.replace(/\//g, " OR ").match(TOKEN) ?? []).map((t) => t.replace(/\+$/, "-or-later"));
}

/**
 * Judges an SPDX expression: `{ ok, ids }`, where `ids` are the licenses that decided it
 * (the chosen side of an OR) and `problems` name what is not allowed or not understood.
 */
export function judge(expr, allowed = ALLOWED) {
  const list = typeof expr === "string" ? tokens(expr) : [];
  let at = 0;
  const bad = [];
  let broken = false;

  function atom() {
    const t = list[at++];
    if (t === "(") {
      const r = or();
      if (list[at] === ")") at++;
      else {
        broken = true;
        bad.push("unbalanced parentheses");
      }
      return r;
    }
    if (t === undefined || ["AND", "OR", "WITH", ")"].includes(t)) {
      broken = true;
      bad.push("incomplete expression");
      return { ok: false, ids: [] };
    }
    // A trailing "+" was turned into "-or-later" above: `MIT+` is not an ID we list.
    let ok = allowed.has(t);
    if (list[at] === "WITH") {
      at++;
      const ex = list[at++];
      if (ok && !ALLOWED_EXCEPTIONS.has(ex ?? "")) ok = false;
      if (!ok) bad.push(`${t} WITH ${ex}`);
      return { ok, ids: [t] };
    }
    if (!ok) bad.push(t);
    return { ok, ids: [t] };
  }

  function and() {
    let r = atom();
    while (list[at] === "AND") {
      at++;
      const n = atom();
      r = { ok: r.ok && n.ok, ids: [...r.ids, ...n.ids] };
    }
    return r;
  }

  function or() {
    const options = [and()];
    while (list[at] === "OR") {
      at++;
      options.push(and());
    }
    return options.find((o) => o.ok) ?? options[0];
  }

  if (list.length === 0) return { ok: false, ids: [], problems: ["no license declared"] };
  const r = or();
  if (at < list.length) bad.push(`unexpected "${list[at]}"`);
  // Problems inside an OR whose other side is fine do not count.
  return r.ok && !broken && at >= list.length
    ? { ok: true, ids: r.ids, problems: [] }
    : { ok: false, ids: r.ids, problems: [...new Set(bad)] };
}
