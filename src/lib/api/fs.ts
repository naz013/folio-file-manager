import { invoke } from "@tauri-apps/api/core";

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

export function joinPath(base: string, name: string): string {
  const sep = base.includes("\\") ? "\\" : "/";
  return base.endsWith(sep) ? base + name : base + sep + name;
}

export function pathSegments(path: string): { name: string; path: string }[] {
  const sep = path.includes("\\") ? "\\" : "/";
  const parts = path.split(sep).filter(Boolean);
  const absolute = path.startsWith("/");
  return parts.map((name, i) => {
    let p = parts.slice(0, i + 1).join(sep);
    if (absolute) p = "/" + p;
    else if (i === 0) p += sep; // "C:" -> "C:\"
    return { name, path: p };
  });
}
