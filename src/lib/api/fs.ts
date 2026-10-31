import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";

/** A directory entry (decoded from `fs::Entry` in src-tauri/src/fs/mod.rs). */
export interface Entry {
  name: string;
  path: string;
  isDir: boolean;
  size: number;
  /** Milliseconds since the Unix epoch. */
  modified: number | null;
  hidden: boolean;
}

/** Wire format from Rust (`fs::Entry`): `[name, flags, size, modified]`. */
type WireEntry = [name: string, flags: number, size: number, modified: number | null];
const FLAG_DIR = 1;
const FLAG_HIDDEN = 2;

export async function listDir(path: string): Promise<Entry[]> {
  const wire = await invoke<WireEntry[]>("list_dir", { path });
  const base = joinPath(path, "");
  return wire.map(([name, flags, size, modified]) => ({
    name,
    path: base + name,
    isDir: (flags & FLAG_DIR) !== 0,
    size,
    modified,
    hidden: (flags & FLAG_HIDDEN) !== 0,
  }));
}
export const homeDir = () => invoke<string>("home_dir");
export const watchDir = (path: string) => invoke<void>("watch_dir", { path });
export const openPath = (path: string) => invoke<void>("open_path", { path });

/** Starts a native OS drag so files can be dropped into Explorer and other apps. */
export const startDrag = (paths: string[]) => invoke<void>("start_drag", { paths });

/** The modifier keys held right now; a native drop doesn't report them. */
export const modifierKeys = () => invoke<{ ctrl: boolean; shift: boolean }>("modifier_keys");

/** Shows the native OS context menu for `paths` at the cursor (Windows only for now). */
export const showContextMenu = (paths: string[], extended: boolean) =>
  invoke<void>("show_context_menu", { paths, extended });

/** URL of the OS thumbnail (or icon) for `path`, served by the Rust `thumb://` protocol. */
export const thumbUrl = (path: string, size: number) =>
  `${convertFileSrc(path, "thumb")}?s=${size}`;

/** Explorer's rule: dragging within a volume moves, across volumes copies. */
export function sameVolume(a: string, b: string): boolean {
  const root = (p: string) =>
    /^[a-zA-Z]:/.test(p)
      ? p.slice(0, 2).toLowerCase()
      : p.startsWith("\\\\")
        ? p.split("\\").slice(0, 4).join("\\").toLowerCase()
        : "/";
  return root(a) === root(b);
}

/** Undoes the latest file operation; resolves to its name, rejects with why it can't. */
export const undoOperation = () => invoke<string>("undo");
/** Redoes the latest undone file operation. */
export const redoOperation = () => invoke<string>("redo");
/** Starts moving to the Recycle Bin in the background; resolves to the job id. */
export const trashPaths = (targets: string[]) => invoke<number>("trash_paths", { targets });
export const renamePath = (path: string, newName: string) =>
  invoke<string>("rename_path", { path, newName });
export const createFolder = (parent: string, name: string) =>
  invoke<string>("create_folder", { parent, name });
/** Result of `copy_paths` (`commands::Copied`). */
export interface Copied {
  /** The folder copied into. */
  dest: string;
  /** The copy of each source. */
  paths: string[];
  /** Links that weren't copied because they can't be recreated on this system. */
  skipped: string[];
  /** Links in the copy that should point into it but still lead to the original. */
  notRepointed: string[];
}
/** A name in the destination that an item being copied or moved would collide with. */
export interface Conflict {
  source: string;
  existing: string;
  name: string;
  sourceIsDir: boolean;
  existingIsDir: boolean;
  /** Bytes; 0 for a folder. */
  sourceSize: number;
  existingSize: number;
  /** Milliseconds since the Unix epoch. */
  sourceModified: number | null;
  existingModified: number | null;
  /** False when replacing would also take the item itself or the destination. */
  canReplace: boolean;
  /** Identity of the existing item as shown; given back so a changed item isn't replaced. */
  existingId: string | null;
}
/** What to do about one conflict: replace (existing goes to the Recycle Bin), skip, or keep both. */
export type Choice = "replace" | "skip" | "both";
export const findConflicts = (sources: string[], dest: string) =>
  invoke<Conflict[]>("find_conflicts", { sources, dest });

/**
 * Starts copying in the background; resolves to the id of the job in the queue. The
 * existing items for the sources in `replace` go to the Recycle Bin first.
 */
export const copyPaths = (
  sources: string[],
  dest: string,
  replace: string[] = [],
  replaceIds: (string | null)[] = [],
) => invoke<number>("copy_paths", { sources, dest, replace, replaceIds });
/** Result of `move_paths` (`commands::Moved`). */
export interface Moved {
  /** The folder moved into. */
  dest: string;
  /** Where each source ended up. */
  paths: string[];
  /** Sources a move to another drive had to leave: changed, in use, or links. */
  leftBehind: string[];
  /** Links in the moved folder that should point into it but dangle or are missing. */
  notRepointed: string[];
}
/** Starts moving in the background; resolves to the id of the job in the queue. */
export const movePaths = (
  sources: string[],
  dest: string,
  replace: string[] = [],
  replaceIds: (string | null)[] = [],
) => invoke<number>("move_paths", { sources, dest, replace, replaceIds });

/** A copy, move or delete in the operations queue (`queue::JobView`). */
export interface Job {
  id: number;
  kind: "copy" | "move" | "trash";
  label: string;
  state: "queued" | "running" | "paused" | "done" | "failed" | "cancelled";
  bytesDone: number;
  bytesTotal: number;
  itemsDone: number;
  itemsTotal: number;
  /** Bytes per second, smoothed. */
  speed: number;
  etaSecs: number | null;
  error: string | null;
  /** `Copied`, `Moved` or `{ count, leftAlone }` once the job is done. */
  result: Copied | Moved | { count: number; leftAlone: number } | null;
}
/** Fires whenever a job started from this window changes. */
export const onQueueUpdate = (cb: (job: Job) => void): Promise<UnlistenFn> =>
  getCurrentWebviewWindow().listen<Job>("queue-update", (e) => cb(e.payload));
/** Fires when closing this window was refused because its operations are still going. */
export const onCloseBlocked = (cb: () => void): Promise<UnlistenFn> =>
  getCurrentWebviewWindow().listen("queue-close-blocked", () => cb());
export const queuePause = (id: number) => invoke<void>("queue_pause", { id });
export const queueResume = (id: number) => invoke<void>("queue_resume", { id });
export const queueCancel = (id: number) => invoke<void>("queue_cancel", { id });
export const queueDismiss = (id: number) => invoke<void>("queue_dismiss", { id });

/** Fires (debounced) when the folder this window watches changes on disk. */
export const onDirChanged = (cb: (path: string) => void): Promise<UnlistenFn> =>
  getCurrentWebviewWindow().listen<{ path: string }>("dir-changed", (e) => cb(e.payload.path));

/** Tabs saved for this window (`session::WindowTabs`), or null for a fresh window. */
export interface WindowTabs {
  tabs: string[];
  active: number;
}

export const windowTabs = () => invoke<WindowTabs | null>("window_tabs");
export const saveTabs = (tabs: string[], active: number) =>
  invoke<void>("save_tabs", { tabs, active });

/** Opens a new window showing `tabs`, its top-left corner at logical screen position (x, y). */
export const openWindow = (tabs: string[], x: number, y: number) =>
  invoke<void>("open_window", { tabs, x, y });

export function separatorOf(path: string): string {
  return path.includes("\\") ? "\\" : "/";
}

export function joinPath(base: string, name: string): string {
  const sep = separatorOf(base);
  return base.endsWith(sep) ? base + name : base + sep + name;
}

export function parentOf(path: string): string | null {
  const segs = pathSegments(path);
  return segs.length > 1 ? segs[segs.length - 2].path : null;
}

export function pathSegments(path: string): { name: string; path: string }[] {
  const sep = separatorOf(path);
  const parts = path.split(sep).filter(Boolean);
  const absolute = path.startsWith("/");
  return parts.map((name, i) => {
    let p = parts.slice(0, i + 1).join(sep);
    if (absolute) p = "/" + p;
    else if (i === 0) p += sep; // "C:" -> "C:\"
    return { name, path: p };
  });
}
