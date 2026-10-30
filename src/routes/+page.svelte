<script lang="ts">
  import { onMount } from "svelte";
  import { SvelteSet } from "svelte/reactivity";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import {
    copyPaths,
    createFolder,
    homeDir,
    listDir,
    movePaths,
    onDirChanged,
    openPath,
    parentOf,
    renamePath,
    sameVolume,
    showContextMenu,
    startDrag,
    trashPaths,
    watchDir,
    type Entry,
  } from "$lib/api/fs";
  import Sidebar from "$lib/components/Sidebar.svelte";
  import Toolbar, { type View } from "$lib/components/Toolbar.svelte";
  import FileView from "$lib/components/FileView.svelte";

  let home = $state("");
  let path = $state("");
  let entries = $state<Entry[]>([]);
  let view = $state<View>("grid");
  let showHidden = $state(false);

  let back = $state<string[]>([]);
  let forward = $state<string[]>([]);

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

  let visible = $derived(showHidden ? entries : entries.filter((e) => !e.hidden));

  function notify(text: string, error = false) {
    toast = { text, error };
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => (toast = null), error ? 5000 : 2500);
  }

  const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;

  /** Reloads the current folder, keeping the selection for paths that still exist. */
  async function refresh(select?: string[]) {
    try {
      entries = await listDir(path);
    } catch (e) {
      notify(String(e), true);
      return;
    }
    const existing = new Set(entries.map((e) => e.path));
    const keep = (select ?? [...selection]).filter((p) => existing.has(p));
    selection.clear();
    keep.forEach((p) => selection.add(p));
    cursor = keep.length ? visible.findIndex((e) => e.path === keep[keep.length - 1]) : -1;
    anchor = cursor;
  }

  async function load(target: string) {
    try {
      entries = await listDir(target);
    } catch (e) {
      notify(String(e), true);
      return false;
    }
    path = target;
    selection.clear();
    anchor = cursor = -1;
    renaming = null;
    fileView?.scrollToTop();
    watchDir(target).catch((e) => console.warn("watch failed", e));
    return true;
  }

  async function navigate(target: string) {
    if (target === path) return;
    const from = path;
    if (await load(target)) {
      if (from) back.push(from);
      forward = [];
    }
  }

  async function goBack() {
    const prev = back.pop();
    if (!prev) return;
    const from = path;
    if (await load(prev)) forward.push(from);
  }

  async function goForward() {
    const next = forward.pop();
    if (next) {
      const from = path;
      if (await load(next)) back.push(from);
    }
  }

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

  async function trashSelected() {
    const items = selected();
    if (!items.length) return;
    await run(
      () => trashPaths(items.map((e) => e.path)),
      () => `Moved ${plural(items.length, "item")} to Recycle Bin`,
    );
    await refresh([]);
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
    const result = await run(
      () => (copy ? copyPaths(paths, dest) : movePaths(paths, dest)),
      (r) => `${copy ? "Copied" : "Moved"} ${plural(r.length, "item")}`,
    );
    if (result) await refresh(dest === path ? result : []);
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

  async function dropPaths(paths: string[], dest: string) {
    // Dropping items back into the folder they came from is a no-op.
    const sources = paths.filter((p) => p !== dest && parentOf(p) !== dest);
    if (sources.length) await transfer(sources, dest, !sameVolume(sources[0], dest));
  }

  function dragOut(paths: string[]) {
    startDrag(paths).catch((e) => notify(String(e), true));
  }

  // ---- Context menu ----

  function itemContextMenu(index: number, e: MouseEvent) {
    if (!selection.has(visible[index].path)) selectOnly(index);
    showContextMenu([...selection], e.shiftKey).catch((err) => notify(String(err), true));
  }

  /** Suppress the webview's own menu everywhere except text fields. */
  function oncontextmenu(e: MouseEvent) {
    if ((e.target as HTMLElement).tagName !== "INPUT") e.preventDefault();
  }

  // ---- Keyboard ----

  function onkeydown(e: KeyboardEvent) {
    if (renaming || (e.target as HTMLElement).tagName === "INPUT") return;
    const ctrl = e.ctrlKey || e.metaKey;
    const key = e.key;
    const handled = () => e.preventDefault();

    if (e.altKey && key === "ArrowLeft") (handled(), goBack());
    else if (e.altKey && key === "ArrowRight") (handled(), goForward());
    else if ((e.altKey && key === "ArrowUp") || key === "Backspace") {
      handled();
      const up = parentOf(path);
      if (up) navigate(up);
    } else if (key === "ArrowDown") (handled(), moveCursor(view === "grid" ? fileView.columns() : 1, e.shiftKey));
    else if (key === "ArrowUp") (handled(), moveCursor(-(view === "grid" ? fileView.columns() : 1), e.shiftKey));
    else if (key === "ArrowRight" && view === "grid") (handled(), moveCursor(1, e.shiftKey));
    else if (key === "ArrowLeft" && view === "grid") (handled(), moveCursor(-1, e.shiftKey));
    else if (key === "Home") (handled(), moveCursor(-visible.length, e.shiftKey));
    else if (key === "End") (handled(), moveCursor(visible.length, e.shiftKey));
    else if (key === "Enter" && cursor >= 0) (handled(), open(visible[cursor]));
    else if (key === "Delete") (handled(), trashSelected());
    else if ((e.shiftKey && key === "F10") || key === "ContextMenu") {
      handled();
      // Spike limitation: the menu opens at the mouse cursor, not at the focused item.
      if (selection.size) showContextMenu([...selection], false).catch((err) => notify(String(err), true));
    }
    else if (key === "F2" && cursor >= 0) (handled(), (renaming = visible[cursor].path));
    else if (key === "Escape") (handled(), selection.clear(), (clipboard = null));
    else if (ctrl && e.shiftKey && key.toLowerCase() === "n") (handled(), newFolder());
    else if (ctrl && key === "a") (handled(), selectRange(0, visible.length - 1));
    else if (ctrl && key === "c" && selection.size) (handled(), (clipboard = { mode: "copy", paths: [...selection] }), notify(`Copied ${plural(selection.size, "item")} to clipboard`));
    else if (ctrl && key === "x" && selection.size) (handled(), (clipboard = { mode: "cut", paths: [...selection] }));
    else if (ctrl && key === "v") (handled(), paste());
    else if (ctrl && key === "h") (handled(), (showHidden = !showHidden));
  }

  onMount(() => {
    const unlisten = onDirChanged((changed) => {
      if (changed === path && !renaming) refresh();
    });
    const unlistenDrop = getCurrentWebview().onDragDropEvent(({ payload }) => {
      if (payload.type === "enter" || payload.type === "over") dropTarget = folderAt(payload.position);
      else if (payload.type === "drop") {
        const dest = folderAt(payload.position) ?? path;
        dropTarget = null;
        dropPaths(payload.paths, dest);
      } else dropTarget = null;
    });
    homeDir().then(async (h) => {
      home = h;
      await load(h);
    });
    return () => {
      unlisten.then((u) => u());
      unlistenDrop.then((u) => u());
    };
  });
</script>

<svelte:window {onkeydown} {oncontextmenu} />

<div class="app">
  <Sidebar {home} current={path} onnavigate={navigate} {dropTarget} />
  <main class="main">
    <Toolbar
      {path}
      bind:view
      canBack={back.length > 0}
      canForward={forward.length > 0}
      onback={goBack}
      onforward={goForward}
      onnavigate={navigate} />
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
      {dropTarget} />
    <footer class="status">
      <span>{plural(visible.length, "item")}{selection.size ? ` · ${selection.size} selected` : ""}</span>
      {#if clipboard}
        <span class="clip">{plural(clipboard.paths.length, "item")} {clipboard.mode === "cut" ? "to move" : "to copy"} · Ctrl+V to paste</span>
      {/if}
    </footer>
  </main>

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
