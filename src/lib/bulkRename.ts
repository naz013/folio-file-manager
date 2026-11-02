/**
 * The new names a bulk rename would give, worked out for the live preview. Whether they are
 * valid or taken is decided by the Rust core (`check_renames`), which also does the renaming.
 */

export type RenameMode = "replace" | "number" | "date" | "case" | "affix";

export interface RenameRule {
  mode: RenameMode;
  find: string;
  replace: string;
  caseSensitive: boolean;
  /** Treat `find` as a regular expression. */
  regex: boolean;
  /** Pattern for "number": `{name}` is the old name, `{n}` the running number. */
  pattern: string;
  start: number;
  /** Zero-pad the number to this many digits. */
  digits: number;
  /** Pattern for "date": `{name}` is the old name, `{date}` the modified date. */
  datePattern: string;
  /** Layout of `{date}`: YYYY, MM, DD, HH, mm and ss stand for the parts of the local time. */
  dateFormat: string;
  textCase: "lower" | "upper" | "title" | "sentence";
  prefix: string;
  suffix: string;
}

export const DEFAULT_RULE: RenameRule = {
  mode: "replace",
  find: "",
  replace: "",
  caseSensitive: false,
  regex: false,
  pattern: "{name} {n}",
  start: 1,
  digits: 1,
  datePattern: "{name} {date}",
  dateFormat: "YYYY-MM-DD",
  textCase: "lower",
  prefix: "",
  suffix: "",
};

/** Splits a file name into the part that is renamed and the extension that stays. */
export function splitName(name: string, isDir: boolean): [string, string] {
  const dot = isDir ? -1 : name.lastIndexOf(".");
  return dot > 0 ? [name.slice(0, dot), name.slice(dot)] : [name, ""];
}

const escapeRegex = (s: string) => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

/** The error in the rule itself (a broken regular expression), if any. */
export function ruleError(rule: RenameRule): string | null {
  if (rule.mode !== "replace" || !rule.regex || !rule.find) return null;
  try {
    new RegExp(rule.find);
    return null;
  } catch (e) {
    return String(e instanceof Error ? e.message : e);
  }
}

/** `pattern` with each placeholder replaced in one pass, so a value is never searched again. */
function fill(pattern: string, values: Record<string, string>): string {
  return pattern.replace(/\{(?:name|n|date)\}/g, (token) => values[token] ?? token);
}

/** `modified` (ms since the epoch) laid out as `format`, in local time. */
export function formatStamp(modified: number, format: string): string {
  const d = new Date(modified);
  const two = (n: number) => String(n).padStart(2, "0");
  const parts: Record<string, string> = {
    YYYY: String(d.getFullYear()).padStart(4, "0"),
    MM: two(d.getMonth() + 1),
    DD: two(d.getDate()),
    HH: two(d.getHours()),
    mm: two(d.getMinutes()),
    ss: two(d.getSeconds()),
  };
  return format.replace(/YYYY|MM|DD|HH|mm|ss/g, (token) => parts[token]);
}

function titleCase(s: string): string {
  return s
    .toLowerCase()
    .replace(/(^|[\s_-])(\p{L})/gu, (_, sep: string, c: string) => sep + c.toUpperCase());
}

function changeCase(s: string, to: RenameRule["textCase"]): string {
  switch (to) {
    case "lower":
      return s.toLowerCase();
    case "upper":
      return s.toUpperCase();
    case "title":
      return titleCase(s);
    case "sentence":
      // By code point, so a character outside the basic plane isn't split.
      return Array.from(s).reduce(
        (out, c, k) => out + (k === 0 ? c.toUpperCase() : c.toLowerCase()),
        "",
      );
  }
}

/** The new name of the item at `index` (0-based) in the batch. The extension is kept. `modified` is the item's modified time in ms. */
export function newName(
  name: string,
  isDir: boolean,
  index: number,
  rule: RenameRule,
  modified: number | null = null,
): string {
  const [stem, ext] = splitName(name, isDir);
  let out = stem;
  switch (rule.mode) {
    case "replace": {
      if (!rule.find || ruleError(rule)) break;
      const source = rule.regex ? rule.find : escapeRegex(rule.find);
      const re = new RegExp(source, rule.caseSensitive ? "g" : "gi");
      // A plain replacement is literal, so `$&` typed by the user stays as typed.
      out = rule.regex ? stem.replace(re, rule.replace) : stem.replace(re, () => rule.replace);
      break;
    }
    case "number": {
      const start = Math.max(0, Math.trunc(rule.start) || 0);
      const n = String(start + index).padStart(Math.max(1, Math.trunc(rule.digits) || 1), "0");
      out = fill(rule.pattern, { "{name}": stem, "{n}": n });
      break;
    }
    case "date":
      // An item with no known date keeps its name rather than getting a made-up one.
      if (modified === null) break;
      out = fill(rule.datePattern, {
        "{name}": stem,
        "{date}": formatStamp(modified, rule.dateFormat),
      });
      break;
    case "case":
      out = changeCase(stem, rule.textCase);
      break;
    case "affix":
      out = rule.prefix + stem + rule.suffix;
      break;
  }
  return out + ext;
}
