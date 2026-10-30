import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

/** Mirrors `fs::Entry` in src-tauri/src/fs/mod.rs. */
export interface Entry {
  name: string;
  path: string;
  isDir: boolean;
  size: number;
  /** Milliseconds since the Unix epoch. */
  modified: number | null;
  hidden: boolean;
}

export const listDir = (path: string) => invoke<Entry[]>("list_dir", { path });
export const homeDir = () => invoke<string>("home_dir");
export const watchDir = (path: string) => invoke<void>("watch_dir", { path });
export const openPath = (path: string) => invoke<void>("open_path", { path });

/** Starts a native OS drag so files can be dropped into Explorer and other apps. */
export const startDrag = (paths: string[]) => invoke<void>("start_drag", { paths });

/** URL of the OS thumbnail (or icon) for `path`, served by the Rust `thumb://` protocol. */
export const thumbUrl = (path: string, size: number) => `${convertFileSrc(path, "thumb")}?s=${size}`;

/** Explorer's rule: dragging within a volume moves, across volumes copies. */
export function sameVolume(a: string, b: string): boolean {
  const root = (p: string) => (/^[a-zA-Z]:/.test(p) ? p.slice(0, 2).toLowerCase() : p.startsWith("\\\\") ? p.split("\\").slice(0, 4).join("\\").toLowerCase() : "/");
  return root(a) === root(b);
}

export const trashPaths = (targets: string[]) => invoke<void>("trash_paths", { targets });
export const renamePath = (path: string, newName: string) =>
  invoke<string>("rename_path", { path, newName });
export const createFolder = (parent: string, name: string) =>
  invoke<string>("create_folder", { parent, name });
export const copyPaths = (sources: string[], dest: string) =>
  invoke<string[]>("copy_paths", { sources, dest });
export const movePaths = (sources: string[], dest: string) =>
  invoke<string[]>("move_paths", { sources, dest });

/** Fires (debounced) when the watched folder changes on disk. */
export const onDirChanged = (cb: (path: string) => void): Promise<UnlistenFn> =>
  listen<{ path: string }>("dir-changed", (e) => cb(e.payload.path));

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
