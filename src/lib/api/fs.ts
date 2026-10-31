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

export const trashPaths = (targets: string[]) => invoke<void>("trash_paths", { targets });
export const renamePath = (path: string, newName: string) =>
  invoke<string>("rename_path", { path, newName });
export const createFolder = (parent: string, name: string) =>
  invoke<string>("create_folder", { parent, name });
export const copyPaths = (sources: string[], dest: string) =>
  invoke<string[]>("copy_paths", { sources, dest });
/** Result of `move_paths` (`commands::Moved`). */
export interface Moved {
  /** Where each source ended up. */
  paths: string[];
  /** Sources a move to another drive had to leave because they changed or are in use. */
  leftBehind: string[];
}
export const movePaths = (sources: string[], dest: string) =>
  invoke<Moved>("move_paths", { sources, dest });

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
