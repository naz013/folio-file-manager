import type { Entry } from "$lib/api/fs";

export type SortKey = "name" | "modified" | "size";
export interface SortState {
  key: SortKey;
  desc: boolean;
}

export const SORT_HEADERS: [SortKey, string][] = [
  ["name", "Name"],
  ["modified", "Date modified"],
  ["size", "Size"],
];

/** How a folder is named in the size cache: lower case, one kind of slash, none at the end. */
export const sizeKey = (path: string) =>
  path.toLowerCase().replace(/\//g, "\\").replace(/\\+$/, "");

/**
 * `entries` in the order the list shows them. Folders stay in front, as in the file listing.
 * Folders have no size of their own, so by size they go by the scan cache; one that hasn't
 * been scanned counts as smaller than any other.
 */
export function sortEntries(
  entries: Entry[],
  { key, desc }: SortState,
  folderSize: (path: string) => number | undefined,
): Entry[] {
  // Name order, ascending, is how the folder is listed already; descending is its reverse.
  if (key === "name") {
    const folders = entries.filter((e) => e.isDir);
    const files = entries.filter((e) => !e.isDir);
    return desc ? [...folders.reverse(), ...files.reverse()] : entries;
  }
  const value = (e: Entry): number | string => {
    if (key === "modified") return e.modified ?? 0;
    return e.isDir ? (folderSize(e.path) ?? -1) : e.size;
  };
  const sign = desc ? -1 : 1;
  return entries
    .map((e) => ({ e, v: value(e) }))
    .sort((a, b) => {
      if (a.e.isDir !== b.e.isDir) return a.e.isDir ? -1 : 1;
      const order = a.v < b.v ? -1 : a.v > b.v ? 1 : 0;
      return order * sign || a.e.name.toLowerCase().localeCompare(b.e.name.toLowerCase());
    })
    .map(({ e }) => e);
}
