/** Dropping items on the Recycle Bin: which of them it takes. The Rust core makes the same
 *  call again before anything moves (`ops::bin_guard`); this keeps the cue honest. */

/** Whether a path is already in an operating system's bin: Windows `$Recycle.Bin`, or the
 *  freedesktop trash and macOS `.Trash` where the systems put them. Sending those to the bin
 *  again would only shuffle them. A folder of one's own called `.Trash` is only a folder. */
export function inBin(path: string): boolean {
  const p = path.replace(/\\/g, "/").toLowerCase();
  if (/(^|\/)\$recycle\.bin(\/|$)/.test(p)) return true;
  // Windows drives and network paths have no other kind of bin.
  if (/^[a-z]:|^\/\//.test(p)) return false;
  const parts = p.split("/").filter(Boolean);
  const trash = parts.indexOf(".trash");
  return (
    parts.includes(".trashes") ||
    parts.some((s) => /^\.trash-\d+$/.test(s)) ||
    /(^|\/)\.local\/share\/trash(\/|$)/.test(p) ||
    // ~/.Trash, directly in a home folder; a volume's own at its top.
    (trash === 0 && parts.length > 1) ||
    (trash === 2 && (parts[0] === "home" || parts[0] === "users")) ||
    (trash === 2 && parts[0] === "volumes")
  );
}

/** The dropped items that go to the bin: everything except what is in it already. */
export const trashable = (paths: string[]): string[] => paths.filter((p) => !inBin(p));
