/** Pure helpers for the pinned folders in the Favorites: their order and where one goes. */

/** Windows paths differ by case and trailing separators without being different folders. */
export const pinKey = (p: string) => p.replace(/[\\/]+$/, "").toLowerCase();

export const isPinned = (list: readonly string[], path: string) =>
  list.some((p) => pinKey(p) === pinKey(path));

/**
 * `list` with `paths` added at slot `at` (0 puts them first, the length or none puts them last).
 * Folders already pinned, and repeats, stay where they are.
 */
export function withPinned(
  list: readonly string[],
  paths: readonly string[],
  at?: number,
): string[] {
  const seen = new Set(list.map(pinKey));
  const fresh = paths.filter((p) => {
    if (seen.has(pinKey(p))) return false;
    seen.add(pinKey(p));
    return true;
  });
  const slot = Math.min(Math.max(at ?? list.length, 0), list.length);
  return [...list.slice(0, slot), ...fresh, ...list.slice(slot)];
}

/**
 * `list` with `path` taken out and put at slot `slot`, counted in the list as it was: the slot
 * after the last item is `list.length`. A slot next to the item itself changes nothing.
 */
export function movedTo(list: readonly string[], path: string, slot: number): string[] {
  const from = list.indexOf(path);
  if (from < 0) return [...list];
  const rest = list.filter((_, i) => i !== from);
  const to = Math.min(Math.max(slot > from ? slot - 1 : slot, 0), rest.length);
  return [...rest.slice(0, to), path, ...rest.slice(to)];
}

/** `list` with `path` one place up (`-1`) or down (`1`). */
export function movedBy(list: readonly string[], path: string, by: -1 | 1): string[] {
  const from = list.indexOf(path);
  const to = from + by;
  if (from < 0 || to < 0 || to >= list.length) return [...list];
  const out = [...list];
  [out[from], out[to]] = [out[to], out[from]];
  return out;
}

/**
 * Where a folder dropped at height `y` over a favorite would be inserted.
 * `index` is the pinned folder's place in the list; `fixed` is a built-in favorite (Home,
 * Desktop...), which cannot be moved. The top and bottom quarters of an item are the gaps
 * between items; the middle is the folder itself, where a drop moves or copies into it.
 * Returns the slot, or null for the middle.
 */
export function slotOver(
  item: { index: number; last?: boolean } | { fixed: true; last?: boolean },
  y: number,
  top: number,
  height: number,
): number | null {
  const frac = (y - top) / height;
  if ("fixed" in item) return item.last && frac > 0.75 ? 0 : null;
  if (frac < 0.25) return item.index;
  if (frac > 0.75) return item.index + 1;
  return null;
}

/** The last part of a path, for naming a favorite in words. */
export const pinName = (p: string) => p.split(/[/\\]/).filter(Boolean).pop() ?? p;
