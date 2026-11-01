/** Fuzzy matching and the list of recent folders behind the command palette (Ctrl+K). */

const RECENT_KEY = "folio.recent";
const RECENT_LIMIT = 20;

/**
 * How well `query` matches `text`: its letters must appear in order, and runs, word starts
 * and early matches score higher. Null when it doesn't match at all.
 */
export function fuzzyScore(query: string, text: string): number | null {
  const q = query.toLowerCase().replace(/\s+/g, "");
  if (!q) return 0;
  const t = text.toLowerCase();
  let score = 0;
  let from = 0;
  let previous = -2;
  for (const ch of q) {
    const at = t.indexOf(ch, from);
    if (at < 0) return null;
    // A letter right after the last one, or at the start of a word, counts for more.
    const wordStart = at === 0 || /[\s\\/._-]/.test(t[at - 1]);
    score += 1 + (at === previous + 1 ? 3 : 0) + (wordStart ? 2 : 0) - Math.min(at - from, 5) * 0.1;
    previous = at;
    from = at + 1;
  }
  // Shorter texts that match are the better match.
  return score - t.length * 0.01;
}

export function loadRecent(): string[] {
  try {
    const saved: unknown = JSON.parse(localStorage.getItem(RECENT_KEY) ?? "[]");
    if (Array.isArray(saved)) return saved.filter((p): p is string => typeof p === "string");
  } catch {
    // Unreadable: start with none.
  }
  return [];
}

/** Two spellings of one Windows path differ only in case. */
const same = (a: string, b: string) =>
  a === b || (a.includes("\\") && a.toLowerCase() === b.toLowerCase());

export function clearRecent() {
  try {
    localStorage.removeItem(RECENT_KEY);
  } catch {
    // Nothing to clear.
  }
}

/** Puts `path` first among the recent folders, once. */
export function recordRecent(path: string) {
  const next = [path, ...loadRecent().filter((p) => !same(p, path))].slice(0, RECENT_LIMIT);
  try {
    localStorage.setItem(RECENT_KEY, JSON.stringify(next));
  } catch {
    // Recent folders just aren't kept.
  }
}
