<script lang="ts">
  import { onMount, tick } from "svelte";
  import { SvelteSet } from "svelte/reactivity";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import {
    copyPaths,
    createFolder,
    findConflicts,
    homeDir,
    listDir,
    movePaths,
    onDirChanged,
    openPath,
    onCloseBlocked,
    onQueueUpdate,
    openWindow,
    parentOf,
    pathSegments,
    queueCancel,
    queueDismiss,
    queuePause,
    queueResume,
    renamePath,
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
  import ConflictDialog from "$lib/components/ConflictDialog.svelte";
  import ContextMenu, { type MenuItem } from "$lib/components/ContextMenu.svelte";
  import OpsPill from "$lib/components/OpsPill.svelte";
  import Sidebar from "$lib/components/Sidebar.svelte";
  import TabBar from "$lib/components/TabBar.svelte";
  import Toolbar, { type View } from "$lib/components/Toolbar.svelte";
  import { createSpring, loadSpring } from "$lib/spring";
  import {
    applyTheme,
    loadTheme,
    nextTheme,
    onThemeChange,
    setTheme,
    type Theme,
  } from "$lib/theme";
  import FileView from "$lib/components/FileView.svelte";
  import { measureJumps, measureScroll, nextPaint, perfConfig, perfReport } from "$lib/perf";

  let home = $state("");
  let path = $state("");
  // Raw (not deeply proxied): listings are replaced wholesale, and proxying 50k objects is slow.
  let entries = $state.raw<Entry[]>([]);
  let view = $state<View>("grid");
  let theme = $state<Theme>(loadTheme());
  let showHidden = $state(false);

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
  }
  interface TabMemory {
    back: string[];
    forward: string[];
    selection: string[];
    scrollTop: number;
  }
  let tabs = $state<Tab[]>([]);
  let active = $state(-1);
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

  const folderName = (p: string) => pathSegments(p).at(-1)?.name ?? p;
  let tabPaths = $derived(tabs.map((t) => t.path));
  let tabList = $derived(tabs.map((t, i) => ({ id: t.id, title: folderName(tabPaths[i]) })));

  const selection = new SvelteSet<string>();
  let anchor = $state(-1);
  let cursor = $state(-1);
  let renaming = $state<string | null>(null);

  let clipboard = $state<{ mode: "copy" | "cut"; paths: string[] } | null>(null);
  let cutSet = $derived(new Set(clipboard?.mode === "cut" ? clipboard.paths : []));

  /** Folder under a native drag (from Folio itself or another app). */
  let dropTarget = $state<string | null>(null);

  let toast = $state<{ text: string; error: boolean } | null>(null);
  let toastTimer: ReturnType<typeof setTimeout>;

  let fileView: FileView;
  let toolbar: Toolbar | undefined = $state();

  let visible = $derived(showHidden ? entries : entries.filter((e) => !e.hidden));

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
    try {
      list = await listDir(path);
    } catch (e) {
      notify(String(e), true);
      return;
    }
    if (gen !== navGen) return; // the tab switched or navigated meanwhile
    entries = list;
    const existing = new Set(entries.map((e) => e.path));
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
    try {
      list = await listDir(target);
    } catch (e) {
      if (gen !== navGen) return null;
      if (!quiet) notify(String(e), true);
      return false;
    }
    if (gen !== navGen) return null;
    entries = list;
    path = target;
    if (tabs[active]) {
      tabs[active].path = target;
      shownTabId = tabs[active].id;
    }
    selection.clear();
    anchor = cursor = -1;
    renaming = null;
    fileView?.scrollToTop();
    watchDir(target).catch((e) => console.warn("watch failed", e));
    return true;
  }

  /** Opens `target`; resolves to whether it is now shown (null: a newer navigation won). */
  async function navigate(target: string, quiet = false): Promise<boolean | null> {
    if (target === path) return true;
    const from = path;
    const ok = await load(target, quiet);
    if (ok) {
      if (from) back.push(from);
      forward = [];
    }
    return ok;
  }

  async function goBack() {
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

  const newTab = (p: string): Tab => ({ id: nextTabId++, path: p });

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
      await openWindow([target], x, y);
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

  function persistTabs(paths = tabPaths, index = active) {
    clearTimeout(saveTimer);
    return saveTabs(paths, Math.max(index, 0)).catch((e) => console.warn("saving tabs failed", e));
  }

  /** Closing goes through the onCloseRequested handler in onMount, which saves first. */
  const closeWindow = () => getCurrentWindow().close();

  // Save the tab list shortly after it settles.
  $effect(() => {
    if (!restored) return;
    const paths = tabPaths;
    const index = active;
    clearTimeout(saveTimer);
    saveTimer = setTimeout(() => persistTabs(paths, index), 300);
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
    const created = await run(() => createFolder(path, "New folder"));
    if (created) {
      await refresh([created]);
      renaming = created;
    }
  }

  async function commitRename(entry: Entry, name: string) {
    if (renaming !== entry.path) return;
    renaming = null;
    const trimmed = name.trim();
    if (!trimmed || trimmed === entry.name) return;
    const renamed = await run(() => renamePath(entry.path, trimmed));
    if (renamed) await refresh([renamed]);
  }

  async function transfer(paths: string[], dest: string, copy: boolean) {
    // One question at a time.
    if (asking) return;
    const conflicts = await run(() => findConflicts(paths, dest));
    if (!conflicts) return;
    let sources = paths;
    let replace: string[] = [];
    let replaceIds: (string | null)[] = [];
    if (conflicts.length) {
      const choices = await askAboutConflicts(conflicts, copy ? "copy" : "move");
      if (!choices) return;
      sources = paths.filter((p) => choices[p] !== "skip");
      replace = sources.filter((p) => choices[p] === "replace");
      replaceIds = replace.map((p) => conflicts.find((c) => c.source === p)?.existingId ?? null);
      if (!sources.length) return;
    }
    await run(() =>
      copy
        ? copyPaths(sources, dest, replace, replaceIds)
        : movePaths(sources, dest, replace, replaceIds),
    );
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

  async function paste() {
    if (!clipboard) return;
    const { mode, paths } = clipboard;
    if (mode === "cut") clipboard = null;
    await transfer(paths, path, mode === "copy");
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

  const springSettings = loadSpring();
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

  const spring = createSpring({
    delayMs: () => springSettings.delayMs,
    eligible: (target) => target !== path && !dragPaths.includes(target),
    open: springOpen,
    ring: (target, ms) => {
      springPath = target;
      springMs = ms;
    },
  });

  /** Counts drags, so work that outlives its drag can tell. */
  let dragId = 0;

  async function springOpen(target: string) {
    const from = path;
    const id = dragId;
    if (await load(target, true)) {
      if (id === dragId) {
        trip ??= { from };
      } else if (!springSettings.returnAfterDrop) {
        // The drag ended while this was loading: its trip is over, so record it now.
        back.push(from);
        forward = [];
      }
      // The list under the pointer changed: whatever is there now starts its own countdown.
      await nextPaint();
      if (id === dragId && dragPos) hoverAt(dragPos);
    }
  }

  function hoverAt(pos: { x: number; y: number }) {
    dragPos = pos;
    dropTarget = folderAt({ x: pos.x * devicePixelRatio, y: pos.y * devicePixelRatio });
    spring.hover(dropTarget);
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
    dragPos = null;
    dragPaths = [];
    spring.stop();
    dropTarget = null;
    const started = trip;
    trip = null;
    if (!started) return;
    if (springSettings.returnAfterDrop) {
      await load(started.from, true);
    } else if (path !== started.from) {
      // The whole trip is one step back.
      back.push(started.from);
      forward = [];
    }
  }

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
    const dest =
      folderAt({ x: e.clientX * devicePixelRatio, y: e.clientY * devicePixelRatio }) ?? path;
    stopInAppDrag();
    void dragEnded();
    dropPaths(paths, dest);
  }

  function dragCancel() {
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
              ? [{ label: "Open in new tab", run: () => openTab(one.path, true) }]
              : []),
            "separator" as const,
          ]
        : []),
      { label: "Cut", hint: "Ctrl+X", run: cutSelection },
      { label: "Copy", hint: "Ctrl+C", run: copySelection },
      ...(one ? [{ label: "Rename", hint: "F2", run: () => (renaming = one.path) }] : []),
      { label: "Move to Recycle Bin", hint: "Del", danger: true, run: trashSelected },
      "separator",
      { label: "More options", run: () => nativeMenu([...selection]) },
    ];
  }

  function backgroundMenu(): MenuItem[] {
    return [
      { label: "New folder", hint: "Ctrl+Shift+N", run: newFolder },
      { label: "Paste", hint: "Ctrl+V", disabled: !clipboard, run: paste },
      "separator",
      { label: "Refresh", run: () => refresh() },
      { label: "Select all", hint: "Ctrl+A", run: () => selectRange(0, visible.length - 1) },
      "separator",
      { label: "More options", run: () => nativeMenu([path]) },
    ];
  }

  /** Suppress the webview's own menu everywhere except text fields. */
  function oncontextmenu(e: MouseEvent) {
    if ((e.target as HTMLElement).tagName !== "INPUT") e.preventDefault();
  }

  // ---- Benchmarks (opt-in, see docs/PERFORMANCE.md) ----

  async function runBench(dirs: string[]) {
    for (const dir of dirs) {
      let t = performance.now();
      const count = (await listDir(dir)).length;
      const listMs = (performance.now() - t).toFixed(1);
      for (const v of ["grid", "list"] as View[]) {
        await navigate(home);
        view = v;
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
    view = "grid";
    await navigate(home);
    await nextPaint();
    await perfReport("bench-done");
  }

  // ---- Keyboard ----

  type Action = () => unknown;
  const ignore: Action = () => {};

  function goUp() {
    const up = parentOf(path);
    if (up) navigate(up);
  }

  function copySelection() {
    clipboard = { mode: "copy", paths: [...selection] };
    notify(`Copied ${plural(selection.size, "item")} to clipboard`);
  }

  function cutSelection() {
    clipboard = { mode: "cut", paths: [...selection] };
  }

  function clearSelectionAndClipboard() {
    selection.clear();
    clipboard = null;
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
    if (key === "l" && !e.shiftKey) return () => toolbar?.edit();
    if (key === "t" || key === "w") {
      // Held-down Ctrl+T / Ctrl+W must not open or close tabs in a burst.
      if (!restored || e.repeat) return ignore;
      return key === "t" ? () => openTab(home) : () => closeTab(active);
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
          return () => (showHidden = !showHidden);
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
      case "F2":
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
    if (asking || menu) return;
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
        const dest = folderAt(payload.position) ?? path;
        // Settle the trip first: the drop's own transfer then lands in the folder shown.
        dragEnded();
        dropPaths(payload.paths, dest);
      } else dragEnded();
    });
    homeDir().then(async (h) => {
      home = h;
      const saved = await windowTabs().catch(() => null);
      tabs = saved ? saved.tabs.map(newTab) : [newTab(h)];
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
      unlisten.then((u) => u());
      unlistenQueue.then((u) => u());
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
    current={path}
    onnavigate={navigate}
    onopentab={(p) => openTab(p, true)}
    {dropTarget}
    {springPath}
    {springMs}
  />
  <main class="main">
    <TabBar
      tabs={tabList}
      {active}
      onselect={activate}
      onclose={closeTab}
      onnew={() => openTab(home)}
      onmove={moveTab}
      ondetach={detachTab}
    />
    <Toolbar
      bind:this={toolbar}
      {path}
      bind:view
      canBack={back.length > 0}
      canForward={forward.length > 0}
      onback={goBack}
      onforward={goForward}
      onnavigate={navigate}
      {dropTarget}
      {springPath}
      {springMs}
      {theme}
      ontheme={() => {
        theme = nextTheme(theme);
        setTheme(theme);
      }}
    />
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
    />
    <footer class="status">
      <span
        >{plural(visible.length, "item")}{selection.size
          ? ` · ${selection.size} selected`
          : ""}</span
      >
      {#if clipboard}
        <span class="clip"
          >{plural(clipboard.paths.length, "item")}
          {clipboard.mode === "cut" ? "to move" : "to copy"} · Ctrl+V to paste</span
        >
      {/if}
      <OpsPill
        {jobs}
        onpause={queuePause}
        onresume={queueResume}
        oncancel={queueCancel}
        ondismiss={dismissJob}
      />
    </footer>
  </main>

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

  .clip {
    color: var(--accent-default);
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
