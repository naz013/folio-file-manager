import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
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

const decode = (path: string, [name, flags, size, modified]: WireEntry): Entry => ({
  name,
  path,
  isDir: (flags & FLAG_DIR) !== 0,
  size,
  modified,
  hidden: (flags & FLAG_HIDDEN) !== 0,
});

export async function listDir(path: string): Promise<Entry[]> {
  const wire = await invoke<WireEntry[]>("list_dir", { path });
  const base = joinPath(path, "");
  return wire.map((w) => decode(base + w[0], w));
}

/** Every item that carries `tag`, from any folder, with the tag ids of each, by path. */
export async function tagFiles(
  tag: number,
): Promise<{ entries: Entry[]; tags: Record<string, number[]> }> {
  const wire = await invoke<[string, WireEntry, number[]][]>("tag_files", { tag });
  return {
    entries: wire.map(([path, w]) => decode(path, w)),
    tags: Object.fromEntries(wire.map(([path, , ids]) => [path, ids])),
  };
}
export const homeDir = () => invoke<string>("home_dir");
export const watchDir = (path: string) => invoke<void>("watch_dir", { path });
export const openPath = (path: string) => invoke<void>("open_path", { path });

/** Starts a native OS drag so files can be dropped into Explorer and other apps. */
export const startDrag = (paths: string[]) => invoke<void>("start_drag", { paths });

/** The modifier keys held right now; a native drop doesn't report them. */
export const modifierKeys = () =>
  invoke<{ ctrl: boolean; shift: boolean; space: boolean }>("modifier_keys");

/**
 * Makes `path` the one file the `preview://` protocol serves, and resolves to its URL. Naming
 * a file replaces the last; `releasePreview` closes the access.
 */
export async function previewUrl(path: string): Promise<string> {
  await invoke<void>("preview_allow", { path });
  return convertFileSrc(path, "preview");
}

export const releasePreview = () => invoke<void>("preview_allow", { path: null });

/** The start of a text file; rejects for files that are not text. */
export const readTextPreview = (path: string) =>
  invoke<{ text: string; truncated: boolean }>("read_text_preview", { path });

/** An item in the Recycle Bin. */
export interface BinItem {
  id: string;
  name: string;
  original: string;
  /** Seconds since 1970. */
  deleted: number;
}
/** What an item weighs: bytes for a file, an entry count for a folder. */
export interface BinSize {
  id: string;
  size: number | null;
  entries: number | null;
}
/** The Recycle Bin as a place a tab can show, in the spot of a folder path. */
export const BIN_PATH = "folio:recycle-bin";

export const binList = () => invoke<BinItem[]>("bin_list");
export const binSizes = () => invoke<BinSize[]>("bin_sizes");
/** An item as listed; the core acts on it only if all of it still matches. */
export type BinRef = Pick<BinItem, "id" | "original" | "deleted">;
export const binRestore = (items: BinRef[]) => invoke<void>("bin_restore", { items });
/** Deletes items from the Recycle Bin for good; ask first. */
export const binPurge = (items: BinRef[]) => invoke<void>("bin_purge", { items });

/** A folder with its size from the scan cache. */
export interface SizedDir {
  path: string;
  name: string;
  bytes: number;
  files: number;
}
export interface BigFile {
  path: string;
  size: number;
  /** Seconds since 1970. */
  modified: number;
}
export interface ScanRoot {
  root: string;
  scannedAt: number;
  bytes: number;
  files: number;
}
export interface ScanProgress {
  root: string;
  files: number;
  bytes: number;
}
export interface ScanDone extends ScanProgress {
  cancelled: boolean;
  error: string | null;
}
/** Scans a folder in the background; progress and the end arrive as events. */
export const scanStart = (root: string) => invoke<void>("scan_start", { root });
export const scanCancel = () => invoke<void>("scan_cancel");
/** The subfolders of a scanned folder, largest first. */
export const scanChildren = (dir: string) => invoke<SizedDir[]>("scan_children", { dir });
export const scanLarge = (min: number, limit: number) =>
  invoke<BigFile[]>("scan_large", { min, limit });
/** Large files untouched for a while, biggest first; never from a protected location. */
export const cleanupLargeOld = (minBytes: number, olderThanDays: number, limit: number) =>
  invoke<BigFile[]>("cleanup_large_old", { minBytes, olderThanDays, limit });
/** Installers and archives in Downloads untouched for a while, biggest first. */
export const cleanupOldDownloads = (olderThanDays: number, limit: number) =>
  invoke<BigFile[]>("cleanup_old_downloads", { olderThanDays, limit });
/** An unfinished copy left behind (`*.folio-partial`): a file, or a folder with all in it. */
export interface PartialCopy extends BigFile {
  isDir: boolean;
}
/** Unfinished copies left alone for a while. Fails while a copy or move is running. */
export const cleanupPartials = (olderThanDays: number, limit: number) =>
  invoke<PartialCopy[]>("cleanup_partials", { olderThanDays, limit });
/** Moves listed unfinished copies to the Recycle Bin, those still exactly as listed. */
export const cleanupTrashPartials = (items: PartialCopy[]) =>
  invoke<CleanupTrash>("cleanup_trash_partials", { items });
/** Copies of one file: same size and same content, newest first. */
export interface DuplicateGroup {
  id: string;
  size: number;
  files: BigFile[];
}
export interface Duplicates {
  groups: DuplicateGroup[];
  /** False when the search stopped at its limits, so there may be more. */
  complete: boolean;
  /** Files that could not be read and were left out. */
  unreadable: number;
}
/** Stops the duplicate search in progress. */
export const cleanupDuplicatesCancel = () => invoke<void>("cleanup_duplicates_cancel");
/** Groups of identical files among the scanned files of at least `minBytes`. */
export const cleanupDuplicates = (minBytes: number) =>
  invoke<Duplicates>("cleanup_duplicates", { minBytes });
/** A reviewed group: every copy, and the ones to move to the Recycle Bin. */
export interface ReviewedGroup {
  files: BigFile[];
  remove: string[];
}
/** Moves the marked copies; the core never moves a group's last copy. */
export const cleanupTrashDuplicates = (groups: ReviewedGroup[]) =>
  invoke<CleanupTrash>("cleanup_trash_duplicates", { groups });
/** Result of `cleanup_trash`. */
export interface CleanupTrash {
  job: number | null;
  leftAlone: number;
}
/** Moves cleanup-listed files to the Recycle Bin, those still exactly as listed. */
/** `keep` are the copies groups of duplicates keep: never moved from a plain list. */
export const cleanupTrash = (items: BigFile[], keep: string[]) =>
  invoke<CleanupTrash>("cleanup_trash", { items, keep });
export interface Drive {
  path: string;
  total: number;
  free: number;
}
export const listDrives = () => invoke<Drive[]>("list_drives");
/** Bytes per category in a scanned folder: Documents, Images, Video, Audio, Archives, Apps, Other. */
export const scanCategories = (dir: string) => invoke<number[]>("scan_categories", { dir });
export const SCAN_CATEGORIES = [
  "Documents",
  "Images",
  "Video",
  "Audio",
  "Archives",
  "Apps",
  "Other",
] as const;
export const scanRunning = () => invoke<boolean>("scan_running");
/** The size of a scanned folder: [bytes, files], or null if it hasn't been scanned. */
export const scanSize = (dir: string) => invoke<[number, number] | null>("scan_size", { dir });
export const scanRoots = () => invoke<ScanRoot[]>("scan_roots");
export const onScanProgress = (cb: (p: ScanProgress) => void): Promise<UnlistenFn> =>
  listen<ScanProgress>("scan-progress", (e) => cb(e.payload));
export const onScanDone = (cb: (d: ScanDone) => void): Promise<UnlistenFn> =>
  listen<ScanDone>("scan-done", (e) => cb(e.payload));

/** Opt-in crash reporting, kept on this computer. */
export interface CrashState {
  /** null until the user has answered. */
  consent: boolean | null;
  sessions: number;
  crashed: number;
  reports: number;
  dir: string;
}
export const crashState = () => invoke<CrashState>("crash_state");
export const crashSet = (on: boolean) => invoke<CrashState>("crash_set", { on });
export const crashClear = () => invoke<CrashState>("crash_clear");

/** Fails with a reason while Folio is busy with files: an update must wait for it. */
export const updateGate = () => invoke<void>("update_gate");
export const updateRestart = () => invoke<void>("update_restart");

/** Puts the latest cut or copy on the system clipboard, so Explorer can paste it too. */
export const clipboardMirror = (paths: string[], cut: boolean) =>
  invoke<void>("clipboard_mirror", { paths, cut });
/** Files cut or copied in another program since the last look, if any. */
export const clipboardTake = () => invoke<[string[], boolean] | null>("clipboard_take");

/** An item on the clipboard shelf, checked against the disk when it was listed. */
export interface ShelfItem {
  path: string;
  mode: "copy" | "cut";
  /** False for an item that went missing after it was shelved. */
  exists: boolean;
  isDir: boolean;
  size: number;
}
export const shelfList = () => invoke<ShelfItem[]>("shelf_list");
/** Puts items on the shelf to copy or move later; the files are not touched. */
export const shelfAdd = (paths: string[], mode: "copy" | "cut") =>
  invoke<void>("shelf_add", { paths, mode });
export const shelfSetMode = (path: string, mode: "copy" | "cut") =>
  invoke<void>("shelf_set_mode", { path, mode });
/** Takes items off the shelf; the files are not touched. */
export const shelfRemove = (paths: string[]) => invoke<void>("shelf_remove", { paths });
export const shelfClear = () => invoke<void>("shelf_clear");

/** A tag: one of the seven colours, or a named tag with an optional colour. */
export interface Tag {
  id: number;
  name: string;
  /** One of the seven colour names, or null for a plain named tag. */
  color: string | null;
}
export const tagList = () => invoke<Tag[]>("tag_list");
export const tagCreate = (name: string, color: string | null) =>
  invoke<Tag>("tag_create", { name, color });
export const tagDelete = (id: number) => invoke<void>("tag_delete", { id });
/**
 * Tags `paths` with `tag`, or untags them if they all have it. `skipped` counts items that
 * couldn't be tagged.
 */
export const tagToggle = (paths: string[], tag: number) =>
  invoke<{ on: boolean; skipped: number }>("tag_toggle", { paths, tag });
/** The tagged items directly in `dir`: [name, tag ids]. */
export const tagsInDir = (dir: string) => invoke<[string, number[]][]>("tags_in_dir", { dir });

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
/** One item of a batch rename and the name it should get. */
export interface RenamePlan {
  path: string;
  newName: string;
}
/** Why each rename of the batch can't be done (null where it can), in order. */
export const checkRenames = (plan: RenamePlan[]) =>
  invoke<(string | null)[]>("check_renames", { plan });
/** Renames the batch as one undo step; resolves to each item's new path. */
export const renameMany = (plan: RenamePlan[]) => invoke<string[]>("rename_many", { plan });
export const createFile = (parent: string, name: string) =>
  invoke<string>("create_file", { parent, name });
export const openTerminal = (dir: string) => invoke<void>("open_terminal", { dir });
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

/** How far a PDF is made smaller (`ops::pdf::Preset`). */
export type PdfPreset = "lossless" | "high" | "balanced" | "small" | "smallest";
/** What a finished tool job (PDF or picture) made. */
export interface MadeFile {
  path: string;
  /** Bytes of the inputs together, and of the new file. */
  before: number;
  after: number;
  /** One file made smaller, as opposed to merged, turned or cropped. */
  compressed: boolean;
  /** Something the user should read, when the job did not go as it should. */
  note?: string;
}
/** Joins the PDFs in the order given into a new file called `name` next to the first. */
export const pdfMerge = (paths: string[], name: string) =>
  invoke<number>("pdf_merge", { paths, name });
/** Saves a smaller copy of a PDF as `name` next to it. */
export const pdfCompress = (path: string, preset: PdfPreset, name: string) =>
  invoke<number>("pdf_compress", { path, preset, name });

/** How far a picture is made smaller (`ops::image_edit::Preset`). */
export type ImagePreset = "high" | "balanced" | "small" | "smallest";
/** One edit of a picture (`ops::image_edit::Edit`). Crop is in pixels of the picture as shown. */
export type ImageEdit =
  | { kind: "rotate"; turns: number }
  | { kind: "compress"; preset: ImagePreset }
  | { kind: "crop"; x: number; y: number; width: number; height: number };
/** Edits a picture in the background: as a copy next to it, or in its place (the original
 * goes to the Recycle Bin and Undo brings it back). */
export const imageEdit = (path: string, edit: ImageEdit, save: "copy" | "replace") =>
  invoke<number>("image_edit", { path, edit, save });
/** How many bytes the picture would be after `preset`; rejects if it can't be made smaller. */
export const imageEstimate = (path: string, preset: ImagePreset) =>
  invoke<number>("image_estimate", { path, preset });

/** A copy, move or delete in the operations queue (`queue::JobView`). */
export interface Job {
  id: number;
  kind: "copy" | "move" | "trash" | "edit";
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
  result: Copied | Moved | MadeFile | { count: number; leftAlone: number } | null;
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
  /** Each tab's view mode, in tab order; "" (or absent) is the default. */
  views?: string[];
}

export const windowTabs = () => invoke<WindowTabs | null>("window_tabs");
export const saveTabs = (tabs: string[], active: number, views: string[]) =>
  invoke<void>("save_tabs", { tabs, active, views });

/** Opens a new window showing `tabs`, its top-left corner at logical screen position (x, y). */
export const openWindow = (tabs: string[], views: string[], x: number, y: number) =>
  invoke<void>("open_window", { tabs, views, x, y });

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
