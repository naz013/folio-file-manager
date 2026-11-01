<script lang="ts">
  import { onMount, tick } from "svelte";
  import { SvelteSet } from "svelte/reactivity";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import {
    copyPaths,
    createFile,
    createFolder,
    openTerminal,
    findConflicts,
    tagCreate,
    tagDelete,
    tagList,
    clipboardMirror,
    onScanDone,
    scanChildren,
    scanRunning,
    scanCancel,
    scanStart,
    crashSet,
    crashState,
    clipboardTake,
    shelfAdd,
    shelfClear,
    shelfList,
    shelfRemove,
    shelfSetMode,
    type ShelfItem,
    tagFiles,
    tagToggle,
    tagsInDir,
    type Tag,
    homeDir,
    listDir,
    movePaths,
    onDirChanged,
    openPath,
    onCloseBlocked,
    onQueueUpdate,
    openWindow,
    joinPath,
    parentOf,
    pathSegments,
    queueCancel,
    queueDismiss,
    queuePause,
    queueResume,
    renamePath,
    renameMany,
    type RenamePlan,
    sameVolume,
    saveTabs,
    modifierKeys,
    showContextMenu,
    startDrag,
    redoOperation,
    trashPaths,
    undoOperation,
    watchDir,
    windowTabs,
    type Copied,
    type Choice,
    type Conflict,
    type Entry,
    type Job,
    type Moved,
  } from "$lib/api/fs";
  import { formatSize } from "$lib/format";
  import { sizeKey, sortEntries, type SortKey, type SortState } from "$lib/sort";
  import ConflictDialog from "$lib/components/ConflictDialog.svelte";
  import ContextMenu, { type MenuItem } from "$lib/components/ContextMenu.svelte";
  import QuickLook from "$lib/components/QuickLook.svelte";
  import TagDialog from "$lib/components/TagDialog.svelte";
  import BulkRenameDialog from "$lib/components/BulkRenameDialog.svelte";
  import TagChips from "$lib/components/TagChips.svelte";
  import Inspector from "$lib/components/Inspector.svelte";
  import Palette, { type PaletteItem } from "$lib/components/Palette.svelte";
  import ShelfPanel from "$lib/components/ShelfPanel.svelte";
  import { clearRecent, loadRecent, recordRecent } from "$lib/palette";
  import { tagColor } from "$lib/tags";
  import OpsPill from "$lib/components/OpsPill.svelte";
  import Sidebar from "$lib/components/Sidebar.svelte";
  import TabBar from "$lib/components/TabBar.svelte";
  import Toolbar, { type View } from "$lib/components/Toolbar.svelte";
  import { createSpring } from "$lib/spring";
  import {
    movePinned,
    pinFolders,
    settings,
    startPath,
    unpinFolder,
    updateSettings,
  } from "$lib/settings.svelte";
  import SettingsDialog from "$lib/components/SettingsDialog.svelte";
  import CrashPrompt from "$lib/components/CrashPrompt.svelte";
  import StorageView from "$lib/components/StorageView.svelte";
  import CleanupView from "$lib/components/CleanupView.svelte";
  import BinView from "$lib/components/BinView.svelte";
  import { applyTheme, loadTheme, onThemeChange, setTheme, type Theme } from "$lib/theme";
  import FileView from "$lib/components/FileView.svelte";
  import { measureJumps, measureScroll, nextPaint, perfConfig, perfReport } from "$lib/perf";

  let home = $state("");
  let path = $state("");
  // Raw (not deeply proxied): listings are replaced wholesale, and proxying 50k objects is slow.
  let entries = $state.raw<Entry[]>([]);
  let settingsOpen = $state(false);
  let storageOpen = $state(false);
  let cleanupOpen = $state(false);
  let binOpen = $state(false);
  /** A size scan is running (it is shared by every window). */
  let scanning = $state(false);

  async function startScan() {
    if (tagView) return notify("Open a folder first", true);
    // Set first: the scan can end before the call returns.
    scanning = true;
    try {
      await scanStart(path);
      notify("Scanning folder sizes in the background");
    } catch (e) {
      scanning = false;
      notify(String(e), true);
    }
  }
  /** Asked once, in the first window, until the user has answered. */
  let askCrash = $state(false);
  let theme = $state<Theme>(loadTheme());

  let back = $state<string[]>([]);
  let forward = $state<string[]>([]);

  /**
   * Tabs of this window. The active tab's live state is the page state above (path,
   * history, selection); inactive tabs keep a plain, non-reactive memory and are
   * re-listed on switch, so only one listing is ever held.
   */
  interface Tab {
    id: number;
    path: string;
    /** Set once the view was changed in this tab; null follows the default. */
    view: View | null;
  }
  interface TabMemory {
    back: string[];
    forward: string[];
    selection: string[];
    scrollTop: number;
  }
  let tabs = $state<Tab[]>([]);
  let active = $state(-1);

  // Each tab remembers its own view; a tab that was never changed follows the default from
  // Settings, which shows at once.
  let view = $derived<View>(tabs[active]?.view ?? settings.defaultView);

  function setView(next: View) {
    if (tabs[active]) tabs[active].view = next;
  }
  let nextTabId = 0;
  // Deliberately not reactive: nothing renders from it, and proxying large selections is slow.
  // eslint-disable-next-line svelte/prefer-svelte-reactivity
  const memory = new Map<number, TabMemory>();
  /** Bumped by every navigation. A listing that arrives after a newer one started is dropped. */
  let navGen = 0;
  /** Id of the tab whose listing is on screen; differs from the active tab while it loads. */
  let shownTabId = -1;
  /** Set once restored tabs are loaded, so the empty initial state is never saved. */
  let restored = $state(false);
  let saveTimer: ReturnType<typeof setTimeout>;

  const asView = (v: string | undefined): View | null => (v === "grid" || v === "list" ? v : null);

  const folderName = (p: string) => pathSegments(p).at(-1)?.name ?? p;
  let tabPaths = $derived(tabs.map((t) => t.path));
  let tabViews = $derived(tabs.map((t) => t.view ?? ""));
  let tabList = $derived(tabs.map((t, i) => ({ id: t.id, title: folderName(tabPaths[i]) })));

  const selection = new SvelteSet<string>();
  let anchor = $state(-1);
  let cursor = $state(-1);
  let renaming = $state<string | null>(null);

  /** Items marked with Ctrl+X or Ctrl+C, kept until they are pasted or removed. */
  let shelf = $state<ShelfItem[]>([]);
  let shelfOpen = $state(false);
  let cutSet = $derived(new Set(shelf.filter((i) => i.mode === "cut").map((i) => i.path)));
  /** Set while an item pulled out of the shelf panel is being dragged. */
  let shelfDrag: ShelfItem | null = null;

  async function loadShelf() {
    shelf = await shelfList().catch(() => shelf);
  }

  /** Folder under a native drag (from Folio itself or another app). */
  let dropTarget = $state<string | null>(null);
  /** A folder drag is over the sidebar's Favorites, where dropping pins it. */
  let pinHover = $state(false);

  let toast = $state<{ text: string; error: boolean } | null>(null);
  let toastTimer: ReturnType<typeof setTimeout>;

  let fileView: FileView;
  let toolbar: Toolbar | undefined = $state();

  /** Ids of the tags the folder is filtered by: an item must have all of them. */
  let tagFilter = $state<number[]>([]);
  /** The tag whose items the list shows, from every folder, instead of a folder. */
  let tagView = $state<Tag | null>(null);
  /** The list's order, set from the column headers. */
  let sort = $state<SortState>({ key: "name", desc: false });
  /** Sizes of the folders shown, from the scan cache; empty where nothing was scanned. */
  let folderSizes = $state<Record<string, number>>({});
  let visible = $derived(
    sortEntries(
      entries.filter(
        (e) =>
          (settings.showHidden || !e.hidden) &&
          tagFilter.every((id) => tagMap[e.path]?.includes(id)),
      ),
      sort,
      (p) => folderSizes[sizeKey(p)],
    ),
  );

  /** A rubber band over the empty space selects these items. */
  function bandSelect(paths: string[]) {
    selection.clear();
    for (const p of paths) selection.add(p);
    cursor = anchor = paths.length ? visible.findIndex((e) => e.path === paths.at(-1)) : -1;
  }

  /** Clicking a header sorts by it; clicking it again reverses the order. */
  function sortBy(key: SortKey) {
    keepCursor(() => {
      sort = { key, desc: sort.key === key ? !sort.desc : key === "modified" || key === "size" };
    });
  }

  /** Runs a change that may re-order the list, then puts the cursor and anchor back on their items. */
  function keepCursor(change: () => void) {
    const at = visible[cursor]?.path;
    const from = visible[anchor]?.path;
    change();
    if (at !== undefined) cursor = visible.findIndex((e) => e.path === at);
    if (from !== undefined) anchor = visible.findIndex((e) => e.path === from);
  }

  /** Reads the sizes of the folders in `dir` from the cache, without holding up the listing. */
  async function loadFolderSizes(dir: string) {
    const sizes = await scanChildren(dir).catch(() => []);
    // A tag's items come from many folders, so there is nothing to read for them.
    if (dir !== path || tagView) return;
    keepCursor(() => {
      folderSizes = Object.fromEntries(sizes.map((s) => [sizeKey(s.path), s.bytes]));
    });
  }

  function notify(text: string, error = false) {
    toast = { text, error };
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => (toast = null), error ? 5000 : 2500);
  }

  const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;

  /** Reloads the current folder, keeping the selection for paths that still exist. */
  async function refresh(select?: string[]) {
    const gen = navGen;
    let list: Entry[];
    let tags: Record<string, number[]>;
    try {
      if (tagView) {
        ({ entries: list, tags } = await tagFiles(tagView.id));
      } else {
        [list, tags] = await Promise.all([listDir(path), fetchTagMap(path)]);
      }
    } catch (e) {
      notify(String(e), true);
      return;
    }
    if (gen !== navGen) return; // the tab switched or navigated meanwhile
    entries = list;
    tagMap = tags;
    // Only what is shown can stay selected.
    const existing = new Set(visible.map((e) => e.path));
    const keep = (select ?? [...selection]).filter((p) => existing.has(p));
    selection.clear();
    keep.forEach((p) => selection.add(p));
    cursor = keep.length ? visible.findIndex((e) => e.path === keep[keep.length - 1]) : -1;
    anchor = cursor;
  }

  /** Shows `target` in the active tab. Resolves to false on error, null if a newer navigation took over. */
  async function load(target: string, quiet = false): Promise<boolean | null> {
    const gen = ++navGen;
    let list: Entry[];
    let tags: Record<string, number[]>;
    try {
      [list, tags] = await Promise.all([listDir(target), fetchTagMap(target)]);
    } catch (e) {
      if (gen !== navGen) return null;
      if (!quiet) notify(String(e), true);
      return false;
    }
    if (gen !== navGen) return null;
    entries = list;
    path = target;
    tagView = null;
    tagFilter = [];
    tagMap = tags;
    recordRecent(target);
    if (tabs[active]) {
      tabs[active].path = target;
      shownTabId = tabs[active].id;
    }
    selection.clear();
    anchor = cursor = -1;
    renaming = null;
    fileView?.scrollToTop();
    loadFolderSizes(target);
    watchDir(target).catch((e) => console.warn("watch failed", e));
    return true;
  }

  /** Opens `target`; resolves to whether it is now shown (null: a newer navigation won). */
  async function navigate(target: string, quiet = false): Promise<boolean | null> {
    if (target === path && !tagView) return true;
    const from = path;
    const ok = await load(target, quiet);
    if (ok) {
      if (from) back.push(from);
      forward = [];
    }
    return ok;
  }

  async function goBack() {
    // A tag view is not a step in the history: back leaves it, for the folder it came from.
    if (tagView) {
      await load(path);
      return;
    }
    const prev = back.pop();
    if (!prev) return;
    const from = path;
    const ok = await load(prev);
    if (ok) forward.push(from);
    else if (ok === false) back.push(prev);
  }

  async function goForward() {
    const next = forward.pop();
    if (!next) return;
    const from = path;
    const ok = await load(next);
    if (ok) back.push(from);
    else if (ok === false) forward.push(next);
  }

  // ---- Tabs ----

  const newTab = (p: string, view: View | null = null): Tab => ({
    id: nextTabId++,
    path: p,
    view,
  });

  /** Remembers the live page state of the active tab before another tab takes over. */
  function snapshot() {
    const t = tabs[active];
    if (!t) return;
    // Switched away before this tab's listing arrived: the selection and scroll on
    // screen belong to the previous tab, so keep what this tab remembered.
    const live = t.id === shownTabId;
    const prev = memory.get(t.id);
    memory.set(t.id, {
      back: [...back],
      forward: [...forward],
      selection: live ? [...selection] : (prev?.selection ?? []),
      scrollTop: live ? (fileView?.scrollElement().scrollTop ?? 0) : (prev?.scrollTop ?? 0),
    });
  }

  async function activate(index: number) {
    if (index === active || !tabs[index]) return;
    snapshot();
    active = index;
    const t = tabs[index];
    const m = memory.get(t.id);
    back = m ? [...m.back] : [];
    forward = m ? [...m.forward] : [];
    // A restored folder may be gone; fall back to home rather than an empty tab.
    let ok = await load(t.path);
    if (ok === false && t.path !== home) ok = await load(home);
    if (ok === null) return; // a newer navigation took over
    if (!ok) {
      // Nothing could be listed: show an empty tab, not the previous tab's files.
      entries = [];
      path = t.path;
      selection.clear();
    }
    if (!m) return;
    const gen = navGen;
    await tick();
    if (gen !== navGen) return;
    if (m.scrollTop) fileView.scrollElement().scrollTop = m.scrollTop;
    if (!m.selection.length) return;
    // Restore the selection after the folder has painted; on a huge selection it is the slow part.
    await nextPaint();
    if (gen !== navGen) return;
    const existing = new Set(entries.map((e) => e.path));
    const keep = m.selection.filter((p) => existing.has(p));
    keep.forEach((p) => selection.add(p));
    cursor = anchor = keep.length ? visible.findIndex((e) => e.path === keep[keep.length - 1]) : -1;
  }

  /** Opens `target` in a new tab right after the active one. */
  function openTab(target: string, background = false) {
    tabs.splice(active + 1, 0, newTab(target));
    if (!background) activate(active + 1);
  }

  async function closeTab(index: number) {
    if (tabs.length === 1) {
      await closeWindow();
      return;
    }
    const [closed] = tabs.splice(index, 1);
    memory.delete(closed.id);
    if (index < active) active--;
    else if (index === active) {
      active = -1; // the closed tab's state must not be snapshotted into its neighbour
      await activate(Math.min(index, tabs.length - 1));
    }
  }

  function moveTab(from: number, to: number) {
    const activeId = tabs[active]?.id;
    const [t] = tabs.splice(from, 1);
    tabs.splice(to, 0, t);
    active = tabs.findIndex((x) => x.id === activeId);
  }

  async function detachTab(index: number, x: number, y: number) {
    if (tabs.length === 1) return;
    const { id, path: target } = tabs[index];
    try {
      await openWindow([target], [tabs[index].view ?? ""], x, y);
    } catch (e) {
      notify(String(e), true);
      return;
    }
    // Tabs may have changed while the window opened; close the detached one by id.
    const now = tabs.findIndex((t) => t.id === id);
    if (now >= 0) await closeTab(now);
  }

  function cycleTab(delta: number) {
    if (tabs.length > 1) activate((active + delta + tabs.length) % tabs.length);
  }

  function persistTabs(paths = tabPaths, index = active, views = tabViews) {
    clearTimeout(saveTimer);
    return saveTabs(paths, Math.max(index, 0), views).catch((e) =>
      console.warn("saving tabs failed", e),
    );
  }

  /** Closing goes through the onCloseRequested handler in onMount, which saves first. */
  const closeWindow = () => getCurrentWindow().close();

  // Save the tab list shortly after it settles.
  $effect(() => {
    if (!restored) return;
    const paths = tabPaths;
    const index = active;
    const views = tabViews;
    clearTimeout(saveTimer);
    saveTimer = setTimeout(() => persistTabs(paths, index, views), 300);
  });

  function open(entry: Entry) {
    if (entry.isDir) navigate(entry.path);
    else openPath(entry.path).catch((e) => notify(String(e), true));
  }

  // ---- Selection ----

  function selectOnly(index: number) {
    selection.clear();
    if (index >= 0) selection.add(visible[index].path);
    anchor = cursor = index;
  }

  function selectRange(from: number, to: number) {
    selection.clear();
    const [a, b] = from < to ? [from, to] : [to, from];
    for (let i = Math.max(a, 0); i <= b; i++) selection.add(visible[i].path);
  }

  function itemClick(index: number, e: MouseEvent) {
    const p = visible[index].path;
    if (e.shiftKey && anchor >= 0) {
      selectRange(anchor, index);
      cursor = index;
    } else if (e.ctrlKey || e.metaKey) {
      if (selection.has(p)) selection.delete(p);
      else selection.add(p);
      anchor = cursor = index;
    } else {
      selectOnly(index);
    }
  }

  function moveCursor(delta: number, extend: boolean) {
    if (!visible.length) return;
    const next = cursor < 0 ? 0 : Math.min(visible.length - 1, Math.max(0, cursor + delta));
    if (extend && anchor >= 0) selectRange(anchor, next);
    else {
      selection.clear();
      selection.add(visible[next].path);
      anchor = next;
    }
    cursor = next;
    fileView.scrollToIndex(next);
  }

  // ---- Operations ----

  const selected = () => visible.filter((e) => selection.has(e.path));

  async function run<T>(op: () => Promise<T>, done?: (r: T) => string): Promise<T | undefined> {
    try {
      const r = await op();
      if (done) notify(done(r));
      return r;
    } catch (e) {
      notify(String(e), true);
    }
  }

  /** Ctrl+Z / Ctrl+Y: reverse or repeat the latest file operation. */
  async function stepHistory(redo: boolean) {
    const name = await run(redo ? redoOperation : undoOperation);
    if (name !== undefined) notify(`${redo ? "Redid" : "Undid"}: ${name}`);
    // Partly done, too: what was reversed shows even when something couldn't be.
    await refresh([]);
  }

  async function trashSelected() {
    const items = selected();
    if (!items.length) return;
    // Runs in the background; `onJobUpdate` reports and refreshes when it is done.
    await run(() => trashPaths(items.map((e) => e.path)));
  }

  async function newFolder() {
    if (tagView) return notify("Open a folder first: a tag's items are in many folders", true);
    const created = await run(() => createFolder(path, "New folder"));
    if (created) {
      await refresh([created]);
      renaming = created;
    }
  }

  /** Makes an empty file with `name` in the folder shown and opens its name for editing. */
  async function newFile(name = "New file.txt") {
    if (tagView) return notify("Open a folder first: a tag's items are in many folders", true);
    const created = await run(() => createFile(path, name));
    if (created) {
      await refresh([created]);
      renaming = created;
    }
  }

  async function terminalIn(dir: string) {
    await run(() => openTerminal(dir));
  }

  async function commitRename(entry: Entry, name: string) {
    if (renaming !== entry.path) return;
    renaming = null;
    const trimmed = name.trim();
    if (!trimmed || trimmed === entry.name) return;
    const renamed = await run(() => renamePath(entry.path, trimmed));
    if (renamed) await refresh([renamed]);
  }

  /** What a transfer did: whether the user cancelled, and which items were queued. */
  type Transferred = { cancelled: boolean; started: string[] };

  async function transfer(paths: string[], dest: string, copy: boolean): Promise<Transferred> {
    return transferBatch([{ paths, copy }], dest);
  }

  /**
   * Copies and moves several sets of items into `dest` after one round of questions about
   * name clashes, so a cancel stops all of them.
   */
  async function transferBatch(
    groups: { paths: string[]; copy: boolean }[],
    dest: string,
  ): Promise<Transferred> {
    const cancelled = { cancelled: true, started: [] };
    // One question at a time.
    if (asking) return cancelled;
    const everything = groups.flatMap((g) => g.paths);
    const conflicts = await run(() => findConflicts(everything, dest));
    if (!conflicts) return cancelled;
    let choices: Record<string, Choice> = {};
    if (conflicts.length) {
      const asked = await askAboutConflicts(
        conflicts,
        groups.every((g) => g.copy) ? "copy" : "move",
      );
      if (!asked) return cancelled;
      choices = asked;
    }
    const started: string[] = [];
    for (const { paths, copy } of groups) {
      const sources = paths.filter((p) => choices[p] !== "skip");
      if (!sources.length) continue;
      const replace = sources.filter((p) => choices[p] === "replace");
      const replaceIds = replace.map(
        (p) => conflicts.find((c) => c.source === p)?.existingId ?? null,
      );
      const id = await run(() =>
        copy
          ? copyPaths(sources, dest, replace, replaceIds)
          : movePaths(sources, dest, replace, replaceIds),
      );
      if (id !== undefined) started.push(...sources);
    }
    return { cancelled: false, started };
  }

  // ---- Name conflicts ----

  /** The conflict being asked about, and how many are still to come. */
  let asking = $state<{ conflict: Conflict; remaining: number; verb: "copy" | "move" } | null>(
    null,
  );
  let answer: ((choice: Choice | "cancel", all: boolean) => void) | null = null;

  /** Asks about each conflict in turn; "do this for all" settles the rest. Null if cancelled. */
  async function askAboutConflicts(
    conflicts: Conflict[],
    verb: "copy" | "move",
  ): Promise<Record<string, Choice> | null> {
    const choices: Record<string, Choice> = {};
    for (let i = 0; i < conflicts.length; i++) {
      const conflict = conflicts[i];
      asking = { conflict, remaining: conflicts.length - i - 1, verb };
      const [choice, all] = await new Promise<[Choice | "cancel", boolean]>((resolve) => {
        answer = (c, a) => resolve([c, a]);
      });
      asking = null;
      if (choice === "cancel") return null;
      // Replace only where it is allowed; the rest keep both.
      const pick = (c: Conflict): Choice =>
        choice === "replace" && !c.canReplace ? "both" : choice;
      choices[conflict.source] = pick(conflict);
      if (all) {
        for (const rest of conflicts.slice(i + 1)) choices[rest.source] = pick(rest);
        break;
      }
    }
    return choices;
  }

  // ---- Operations queue ----

  let jobs = $state<Job[]>([]);
  /** Jobs whose end has been reported, so each is reported once. */
  const reported = new SvelteSet<number>();

  function onJobUpdate(job: Job) {
    const at = jobs.findIndex((j) => j.id === job.id);
    if (at < 0) jobs.push(job);
    else jobs[at] = job;
    if (reported.has(job.id)) return;
    if (job.state === "done") {
      reported.add(job.id);
      reportDone(job);
    } else if (job.state === "failed" || job.state === "cancelled") {
      reported.add(job.id);
      notify(job.error ?? "Failed", job.state === "failed");
      // What was done before it stopped shows too.
      refresh([]);
    }
  }

  /** Dismissed by the user, or by itself once it has been seen. */
  function dismissJob(id: number) {
    jobs = jobs.filter((j) => j.id !== id);
    queueDismiss(id).catch(() => {});
  }

  function reportDone(job: Job) {
    if (job.kind === "trash") {
      const { count, leftAlone } = (job.result as { count: number; leftAlone: number } | null) ?? {
        count: 0,
        leftAlone: 0,
      };
      const note = leftAlone
        ? ` ${plural(leftAlone, "item")} changed before it could be deleted and was left alone.`
        : "";
      notify(`Moved ${plural(count, "item")} to Recycle Bin.${note}`, leftAlone > 0);
      // Finished and quiet: no need to keep the row around.
      if (!leftAlone) dismissJob(job.id);
      refresh([]);
      return;
    }
    const result = job.result as Copied | Moved | null;
    if (!result) return;
    const copy = job.kind === "copy";
    const dest = result.dest;
    const created = result.paths;
    const verb = copy ? "Copied" : "Moved";
    const done = created.length ? `${verb} ${plural(created.length, "item")}. ` : "";
    // Shown in the error style so it's noticed; nothing is lost either way.
    const notes: string[] = [];
    if ("skipped" in result && result.skipped.length) {
      notes.push(`Skipped ${plural(result.skipped.length, "link")}: links can't be copied yet.`);
    }
    if ("leftBehind" in result && result.leftBehind.length) {
      const n = result.leftBehind.length;
      notes.push(
        `${plural(n, "item")} stayed in the original folder: changed during the move, in use, or a link.`,
      );
    }
    if (result.notRepointed.length) {
      const n = result.notRepointed.length;
      notes.push(
        `${plural(n, "link")} in the ${copy ? "copy" : "moved folder"} couldn't be pointed at its new place and may not work or may be missing.`,
      );
    }
    if (notes.length) {
      notify(`${done}${notes.join(" ")}`, true);
    } else {
      notify(`${verb} ${plural(created.length, "item")}`);
      dismissJob(job.id);
    }
    refresh(dest === path ? created : []);
  }

  /** Pastes the shelf into the folder shown: what was cut moves, what was copied is copied. */
  async function paste() {
    // Files cut or copied in Explorer since the last look arrive as a group on the shelf.
    const outside = await clipboardTake().catch(() => null);
    if (outside) await run(() => shelfAdd(outside[0], outside[1] ? "cut" : "copy"));
    // Another window may have used the shelf since it was last read.
    await loadShelf();
    await pasteItems(shelf);
  }

  /** Whether `inner` is `outer` or lies inside it. */
  function isInside(inner: string, outer: string): boolean {
    const norm = (p: string) => p.replace(/[\\/]+$/, "").toLowerCase();
    const [i, o] = [norm(inner), norm(outer)];
    return i === o || i.startsWith(o + "\\") || i.startsWith(o + "/");
  }

  /**
   * Pastes `items` from the shelf into the folder shown, and takes off the shelf what was
   * queued. An item that can't be pasted here stays on it.
   */
  async function pasteItems(items: ShelfItem[]) {
    if (tagView) return notify("Open a folder first: a tag's items are in many folders", true);
    const dest = path;
    const ready = items.filter((i) => i.exists);
    if (!ready.length) return notify("Nothing on the shelf can be pasted", true);
    // A folder can't go into itself, and what is inside another shelved item travels with it.
    const usable = ready.filter(
      (i) =>
        !(i.isDir && isInside(dest, i.path)) &&
        !ready.some((o) => o !== i && o.isDir && isInside(i.path, o.path)),
    );
    if (usable.length < ready.length) {
      notify(
        "Some items stay on the shelf: a folder can't go into itself, or is inside another item",
        true,
      );
    }
    const groups = (["cut", "copy"] as const)
      .map((mode) => ({
        copy: mode === "copy",
        paths: usable.filter((i) => i.mode === mode).map((i) => i.path),
      }))
      .filter((g) => g.paths.length);
    if (!groups.length) return;
    const { started } = await transferBatch(groups, dest);
    if (started.length) {
      await run(() => shelfRemove(started));
      await loadShelf();
    }
  }

  async function shelfMark(mode: "copy" | "cut") {
    const paths = selected().map((e) => e.path);
    if (!paths.length) return;
    await run(() => shelfAdd(paths, mode));
    // The latest batch also goes to the system clipboard, for Explorer.
    await clipboardMirror(paths, mode === "cut").catch(() =>
      notify("Could not put the files on the system clipboard", true),
    );
    await loadShelf();
    notify(
      `Added ${plural(paths.length, "item")} to the shelf: ${mode === "cut" ? "move" : "copy"}`,
    );
  }

  async function shelfRemoveItems(paths: string[]) {
    await run(() => shelfRemove(paths));
    await loadShelf();
  }

  /** A shelf item dropped on a folder: pasted there, by its own mode. */
  async function pasteShelfItem(item: ShelfItem, dest: string) {
    const { started } = await transfer([item.path], dest, item.mode === "copy");
    if (started.length) {
      await run(() => shelfRemove([item.path]));
      await loadShelf();
    }
  }

  /** An item pulled out of the shelf panel is dragged like a file in the list. */
  function dragShelfItem(item: ShelfItem) {
    shelfDrag = item;
    dragOut([item.path]);
  }

  // ---- Native drag & drop ----

  /** Finds the folder element under a drag position (physical pixels). */
  function folderAt(pos: { x: number; y: number }): string | null {
    const el = document.elementFromPoint(pos.x / devicePixelRatio, pos.y / devicePixelRatio);
    return el?.closest<HTMLElement>("[data-drop-path]")?.dataset.dropPath ?? null;
  }

  /** Explorer's rule: Ctrl copies, Shift moves, otherwise it follows the drive. */
  function copyOnDrop(
    keys: { ctrl: boolean; shift: boolean },
    sources: string[],
    dest: string,
  ): boolean {
    if (keys.ctrl) return true;
    if (keys.shift) return false;
    return !sources.every((s) => sameVolume(s, dest));
  }

  /**
   * The modifier keys as of the last poll during the drag. People often let go of Ctrl or
   * Shift as they drop, so reading them after the drop arrives would see them already up.
   */
  let dragKeys = { ctrl: false, shift: false };

  // ---- Spring-loaded folders ----

  /** The folder whose countdown is running, for the ring. */
  let springPath = $state<string | null>(null);
  let springMs = $state(0);
  /** What is being dragged; those folders never spring open. */
  let dragPaths: string[] = [];
  /** Where the drag is, in CSS pixels, for auto-scroll and for re-checking after a spring. */
  let dragPos: { x: number; y: number } | null = null;
  /** Where the tab was when a spring-loaded trip began; the trip is one history entry. */
  let trip: { from: string } | null = null;
  let dragTimer: ReturnType<typeof setInterval> | undefined;
  let spaceWasDown: boolean | null = null;

  /**
   * What the drag can spring-open: a folder (its path), a tab ("tab:<id>") or the new-tab
   * button ("new"). A target that just opened waits for the pointer to leave before it can
   * open again, so hovering + doesn't keep adding tabs.
   */
  let spent: string | null = null;

  const spring = createSpring({
    delayMs: () => settings.springDelayMs,
    eligible: (target) => {
      if (target === spent) return false;
      if (target === "new") return true;
      if (target.startsWith("tab:")) return Number(target.slice(4)) !== tabs[active]?.id;
      return target !== path && !dragPaths.includes(target);
    },
    open: springGo,
    ring: (target, ms) => {
      springPath = target;
      springMs = ms;
    },
  });

  /** Counts drags, so work that outlives its drag can tell. */
  let dragId = 0;

  /** Ends the current spring-loaded trip: the folders it went through are one step back. */
  async function settleTrip() {
    const started = trip;
    trip = null;
    if (!started || path === started.from) return;
    if (settings.returnAfterDrop) {
      // The tab goes back to where the drag began before it is left behind.
      await load(started.from, true);
    } else {
      back.push(started.from);
      forward = [];
    }
  }

  async function springGo(target: string) {
    const id = dragId;
    if (target === "new" || target.startsWith("tab:")) {
      // Another tab has its own history, so the trip so far is complete.
      await settleTrip();
      spent = target;
      if (target === "new") openTab(path);
      else {
        const index = tabs.findIndex((t) => t.id === Number(target.slice(4)));
        if (index >= 0) await activate(index);
      }
      await nextPaint();
      if (id === dragId && dragPos) hoverAt(dragPos);
      return;
    }
    const from = path;
    if (await load(target, true)) {
      if (id === dragId) {
        trip ??= { from };
      } else if (!settings.returnAfterDrop) {
        // The drag ended while this was loading: its trip is over, so record it now.
        back.push(from);
        forward = [];
      }
      // The list under the pointer changed: whatever is there now starts its own countdown.
      await nextPaint();
      if (id === dragId && dragPos) hoverAt(dragPos);
    }
  }

  /** Whether a drag at this position is over the Favorites but not on one of them (pins). */
  function pinZoneAt(el: Element | null): boolean {
    return !el?.closest("[data-drop-path]") && !!el?.closest("[data-pin-drop]");
  }

  /** The dragged folders, for pinning; a drag of files (or from another app) pins nothing. */
  function foldersOf(paths: string[]): string[] {
    return paths.filter((p) => entries.some((e) => e.path === p && e.isDir));
  }

  function hoverAt(pos: { x: number; y: number }) {
    dragPos = pos;
    const el = document.elementFromPoint(pos.x, pos.y);
    dropTarget = el?.closest<HTMLElement>("[data-drop-path]")?.dataset.dropPath ?? null;
    pinHover = !shelfDrag && pinZoneAt(el) && foldersOf(dragPaths).length > 0;
    const target = dropTarget ?? el?.closest<HTMLElement>("[data-spring]")?.dataset.spring ?? null;
    if (target !== spent) spent = null;
    spring.hover(target);
  }

  const EDGE = 48;
  const MAX_SCROLL = 24;

  /** Scrolls the list while the drag is near its top or bottom edge. */
  function autoScroll() {
    if (!dragPos || !fileView) return;
    const list = fileView.scrollElement();
    const box = list.getBoundingClientRect();
    if (dragPos.x < box.left || dragPos.x > box.right) return;
    const above = box.top + EDGE - dragPos.y;
    const below = dragPos.y - (box.bottom - EDGE);
    const before = list.scrollTop;
    if (above > 0) list.scrollBy(0, -Math.min(MAX_SCROLL, (above / EDGE) * MAX_SCROLL));
    else if (below > 0) list.scrollBy(0, Math.min(MAX_SCROLL, (below / EDGE) * MAX_SCROLL));
    // Different items are under the pointer now.
    if (list.scrollTop !== before) hoverAt(dragPos);
  }

  async function dragTick() {
    autoScroll();
    // An in-app drag gets real key events; only a native one needs the keys polled.
    if (inAppDrag) return;
    const keys = await modifierKeys().catch(() => null);
    if (!keys) return;
    dragKeys = keys;
    // A drag owns the keyboard, so Space is read from here rather than a key event. The
    // first sample only says whether it was already down.
    if (spaceWasDown !== null && keys.space && !spaceWasDown) spring.openNow();
    spaceWasDown = keys.space;
  }

  function dragStarted(paths: string[]) {
    dragId++;
    dragPaths = paths;
    if (dragTimer === undefined) dragTimer = setInterval(dragTick, 50);
  }

  /** The drag ended, dropped or cancelled. Settles where the tab stays. */
  async function dragEnded() {
    dragId++;
    clearInterval(dragTimer);
    dragTimer = undefined;
    spaceWasDown = null;
    spent = null;
    dragPos = null;
    dragPaths = [];
    spring.stop();
    dropTarget = null;
    pinHover = false;
    const started = trip;
    trip = null;
    if (!started) return;
    if (settings.returnAfterDrop) {
      await load(started.from, true);
    } else if (path !== started.from) {
      // The whole trip is one step back.
      back.push(started.from);
      forward = [];
    }
  }

  /** Where a drop that lands on no folder goes: the folder shown, if one is. */
  const dropFallback = (): string | null => (tagView ? null : path);

  async function dropPaths(paths: string[], dest: string) {
    // Dropping items back into the folder they came from is a no-op.
    const sources = paths.filter((p) => p !== dest && parentOf(p) !== dest);
    if (!sources.length) return;
    // Held at the last drag-over, or still held now: either counts.
    const now = await modifierKeys().catch(() => dragKeys);
    const keys = { ctrl: dragKeys.ctrl || now.ctrl, shift: dragKeys.shift || now.shift };
    dragKeys = { ctrl: false, shift: false };
    await transfer(sources, dest, copyOnDrop(keys, sources, dest));
  }

  // ---- Dragging items ----
  //
  // A drag starts inside the page, where it gets live pointer and key events: that is what
  // lets folders spring open, the list auto-scroll and Ctrl / Shift pick copy or move. A
  // native OS drag runs a modal loop that holds back the page's events, so it only takes
  // over once the pointer leaves the window, to drop into Explorer and other apps.

  /** The in-page drag in progress: what is dragged and where its label is drawn. */
  let inAppDrag = $state<{ paths: string[]; x: number; y: number } | null>(null);

  function dragOut(paths: string[]) {
    inAppDrag = { paths, x: -999, y: -999 };
    dragStarted(paths);
    window.addEventListener("pointermove", dragMove);
    window.addEventListener("pointerup", dragUp);
    window.addEventListener("pointercancel", dragCancel);
    window.addEventListener("keydown", dragKey, true);
    window.addEventListener("blur", dragCancel);
  }

  function stopInAppDrag() {
    window.removeEventListener("pointermove", dragMove);
    window.removeEventListener("pointerup", dragUp);
    window.removeEventListener("pointercancel", dragCancel);
    window.removeEventListener("keydown", dragKey, true);
    window.removeEventListener("blur", dragCancel);
    inAppDrag = null;
    shelfDrag = null;
  }

  function dragMove(e: PointerEvent) {
    if (!inAppDrag) return;
    dragKeys = { ctrl: e.ctrlKey || e.metaKey, shift: e.shiftKey };
    const outside =
      e.clientX < 0 || e.clientY < 0 || e.clientX >= innerWidth || e.clientY >= innerHeight;
    if (!(e.buttons & 1) || outside) {
      // Left the window with the button down: the OS takes the drag from here.
      const paths = inAppDrag.paths;
      stopInAppDrag();
      void dragEnded();
      if (e.buttons & 1) startDrag(paths).catch((err) => notify(String(err), true));
      return;
    }
    inAppDrag.x = e.clientX;
    inAppDrag.y = e.clientY;
    hoverAt({ x: e.clientX, y: e.clientY });
  }

  /** The click that follows a drag must not change the selection. */
  function swallowNextClick() {
    const stop = (e: Event) => e.stopPropagation();
    window.addEventListener("click", stop, true);
    setTimeout(() => window.removeEventListener("click", stop, true));
  }

  function dragUp(e: PointerEvent) {
    if (!inAppDrag) return;
    // Letting go outside the window is not a drop here.
    if (e.clientX < 0 || e.clientY < 0 || e.clientX >= innerWidth || e.clientY >= innerHeight) {
      dragCancel();
      return;
    }
    swallowNextClick();
    const { paths } = inAppDrag;
    dragKeys = { ctrl: e.ctrlKey || e.metaKey, shift: e.shiftKey };
    const onFolder = folderAt({
      x: e.clientX * devicePixelRatio,
      y: e.clientY * devicePixelRatio,
    });
    const fromShelf = shelfDrag;
    // Decided from where the button was released, not from the last move.
    const pin =
      !fromShelf &&
      pinZoneAt(document.elementFromPoint(e.clientX, e.clientY)) &&
      foldersOf(paths).length > 0;
    stopInAppDrag();
    void dragEnded();
    // A shelf item is pasted only where it was dropped on a folder, never just "here".
    if (fromShelf) {
      if (onFolder) pasteShelfItem(fromShelf, onFolder);
    } else if (pin) {
      pinFolders(foldersOf(paths));
    } else {
      const dest = onFolder ?? dropFallback();
      if (dest) dropPaths(paths, dest);
    }
  }

  function dragCancel() {
    shelfDrag = null;
    if (!inAppDrag) return;
    swallowNextClick();
    stopInAppDrag();
    void dragEnded();
  }

  function dragKey(e: KeyboardEvent) {
    if (!inAppDrag) return;
    // Nothing else acts on keys mid-drag.
    e.stopImmediatePropagation();
    if (e.key === "Escape") {
      e.preventDefault();
      dragCancel();
    } else if (e.key === " ") {
      e.preventDefault();
      spring.openNow();
    }
  }

  // ---- Tags ----

  let tagDefs = $state<Tag[]>([]);
  /** Tag ids of the tagged items in this folder, by item name. */
  let tagMap = $state<Record<string, number[]>>({});
  let tagsOpen = $state(false);

  // Nothing selected (it was moved or deleted meanwhile): nothing to tag.
  $effect(() => {
    if (tagsOpen && selection.size === 0) tagsOpen = false;
  });

  async function loadTagDefs() {
    tagDefs = await tagList().catch(() => tagDefs);
  }

  /** Which items of `dir` are tagged, by path. Empty if the index can't be read. */
  async function fetchTagMap(dir: string): Promise<Record<string, number[]>> {
    const found = await tagsInDir(dir).catch(() => []);
    return Object.fromEntries(found.map(([name, ids]) => [joinPath(dir, name), ids]));
  }

  const tagColors = (path: string) =>
    (tagMap[path] ?? [])
      .map((id) => tagDefs.find((t) => t.id === id))
      .filter((t): t is Tag => !!t)
      .map((t) => ({ name: t.name, color: tagColor(t.color) }));

  /** Tags of the sidebar's folders (Home and its subfolders), by path; kept apart from the listing. */
  let sideTags = $state<Record<string, number[]>>({});

  let sideSeq = 0;

  async function loadSideTags() {
    const seq = ++sideSeq;
    const up = parentOf(home);
    const [inHome, inUp] = await Promise.all([
      fetchTagMap(home),
      up ? fetchTagMap(up) : Promise.resolve({}),
    ]);
    // A newer read began meanwhile: it wins.
    if (seq === sideSeq) sideTags = { ...inUp, ...inHome };
  }

  // Tags changed (here or in the listing): re-read the sidebar's folders.
  $effect(() => {
    void tagDefs;
    void tagMap;
    if (home) void loadSideTags();
  });

  /** The colour of the first colour tag on the item, so a folder's icon matches its first dot. */
  const tintOf = (ids: number[] | undefined) => {
    for (const id of ids ?? []) {
      const color = tagDefs.find((t) => t.id === id)?.color;
      if (color) return tagColor(color);
    }
    return null;
  };

  /** Gives the tag to the selected items, or takes it from them if they all have it. */
  async function toggleTag(id: number) {
    const picked = selected();
    if (!picked.length) return;
    const done = await run(() =>
      tagToggle(
        picked.map((e) => e.path),
        id,
      ),
    );
    if (!done) return;
    await refresh();
    const name = tagDefs.find((t) => t.id === id)?.name ?? "tag";
    const count = picked.length - done.skipped;
    const left = done.skipped ? ` (${plural(done.skipped, "item")} can't be tagged)` : "";
    notify(
      (done.on ? `Tagged ${plural(count, "item")}: ${name}` : `Removed ${name}`) + left,
      done.skipped > 0,
    );
  }

  /** For the tag dialog: how many of the selected items have each tag. */
  let tagState = $derived.by(() => {
    const picked = selected();
    const out: Record<number, "all" | "some" | "none"> = {};
    for (const t of tagDefs) {
      const have = picked.filter((e) => tagMap[e.path]?.includes(t.id)).length;
      out[t.id] = have === 0 ? "none" : have === picked.length ? "all" : "some";
    }
    return out;
  });

  let bulkOpen = $state(false);

  // The selection changed under the dialog: it renames what it was opened with, so close it.
  $effect(() => {
    if (bulkOpen && selection.size < 2) bulkOpen = false;
  });

  /** Renames the batch in one undo step; resolves to an error message or null. */
  async function renameBatch(plan: RenamePlan[]): Promise<string | null> {
    try {
      const to = await renameMany(plan);
      notify(`Renamed ${plan.length} items`);
      await refresh(to);
      return null;
    } catch (e) {
      await refresh();
      return String(e);
    }
  }

  async function createTag(name: string, color: string | null): Promise<string | null> {
    try {
      await tagCreate(name, color);
      await loadTagDefs();
      return null;
    } catch (e) {
      return String(e);
    }
  }

  async function removeTag(id: number) {
    await run(() => tagDelete(id));
    await loadTagDefs();
    if (tagView?.id === id) await load(path);
    else await refresh();
  }

  /** The tags that can filter this folder: the ones on its items, plus any in use. */
  let chipTags = $derived(
    tagDefs.filter(
      (t) => tagFilter.includes(t.id) || Object.values(tagMap).some((ids) => ids.includes(t.id)),
    ),
  );

  function toggleFilter(id: number) {
    tagFilter = tagFilter.includes(id) ? tagFilter.filter((t) => t !== id) : [...tagFilter, id];
    selection.clear();
    anchor = cursor = -1;
  }

  /** Shows every item with `tag`, from any folder, in the list. */
  async function openTagView(tag: Tag) {
    const gen = navGen;
    const found = await run(() => tagFiles(tag.id));
    // Another navigation began while the items were being found: it wins.
    if (!found || gen !== navGen) return;
    navGen++;
    tagView = tag;
    tagFilter = [];
    entries = found.entries;
    tagMap = found.tags;
    selection.clear();
    anchor = cursor = -1;
    renaming = null;
    fileView?.scrollToTop();
  }

  // ---- Command palette (Ctrl+K) ----

  let paletteOpen = $state(false);
  let paletteItems = $state<PaletteItem[]>([]);

  /** Everything the palette can do right now: folders to go to, and actions. */
  function buildPalette(): PaletteItem[] {
    const places: Record<string, PaletteItem> = Object.create(null);
    const add = (p: string, recent?: number) => {
      if (!p || p === path || p in places) return;
      places[p] = {
        kind: "folder",
        label: folderName(p),
        detail: p,
        recent,
        run: () => navigate(p),
      };
    };
    loadRecent().forEach((p, i) => add(p, i));
    for (const sub of ["", "Desktop", "Documents", "Downloads", "Pictures"]) {
      add(sub ? joinPath(home, sub) : home);
    }
    // What is in the folder shown.
    for (const e of entries) if (e.isDir) add(e.path);

    const any = selected().length > 0;
    const one = selected().length === 1;
    const action = (label: string, detail: string | undefined, run: () => void, when = true) =>
      when ? [{ kind: "action" as const, label, detail, run }] : [];
    const actions: PaletteItem[] = [
      ...action("New folder", "Ctrl+Shift+N", newFolder, !tagView),
      ...action("New text file", "", () => newFile(), !tagView),
      ...action("New Markdown file", "", () => newFile("New file.md"), !tagView),
      ...action("Open in terminal", "", () => terminalIn(path), !tagView),
      ...action("Copy", "Ctrl+C", copySelection, any),
      ...action("Cut", "Ctrl+X", cutSelection, any),
      ...action("Paste", "Ctrl+V", paste, !tagView),
      ...action("Open the clipboard shelf", "Ctrl+Shift+V", () => (shelfOpen = true)),
      ...action("Open", "Enter", () => open(selected()[0]), one),
      ...action(
        "Open in new tab",
        undefined,
        () => openTab(selected()[0].path, true),
        one && selected()[0].isDir,
      ),
      ...action("Rename", "F2", () => (renaming = selected()[0]?.path ?? null), one),
      ...action("Rename selected items…", "F2", () => (bulkOpen = true), selected().length > 1),
      ...action("Move to Recycle Bin", "Del", trashSelected, any),
      ...action("Quick Look", "Space", openQuickLook, any),
      ...action("Tags…", undefined, () => (tagsOpen = true), any),
      ...tagDefs
        .filter((t) => t.id <= 7)
        .flatMap((t) => action(`Tag: ${t.name}`, String(t.id), () => toggleTag(t.id), any)),
      ...action(
        "More options (system menu)",
        undefined,
        () => nativeMenu(selected().map((e) => e.path)),
        any,
      ),
      ...action("Clear selection", "Esc", clearSelectionAndClipboard, any),
      ...action(
        "Select all",
        "Ctrl+A",
        () => selectRange(0, visible.length - 1),
        visible.length > 0,
      ),
      ...action("Undo", "Ctrl+Z", () => stepHistory(false)),
      ...action("Redo", "Ctrl+Y", () => stepHistory(true)),
      ...action("Back", "Alt+←", goBack, back.length > 0 || !!tagView),
      ...action("Forward", "Alt+→", goForward, forward.length > 0),
      ...action("Up one folder", "Alt+↑", goUp),
      ...action("Refresh", undefined, () => refresh()),
      ...action("Go to a path…", "Ctrl+L", () => toolbar?.edit(), !tagView),
      ...action("New tab", "Ctrl+T", () => openTab(startPath(home))),
      ...action("Close tab", "Ctrl+W", () => closeTab(active)),
      ...action(
        "Next tab",
        "Ctrl+Tab",
        () => activate((active + 1) % tabs.length),
        tabs.length > 1,
      ),
      ...action(
        "Previous tab",
        "Ctrl+Shift+Tab",
        () => activate((active - 1 + tabs.length) % tabs.length),
        tabs.length > 1,
      ),
      ...tabs.flatMap((t, i) =>
        action(
          `Switch to tab: ${folderName(t.path)}`,
          i < 9 ? `Ctrl+${i + 1}` : undefined,
          () => activate(i),
          i !== active,
        ),
      ),
      ...action("Clear recent folders", undefined, clearRecent),
      ...action("Grid view", undefined, () => setView("grid"), view !== "grid"),
      ...action("List view", undefined, () => setView("list"), view !== "list"),
      ...action(settings.showHidden ? "Hide hidden files" : "Show hidden files", "Ctrl+H", () =>
        updateSettings({ showHidden: !settings.showHidden }),
      ),
      ...action(
        settings.showInspector ? "Hide the inspector" : "Show the inspector",
        "Ctrl+I",
        () => updateSettings({ showInspector: !settings.showInspector }),
      ),
      ...action("Light theme", undefined, () => chooseTheme("light"), theme !== "light"),
      ...action("Dark theme", undefined, () => chooseTheme("dark"), theme !== "dark"),
      ...action(
        "Follow the system theme",
        undefined,
        () => chooseTheme("system"),
        theme !== "system",
      ),
      ...action("Settings", "Ctrl+,", () => (settingsOpen = true)),
      ...action("Recycle Bin", "", () => (binOpen = true)),
      ...action("Storage breakdown", "", () => (storageOpen = true)),
      ...action("Clean up large and old files", "", () => (cleanupOpen = true)),
      ...action("Scan this folder for sizes", "", startScan, !scanning && !tagView),
      ...action("Cancel the size scan", "", () => scanCancel(), scanning),
    ];
    return [...Object.values(places), ...actions];
  }

  function openPalette() {
    paletteItems = buildPalette();
    paletteOpen = true;
  }

  function chooseTheme(t: Theme) {
    theme = t;
    setTheme(t);
  }

  // ---- Quick Look ----

  /** The file being previewed, by path so a refresh doesn't change what is shown. */
  let look = $state<string | null>(null);
  let lookIndex = $derived(look ? visible.findIndex((e) => e.path === look) : -1);

  // The file went away (deleted, or another folder opened): nothing left to preview.
  $effect(() => {
    if (look && lookIndex < 0) look = null;
  });

  function openQuickLook() {
    const at =
      cursor >= 0 && selection.has(visible[cursor]?.path)
        ? cursor
        : visible.findIndex((e) => selection.has(e.path));
    if (at >= 0) look = visible[at].path;
  }

  /** Moves the preview (and the selection with it) to the next or previous item. */
  function lookStep(delta: number) {
    const to = Math.min(visible.length - 1, Math.max(0, lookIndex + delta));
    if (to === lookIndex) return;
    selectOnly(to);
    fileView.scrollToIndex(to);
    look = visible[to].path;
  }

  function lookKey(e: KeyboardEvent) {
    if (e.ctrlKey || e.metaKey || e.altKey) return;
    // Holding Space (the key that opened it) must not close it again straight away.
    if (e.repeat && (e.key === " " || e.key === "Escape")) {
      e.preventDefault();
      return;
    }
    const step =
      e.key === "ArrowLeft" || e.key === "ArrowUp"
        ? -1
        : e.key === "ArrowRight" || e.key === "ArrowDown"
          ? 1
          : 0;
    if (e.key === " " || e.key === "Escape") look = null;
    else if (step) lookStep(step);
    else return;
    e.preventDefault();
  }

  // ---- Context menu ----

  /** Folio's own menu, while one is open. */
  let menu = $state<{ items: MenuItem[]; x: number; y: number } | null>(null);

  /** The OS menu for `paths`: everything the shell offers, e.g. "Open with". */
  function nativeMenu(paths: string[], extended = false) {
    showContextMenu(paths, extended).catch((err) => notify(String(err), true));
  }

  function itemContextMenu(index: number, e: MouseEvent) {
    if (!selection.has(visible[index].path)) selectOnly(index);
    // Shift+right-click goes straight to the OS menu, as in Explorer.
    if (e.shiftKey) nativeMenu([...selection], true);
    else menu = { items: itemMenu(), x: e.clientX, y: e.clientY };
  }

  function backgroundContextMenu(e: MouseEvent) {
    selection.clear();
    anchor = cursor = -1;
    // A tag's items are in many folders: there is no folder to act on.
    if (tagView) return;
    if (e.shiftKey) nativeMenu([path], true);
    else menu = { items: backgroundMenu(), x: e.clientX, y: e.clientY };
  }

  function itemMenu(): MenuItem[] {
    const picked = selected();
    const one = picked.length === 1 ? picked[0] : null;
    return [
      ...(one
        ? [
            { label: "Open", hint: "Enter", run: () => open(one) },
            ...(one.isDir
              ? []
              : [{ label: "Quick Look", hint: "Space", run: () => (look = one.path) }]),
            ...(one.isDir
              ? [
                  { label: "Open in new tab", run: () => openTab(one.path, true) },
                  { label: "Open in terminal", run: () => terminalIn(one.path) },
                ]
              : []),
            "separator" as const,
          ]
        : []),
      { label: "Cut", hint: "Ctrl+X", run: cutSelection },
      { label: "Copy", hint: "Ctrl+C", run: copySelection },
      ...(one ? [{ label: "Rename", hint: "F2", run: () => (renaming = one.path) }] : []),
      ...(picked.length > 1
        ? [{ label: "Rename…", hint: "F2", run: () => (bulkOpen = true) }]
        : []),
      { label: "Tags…", run: () => (tagsOpen = true) },
      { label: "Move to Recycle Bin", hint: "Del", danger: true, run: trashSelected },
      "separator",
      { label: "More options", run: () => nativeMenu([...selection]) },
    ];
  }

  function backgroundMenu(): MenuItem[] {
    return [
      { label: "New folder", hint: "Ctrl+Shift+N", run: newFolder },
      { label: "New text file", run: () => newFile() },
      { label: "New Markdown file", run: () => newFile("New file.md") },
      { label: "Paste", hint: "Ctrl+V", run: paste },
      "separator",
      { label: "Refresh", run: () => refresh() },
      { label: "Select all", hint: "Ctrl+A", run: () => selectRange(0, visible.length - 1) },
      "separator",
      { label: "Open in terminal", run: () => terminalIn(path) },
      { label: "More options", run: () => nativeMenu([path]) },
    ];
  }

  /** Suppress the webview's own menu everywhere except text fields. */
  function oncontextmenu(e: MouseEvent) {
    if ((e.target as HTMLElement).tagName !== "INPUT") e.preventDefault();
  }

  // ---- Benchmarks (opt-in, see docs/PERFORMANCE.md) ----

  async function runBench(dirs: string[]) {
    const viewBefore = tabs[active]?.view ?? null;
    for (const dir of dirs) {
      let t = performance.now();
      const count = (await listDir(dir)).length;
      const listMs = (performance.now() - t).toFixed(1);
      for (const v of ["grid", "list"] as View[]) {
        await navigate(home);
        setView(v);
        await nextPaint();
        t = performance.now();
        await navigate(dir);
        await nextPaint();
        const openMs = (performance.now() - t).toFixed(1);
        const el = fileView.scrollElement();
        const scroll = await measureScroll(el);
        const jumpMs = await measureJumps(el);
        await perfReport(
          `bench\titems=${count}\tview=${v}\tlistMs=${listMs}\topenMs=${openMs}\tavgFps=${scroll.avgFps}` +
            `\tp95Ms=${scroll.p95Ms}\tmaxMs=${scroll.maxMs}\tslow=${scroll.slow}/${scroll.frames}\tjumpMaxMs=${jumpMs}`,
        );
      }
    }
    if (tabs[active]) tabs[active].view = viewBefore;
    await navigate(home);
    await nextPaint();
    await perfReport("bench-done");
  }

  // ---- Keyboard ----

  type Action = () => unknown;
  const ignore: Action = () => {};

  function goUp() {
    if (tagView) {
      load(path);
      return;
    }
    const up = parentOf(path);
    if (up) navigate(up);
  }

  function copySelection() {
    shelfMark("copy");
  }

  function cutSelection() {
    shelfMark("cut");
  }

  /** Esc clears the selection; the shelf keeps its items. */
  function clearSelectionAndClipboard() {
    selection.clear();
  }

  /** The menu key and Shift+F10: opens at the selected item, else the folder background. */
  async function keyboardContextMenu(shift: boolean) {
    if (!selection.size) {
      const pane = document.querySelector(".scroller")?.getBoundingClientRect();
      menu = { items: backgroundMenu(), x: (pane?.left ?? 0) + 16, y: (pane?.top ?? 0) + 16 };
      return;
    }
    if (shift) {
      nativeMenu([...selection], true);
      return;
    }
    // Open at the cursor row; the list is virtualized, so bring it into view first.
    const row = cursor >= 0 && selection.has(visible[cursor]?.path) ? cursor : -1;
    if (row >= 0) {
      fileView.scrollToIndex(row);
      await tick();
    }
    const at = document.querySelector<HTMLElement>("[role=option][aria-selected=true]");
    const box = at?.getBoundingClientRect();
    menu = { items: itemMenu(), x: (box?.left ?? 0) + 16, y: (box?.bottom ?? 0) - 8 };
  }

  /** Tab shortcuts. They work everywhere, including while a text field has focus. */
  function tabShortcut(e: KeyboardEvent, ctrl: boolean): Action | null {
    if (!ctrl) return null;
    const key = e.key.toLowerCase();
    if (key === "," && !e.shiftKey) return () => (settingsOpen = true);
    if (key === "i" && !e.shiftKey && !e.altKey)
      return () => updateSettings({ showInspector: !settings.showInspector });
    if (key === "k" && !e.shiftKey && !e.altKey)
      return renaming || (e.target as HTMLElement).tagName === "INPUT" ? ignore : openPalette;
    if (key === "l" && !e.shiftKey) return () => !tagView && toolbar?.edit();
    if (key === "t" || key === "w") {
      // Held-down Ctrl+T / Ctrl+W must not open or close tabs in a burst.
      if (!restored || e.repeat) return ignore;
      return key === "t" ? () => openTab(startPath(home)) : () => closeTab(active);
    }
    if (e.key === "Tab") return () => cycleTab(e.shiftKey ? -1 : 1);
    if (e.key === "PageDown") return () => cycleTab(1);
    if (e.key === "PageUp") return () => cycleTab(-1);
    if (!e.shiftKey && e.key >= "1" && e.key <= "9") {
      // Ctrl+9 is always the last tab, as in browsers.
      const last = tabs.length - 1;
      return () => activate(e.key === "9" ? last : Math.min(Number(e.key) - 1, last));
    }
    return null;
  }

  /** File list shortcuts. */
  function fileShortcut(e: KeyboardEvent, ctrl: boolean): Action | null {
    const { key, shiftKey: shift } = e;
    // Other Alt combinations fall through and act like the plain key (Alt+Backspace goes up).
    if (e.altKey) {
      if (key === "ArrowLeft") return goBack;
      if (key === "ArrowRight") return goForward;
      if (key === "ArrowUp") return goUp;
    }
    if (ctrl && shift && key.toLowerCase() === "n") return newFolder;
    // Paste a few of the shelf's items: open the panel to pick them.
    if (ctrl && shift && key.toLowerCase() === "v") return () => (shelfOpen = !shelfOpen);
    // Holding the key must not undo one operation after another.
    if (ctrl && key.toLowerCase() === "z") return e.repeat ? null : () => stepHistory(shift);
    if (ctrl && !shift && key.toLowerCase() === "y")
      return e.repeat ? null : () => stepHistory(true);
    if (ctrl) {
      // Exact lowercase match: Ctrl+Shift+letter combinations are left alone.
      switch (key) {
        case "a":
          return () => selectRange(0, visible.length - 1);
        case "c":
          if (selection.size) return copySelection;
          break;
        case "x":
          if (selection.size) return cutSelection;
          break;
        case "v":
          return paste;
        case "h":
          return () => updateSettings({ showHidden: !settings.showHidden });
      }
    }
    const row = view === "grid" ? fileView.columns() : 1;
    const grid = view === "grid";
    switch (key) {
      case "Backspace":
        return goUp;
      case "ArrowDown":
        return () => moveCursor(row, shift);
      case "ArrowUp":
        return () => moveCursor(-row, shift);
      case "ArrowRight":
        return grid ? () => moveCursor(1, shift) : null;
      case "ArrowLeft":
        return grid ? () => moveCursor(-1, shift) : null;
      case "Home":
        return () => moveCursor(-visible.length, shift);
      case "End":
        return () => moveCursor(visible.length, shift);
      case "Enter":
        return cursor >= 0 ? () => open(visible[cursor]) : null;
      case " ":
        return selection.size ? openQuickLook : null;
      case "1":
      case "2":
      case "3":
      case "4":
      case "5":
      case "6":
      case "7":
        // The colour tags, in the order of their keys.
        // Held down, it must not flip the tag on and off.
        if (shift || e.altKey || !selection.size) return null;
        return e.repeat ? ignore : () => toggleTag(Number(key));
      case "F2":
        if (selection.size > 1 && !tagView) return () => (bulkOpen = true);
        return cursor >= 0 ? () => (renaming = visible[cursor].path) : null;
      case "Delete":
        return trashSelected;
      case "F10":
        return shift ? () => keyboardContextMenu(false) : null;
      case "ContextMenu":
        return () => keyboardContextMenu(shift);
      case "Escape":
        return clearSelectionAndClipboard;
    }
    return null;
  }

  function onkeydown(e: KeyboardEvent) {
    // A dialog handles its own keys; nothing behind it may act on them.
    if (
      asking ||
      menu ||
      settingsOpen ||
      askCrash ||
      storageOpen ||
      cleanupOpen ||
      binOpen ||
      tagsOpen ||
      bulkOpen ||
      paletteOpen
    )
      return;
    if (look) {
      // Quick Look owns the keyboard, but the audio and video controls keep Space and the
      // arrows (play, seek, volume).
      if (!(e.target as HTMLElement).closest?.("audio, video")) lookKey(e);
      else if (e.key === "Escape") lookKey(e);
      return;
    }
    const ctrl = e.ctrlKey || e.metaKey;
    const target = e.target as HTMLElement;
    // File shortcuts don't apply while renaming or typing, or on a keyboard-focused tab
    // (it handles its own keys).
    const fileKeys = !renaming && target.tagName !== "INPUT" && !target.closest?.("[role=tablist]");
    const action = tabShortcut(e, ctrl) ?? (fileKeys ? fileShortcut(e, ctrl) : null);
    if (!action) return;
    e.preventDefault();
    action();
  }

  onMount(() => {
    // Another window changed the theme: follow it.
    const stopTheme = onThemeChange((t) => {
      theme = t;
      applyTheme(t);
    });
    const unlistenQueue = onQueueUpdate(onJobUpdate);
    const unlistenScan = onScanDone((d) => {
      scanning = false;
      loadFolderSizes(path);
      if (d.error) notify(`Scan failed: ${d.error}`, true);
      else if (d.cancelled) notify("Scan cancelled");
      else notify(`Scanned ${plural(d.files, "file")}, ${formatSize(d.bytes)}`);
    });
    const unlistenBlocked = onCloseBlocked(() =>
      notify("Operations are still running. Wait for them, or cancel them, before closing.", true),
    );
    const unlisten = onDirChanged((changed) => {
      if (changed === path && !renaming) refresh();
    });
    // Flush pending tab changes before any close (button, Ctrl+W, Alt+F4, taskbar).
    const unlistenClose = getCurrentWindow().onCloseRequested(() => persistTabs());
    const unlistenDrop = getCurrentWebview().onDragDropEvent(({ payload }) => {
      const at = {
        x: "position" in payload ? payload.position.x / devicePixelRatio : 0,
        y: "position" in payload ? payload.position.y / devicePixelRatio : 0,
      };
      if (payload.type === "enter") {
        dragStarted(payload.paths);
        hoverAt(at);
      } else if (payload.type === "over") {
        hoverAt(at);
      } else if (payload.type === "drop") {
        const dest = folderAt(payload.position) ?? dropFallback();
        // Settle the trip first: the drop's own transfer then lands in the folder shown.
        dragEnded();
        if (dest) dropPaths(payload.paths, dest);
      } else dragEnded();
    });
    loadTagDefs();
    loadShelf();
    scanRunning().then(
      (running) => (scanning = running),
      () => {},
    );
    if (getCurrentWindow().label === "main") {
      crashState().then(
        (s) => (askCrash = s.consent === null),
        () => {},
      );
    }
    const onFocus = () => loadShelf();
    window.addEventListener("focus", onFocus);
    homeDir().then(async (h) => {
      home = h;
      const saved = await windowTabs().catch(() => null);
      tabs = saved
        ? saved.tabs.map((p, i) => newTab(p, asView(saved.views?.[i])))
        : [newTab(startPath(h))];
      await activate(saved?.active ?? 0);
      restored = true;
      // Benchmarks are optional; never let them break startup.
      const perf = await perfConfig().catch(() => null);
      if (perf?.enabled) {
        await nextPaint();
        // webviewMs: time since the page started loading, to split host vs webview startup.
        await perfReport(`first-paint\twebviewMs=${performance.now().toFixed(0)}`);
        if (perf.benchDirs.length) await runBench(perf.benchDirs);
      }
    });
    return () => {
      stopTheme();
      window.removeEventListener("focus", onFocus);
      unlisten.then((u) => u());
      unlistenQueue.then((u) => u());
      unlistenScan.then((u) => u());
      unlistenBlocked.then((u) => u());
      unlistenDrop.then((u) => u());
      unlistenClose.then((u) => u());
    };
  });
</script>

<svelte:window {onkeydown} {oncontextmenu} />

<div class="app">
  <Sidebar
    {home}
    current={tagView ? "" : path}
    onnavigate={navigate}
    onopentab={(p) => openTab(p, true)}
    {dropTarget}
    {springPath}
    {springMs}
    tags={tagDefs}
    activeTag={tagView?.id ?? null}
    ontag={openTagView}
    onbin={() => (binOpen = true)}
    folderTint={(p) => tintOf(sideTags[p])}
    {pinHover}
    onpinmenu={(p, e) => {
      const list = settings.pinnedFolders;
      const i = list.indexOf(p);
      menu = {
        x: e.clientX,
        y: e.clientY,
        items: [
          { label: "Move up", disabled: i <= 0, run: () => movePinned(p, -1) },
          {
            label: "Move down",
            disabled: i < 0 || i >= list.length - 1,
            run: () => movePinned(p, 1),
          },
          "separator",
          { label: "Unpin from Favorites", run: () => unpinFolder(p) },
        ],
      };
    }}
  />
  <main class="main">
    <TabBar
      tabs={tabList}
      {active}
      onselect={activate}
      onclose={closeTab}
      onnew={() => openTab(startPath(home))}
      {springPath}
      {springMs}
      onmove={moveTab}
      ondetach={detachTab}
    />
    <Toolbar
      bind:this={toolbar}
      {path}
      {view}
      onview={setView}
      canBack={back.length > 0}
      canForward={forward.length > 0}
      onback={goBack}
      onforward={goForward}
      onnavigate={navigate}
      {dropTarget}
      {springPath}
      {springMs}
      onsettings={() => (settingsOpen = true)}
      tagLabel={tagView?.name ?? null}
      inspectorOpen={settings.showInspector}
      oninspector={() => updateSettings({ showInspector: !settings.showInspector })}
    />
    {#if !tagView && (chipTags.length || tagFilter.length)}
      <TagChips
        tags={chipTags}
        active={tagFilter}
        ontoggle={toggleFilter}
        onclear={() => (tagFilter = [])}
      />
    {/if}
    <FileView
      bind:this={fileView}
      entries={visible}
      {view}
      {selection}
      cut={cutSet}
      {renaming}
      onitemclick={itemClick}
      onbackgroundclick={() => selectOnly(-1)}
      onopen={open}
      onrename={commitRename}
      oncancelrename={() => (renaming = null)}
      ondragout={dragOut}
      onitemcontextmenu={itemContextMenu}
      onbackgroundcontextmenu={backgroundContextMenu}
      onopentab={(p) => openTab(p, true)}
      {dropTarget}
      {springPath}
      {springMs}
      {tagColors}
      folderTint={(p) => tintOf(tagMap[p])}
      onband={bandSelect}
      {sort}
      onsort={sortBy}
      folderSize={(p) => folderSizes[sizeKey(p)]}
    />
    <footer class="status">
      <span
        >{plural(visible.length, "item")}{selection.size
          ? ` · ${selection.size} selected`
          : ""}</span
      >
      <OpsPill
        {jobs}
        onpause={queuePause}
        onresume={queueResume}
        oncancel={queueCancel}
        ondismiss={dismissJob}
      />
    </footer>
  </main>

  {#if settings.showInspector}
    <Inspector
      folder={tagView ? null : path}
      entries={visible}
      picked={selected()}
      tags={tagDefs}
      have={tagState}
      ontoggle={toggleTag}
    />
  {/if}

  <ShelfPanel
    items={shelf}
    open={shelfOpen}
    ontoggle={() => (shelfOpen = !shelfOpen)}
    onpaste={pasteItems}
    onremove={shelfRemoveItems}
    onmode={async (item, mode) => {
      await run(() => shelfSetMode(item.path, mode));
      await loadShelf();
    }}
    onclear={async () => {
      await run(() => shelfClear());
      await loadShelf();
    }}
    ondrag={dragShelfItem}
  />

  {#if paletteOpen}
    <Palette items={paletteItems} onclose={() => (paletteOpen = false)} />
  {/if}

  {#if tagsOpen}
    <TagDialog
      tags={tagDefs}
      have={tagState}
      count={selected().length}
      ontoggle={toggleTag}
      oncreate={createTag}
      ondelete={removeTag}
      onclose={() => (tagsOpen = false)}
    />
  {/if}

  {#if bulkOpen}
    <BulkRenameDialog items={selected()} onapply={renameBatch} onclose={() => (bulkOpen = false)} />
  {/if}

  {#if binOpen}
    <BinView onclose={() => (binOpen = false)} onrestored={() => refresh()} />
  {/if}

  {#if cleanupOpen}
    <CleanupView
      onclose={() => (cleanupOpen = false)}
      onopen={(p) => {
        cleanupOpen = false;
        navigate(p);
      }}
    />
  {/if}

  {#if storageOpen}
    <StorageView
      start={path}
      onclose={() => (storageOpen = false)}
      onopen={(p) => {
        storageOpen = false;
        navigate(p);
      }}
    />
  {/if}

  {#if askCrash}
    <CrashPrompt
      ondismiss={() => (askCrash = false)}
      onanswer={(on) => {
        askCrash = false;
        crashSet(on).catch(() => {});
      }}
    />
  {/if}

  {#if settingsOpen}
    <SettingsDialog
      {theme}
      ontheme={(t) => {
        theme = t;
        setTheme(t);
      }}
      onclose={() => (settingsOpen = false)}
    />
  {/if}

  {#if look && lookIndex >= 0}
    <QuickLook
      entry={visible[lookIndex]}
      position={lookIndex + 1}
      total={visible.length}
      onopen={() => open(visible[lookIndex])}
      onclose={() => (look = null)}
    />
  {/if}

  {#if inAppDrag}
    <div class="ghost" style:left="{inAppDrag.x + 14}px" style:top="{inAppDrag.y + 14}px">
      {plural(inAppDrag.paths.length, "item")}
    </div>
  {/if}

  {#if menu}
    <ContextMenu items={menu.items} x={menu.x} y={menu.y} onclose={() => (menu = null)} />
  {/if}

  {#if asking}
    <!-- Keyed, so each conflict gets a fresh dialog with focus on Keep both. -->
    {#key asking.conflict.source}
      <ConflictDialog
        conflict={asking.conflict}
        remaining={asking.remaining}
        verb={asking.verb}
        ondecide={(choice, all) => answer?.(choice, all)}
        oncancel={() => answer?.("cancel", false)}
      />
    {/key}
  {/if}

  {#if toast}
    <div class="toast" class:error={toast.error} role="status">{toast.text}</div>
  {/if}
</div>

<style>
  .app {
    display: flex;
    height: 100vh;
  }

  .main {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
    background: var(--bg-window);
  }

  .status {
    display: flex;
    align-items: center;
    justify-content: space-between;
    height: 30px;
    flex-shrink: 0;
    padding: 0 16px;
    border-top: 1px solid var(--border-default);
    font-size: 11.5px;
    font-weight: var(--font-weight-medium);
    color: var(--text-secondary);
  }

  .ghost {
    position: fixed;
    z-index: 70;
    padding: var(--space-2xs) var(--space-sm);
    border-radius: var(--radius-md);
    background: var(--accent-default);
    color: var(--text-on-accent);
    box-shadow: var(--shadow-popover);
    font-weight: var(--font-weight-medium);
    pointer-events: none;
  }

  .toast {
    position: fixed;
    left: 50%;
    bottom: 44px;
    transform: translateX(-50%);
    max-width: 560px;
    padding: 9px 14px;
    border-radius: var(--radius-lg);
    background: var(--text-primary);
    color: var(--bg-window);
    box-shadow: var(--shadow-popover);
    font-weight: var(--font-weight-medium);
  }

  .toast.error {
    background: var(--status-danger);
    color: #fff;
  }
</style>
