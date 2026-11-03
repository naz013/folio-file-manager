/** Small pure parts of the second pane, kept apart so they can be tested. */

/** What a drag over the second pane's folders asks to spring open: "side:<path>". */
const SIDE = "side:";

export const sideTarget = (path: string) => SIDE + path;

/** The folder in a `sideTarget`, or null for any other spring target. */
export const sideFolder = (target: string): string | null =>
  target.startsWith(SIDE) ? target.slice(SIDE.length) : null;

/** Where the cursor lands after moving `delta` items in a list of `length`: kept inside the list. */
export function stepCursor(cursor: number, delta: number, length: number): number {
  if (length <= 0) return -1;
  if (cursor < 0) return delta < 0 ? length - 1 : 0;
  return Math.min(length - 1, Math.max(0, cursor + delta));
}

/**
 * The folders of a path to show in a narrow header: the last `max`, with `more` set when
 * earlier ones were left out.
 */
export function lastSegments<T>(segments: T[], max: number): { shown: T[]; more: boolean } {
  return segments.length > max
    ? { shown: segments.slice(segments.length - max), more: true }
    : { shown: segments, more: false };
}
