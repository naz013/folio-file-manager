<script lang="ts">
  import { onMount } from "svelte";
  import { SvelteSet } from "svelte/reactivity";
  import { listDir, openPath, parentOf, pathSegments, type Entry } from "$lib/api/fs";
  import { lastSegments, stepCursor } from "$lib/pane";
  import { sortEntries, type SortKey, type SortState } from "$lib/sort";
  import FileView from "./FileView.svelte";
  import Icon from "./Icon.svelte";

  /** How often a quiet check looks for changes made outside the pane (ms). */
  const POLL_MS = 4000;
  /** One value each, so the list does not see a new one on every render. */
  const NO_CUT = new Set<string>();
  const NO_TAGS = () => [];
  const NO_TINT = () => null;
  const NO_SIZE = () => undefined;

  let {
    path,
    fallback,
    showHidden,
    active,
    refreshKey,
    otherPath,
    dropTarget,
    springPath,
    springMs,
    onpath,
    onfocuspane,
    ondragout,
    ontoother,
    ontrash,
    onmenu,
    onerror,
    onclose,
  }: {
    /** The folder shown. The pane only reports where it went (`onpath`); the page keeps it. */
    path: string;
    /** Where to go when `path` can no longer be read, at the start. */
    fallback: string;
    showHidden: boolean;
    /** Whether this pane has the keyboard, for the accent line under its header. */
    active: boolean;
    /** Changes whenever something finished elsewhere that may have changed this folder. */
    refreshKey: number;
    /** The other pane's folder, for "Show the other pane's folder". */
    otherPath: string;
    /** Folder under a drag, and the one whose spring-loading countdown is running. */
    dropTarget: string | null;
    springPath: string | null;
    springMs: number;
    onpath: (path: string) => void;
    onfocuspane: () => void;
    ondragout: (paths: string[]) => void;
    /** Copies or moves these items into the other pane's folder. */
    ontoother: (paths: string[], copy: boolean) => void;
    ontrash: (paths: string[]) => void;
    onmenu: (paths: string[], folder: string, e: MouseEvent | { x: number; y: number }) => void;
    onerror: (text: string) => void;
    onclose: () => void;
  } = $props();

  let entries = $state.raw<Entry[]>([]);
  const selection = new SvelteSet<string>();
  let anchor = $state(-1);
  let cursor = $state(-1);
  let sort = $state<SortState>({ key: "name", desc: false });
  let view = $state<"list" | "grid">("list");
  const back = $state<string[]>([]);
  let forward = $state<string[]>([]);
  let list = $state<FileView>();
  /** Counts loads, so a slow listing that a newer one overtook is thrown away. */
  let gen = 0;
  /** The folder the entries belong to; `path` may be ahead of it while a listing is on its way. */
  let shown = "";

  let visible = $derived(
    sortEntries(
      entries.filter((e) => showHidden || !e.hidden),
      sort,
      () => undefined,
    ),
  );
  let crumbs = $derived(lastSegments(pathSegments(path), 3));

  const selected = () => visible.filter((e) => selection.has(e.path));

  /** Reads `target`; false when it could not be (the error is passed on unless `quiet`). */
  async function show(target: string, quiet = false): Promise<boolean> {
    const mine = ++gen;
    let found: Entry[];
    try {
      found = await listDir(target);
    } catch (e) {
      if (!quiet) onerror(String(e));
      return false;
    }
    if (mine !== gen) return true;
    const moved = target !== shown;
    entries = found;
    shown = target;
    if (moved) {
      selection.clear();
      anchor = cursor = -1;
      list?.scrollToTop();
    } else {
      // Only what is still there stays selected.
      const here = new Set(found.map((e) => e.path));
      for (const p of [...selection]) if (!here.has(p)) selection.delete(p);
      cursor = Math.min(cursor, visible.length - 1);
    }
    return true;
  }

  async function go(target: string) {
    if (target === path) return;
    const from = path;
    if (await show(target)) {
      back.push(from);
      forward = [];
      onpath(target);
    }
  }

  async function goBack() {
    const to = back.at(-1);
    if (to === undefined) return;
    if (await show(to)) {
      forward.push(path);
      back.pop();
      onpath(to);
    }
  }

  async function goForward() {
    const to = forward.at(-1);
    if (to === undefined) return;
    if (await show(to)) {
      back.push(path);
      forward.pop();
      onpath(to);
    }
  }

  function goUp() {
    const up = parentOf(path);
    if (up) go(up);
  }

  /** Called from the page, to take the pane to a folder (a spring-loaded drag, "Match"). */
  export function navigate(target: string) {
    return go(target);
  }

  /** Opens the item at `target` (a folder, or a file in its program); the context menu's Open. */
  export function openItem(target: string) {
    const entry = entries.find((e) => e.path === target);
    if (entry) open(entry);
  }

  /** Whether `target` is a folder listed here, so a drag from this pane can be pinned. */
  export function isFolder(target: string): boolean {
    return entries.some((e) => e.path === target && e.isDir);
  }

  export function scrollElement(): HTMLElement {
    return list!.scrollElement();
  }

  function open(entry: Entry) {
    if (entry.isDir) go(entry.path);
    else openPath(entry.path).catch((e) => onerror(String(e)));
  }

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
    list?.focusList();
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
    const next = stepCursor(cursor, delta, visible.length);
    if (extend && anchor >= 0) selectRange(anchor, next);
    else {
      selectOnly(next);
    }
    cursor = next;
    list?.scrollToIndex(next);
    list?.focusList();
  }

  function bandSelect(paths: string[]) {
    selection.clear();
    for (const p of paths) selection.add(p);
    cursor = anchor = paths.length ? visible.findIndex((e) => e.path === paths.at(-1)) : -1;
  }

  function sortBy(key: SortKey) {
    sort = { key, desc: sort.key === key ? !sort.desc : false };
  }

  function itemContextMenu(index: number, e: MouseEvent) {
    if (!selection.has(visible[index].path)) selectOnly(index);
    onmenu([...selection], path, e);
  }

  /** Keys the pane answers to while it has the keyboard; the page ignores them. */
  function onkeydown(e: KeyboardEvent) {
    const target = e.target as HTMLElement;
    const inList = !!target.closest?.("[role=listbox]");
    if (target.tagName === "INPUT" || (e.ctrlKey && e.altKey)) return;
    const grid = view === "grid";
    const row = grid ? (list?.columns() ?? 1) : 1;
    const act = (run: () => void) => {
      e.preventDefault();
      e.stopPropagation();
      run();
    };
    if (e.altKey && !e.ctrlKey) {
      if (e.key === "ArrowLeft") return act(goBack);
      if (e.key === "ArrowRight") return act(goForward);
      if (e.key === "ArrowUp") return act(goUp);
      return;
    }
    // Buttons in the header keep Enter, Backspace and the arrows for themselves.
    if (!inList) return;
    if (e.ctrlKey || e.metaKey) {
      if (e.key.toLowerCase() === "a" && !e.shiftKey) {
        return act(() => selectRange(0, visible.length - 1));
      }
      return;
    }
    switch (e.key) {
      case "ArrowDown":
        return act(() => moveCursor(row, e.shiftKey));
      case "ArrowUp":
        return act(() => moveCursor(-row, e.shiftKey));
      case "ArrowRight":
        if (grid) act(() => moveCursor(1, e.shiftKey));
        return;
      case "ArrowLeft":
        if (grid) act(() => moveCursor(-1, e.shiftKey));
        return;
      case "Home":
        return act(() => moveCursor(-visible.length, e.shiftKey));
      case "End":
        return act(() => moveCursor(visible.length, e.shiftKey));
      case "Enter":
        if (cursor >= 0) act(() => open(visible[cursor]));
        return;
      case "Backspace":
        return act(goUp);
      case "Escape":
        if (selection.size) act(() => selection.clear());
        return;
      case "Delete":
        if (selection.size) act(() => ontrash([...selection]));
        return;
      case "F5":
        if (selection.size) act(() => ontoother([...selection], !e.shiftKey));
        else act(() => {});
        return;
      case "ContextMenu":
        return act(() => {
          const at = list?.scrollElement().getBoundingClientRect();
          onmenu(selected().length ? [...selection] : [], path, {
            x: (at?.left ?? 0) + 16,
            y: (at?.top ?? 0) + 16,
          });
        });
      case "F10":
        if (e.shiftKey) {
          const at = list?.scrollElement().getBoundingClientRect();
          act(() =>
            onmenu([...selection], path, { x: (at?.left ?? 0) + 16, y: (at?.top ?? 0) + 16 }),
          );
        }
        return;
    }
  }

  /** Set once the first look at the saved folder is over, so the effect below does not repeat it. */
  let started = $state(false);

  // The folder is the page's to change (it is saved with the settings); show whatever it says.
  $effect(() => {
    const target = path;
    if (started && target !== shown) void show(target);
  });

  // Something finished elsewhere: look again.
  let seenKey: number | null = null;
  $effect(() => {
    const key = refreshKey;
    const first = seenKey === null;
    const same = key === seenKey;
    seenKey = key;
    if (first || same) return;
    void show(shown || path, true);
  });

  onMount(() => {
    // The folder may be gone since last time: start from a place that exists.
    show(path, true).then((ok) => {
      if (!ok && fallback && fallback !== path) onpath(fallback);
      started = true;
    });
    const look = () => {
      if (document.visibilityState === "visible") void quietRefresh();
    };
    const timer = setInterval(look, POLL_MS);
    window.addEventListener("focus", look);
    return () => {
      clearInterval(timer);
      window.removeEventListener("focus", look);
    };
  });

  let unreachable = false;

  /** Looks again without a word, and leaves the list alone when nothing changed. */
  async function quietRefresh() {
    const target = shown;
    if (!target) return;
    let found: Entry[];
    try {
      found = await listDir(target);
      unreachable = false;
    } catch (e) {
      // Said once: a folder that went away would otherwise sit there looking fine.
      if (!unreachable) onerror(`The second pane's folder can't be read: ${String(e)}`);
      unreachable = true;
      return;
    }
    if (target !== shown) return;
    const same =
      found.length === entries.length &&
      found.every((e, i) => {
        const was = entries[i];
        return (
          e.path === was.path &&
          e.isDir === was.isDir &&
          e.size === was.size &&
          e.modified === was.modified
        );
      });
    if (!same) await show(target, true);
  }
</script>

<!-- The pane only watches for focus, and for keys that reach it from the list inside. -->
<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<section
  class="pane"
  class:active
  data-pane="side"
  aria-label="Second pane"
  onpointerdowncapture={onfocuspane}
  onfocusin={onfocuspane}
  {onkeydown}
>
  <header class="head">
    <button class="icon-btn" disabled={!back.length} onclick={goBack} aria-label="Back">
      <Icon name="chevronLeft" size={15} />
    </button>
    <button class="icon-btn" disabled={!forward.length} onclick={goForward} aria-label="Forward">
      <Icon name="chevronRight" size={15} />
    </button>
    <button class="icon-btn" disabled={!parentOf(path)} onclick={goUp} aria-label="Up one folder">
      <Icon name="chevronUp" size={15} />
    </button>
    <nav class="crumbs" aria-label="Second pane location">
      {#if crumbs.more}<span class="more" aria-hidden="true">…</span>{/if}
      {#each crumbs.shown as seg, i (seg.path)}
        {#if i > 0 || crumbs.more}<span class="sep" aria-hidden="true">›</span>{/if}
        <button
          class="crumb"
          class:last={i === crumbs.shown.length - 1}
          title={seg.path}
          aria-current={i === crumbs.shown.length - 1 ? "page" : undefined}
          onclick={() => go(seg.path)}
        >
          {seg.name}
        </button>
      {/each}
    </nav>
    <button
      class="text-btn"
      disabled={otherPath === path}
      onclick={() => go(otherPath)}
      title="Show the other pane's folder here"
    >
      Match
    </button>
    <button
      class="icon-btn"
      class:on={view === "list"}
      aria-pressed={view === "list"}
      onclick={() => (view = "list")}
      aria-label="List view"
    >
      <Icon name="list" size={15} />
    </button>
    <button
      class="icon-btn"
      class:on={view === "grid"}
      aria-pressed={view === "grid"}
      onclick={() => (view = "grid")}
      aria-label="Grid view"
    >
      <Icon name="grid" size={15} />
    </button>
    <button
      class="icon-btn"
      onclick={onclose}
      aria-label="Close second pane (F8)"
      title="Close (F8)"
    >
      <Icon name="x" size={15} />
    </button>
  </header>
  <div class="body">
    <FileView
      bind:this={list}
      entries={visible}
      {view}
      {selection}
      cut={NO_CUT}
      renaming={null}
      onitemclick={itemClick}
      onbackgroundclick={() => selectOnly(-1)}
      onopen={open}
      onrename={() => {}}
      oncancelrename={() => {}}
      {ondragout}
      onitemcontextmenu={itemContextMenu}
      onbackgroundcontextmenu={(e) => {
        selection.clear();
        anchor = cursor = -1;
        onmenu([], path, e);
      }}
      onopentab={() => {}}
      {dropTarget}
      {springPath}
      {springMs}
      tagColors={NO_TAGS}
      folderTint={NO_TINT}
      onband={bandSelect}
      {sort}
      onsort={sortBy}
      folderSize={NO_SIZE}
      cursorIndex={cursor}
    />
  </div>
  <footer class="foot" role="status">
    {visible.length} item{visible.length === 1 ? "" : "s"}{selection.size
      ? ` · ${selection.size} selected`
      : ""}
  </footer>
</section>

<style>
  .pane {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    border-left: 1px solid var(--border-default);
    background: var(--bg-window);
  }

  .head {
    display: flex;
    align-items: center;
    gap: var(--space-2xs);
    height: 40px;
    flex-shrink: 0;
    padding: 0 var(--space-sm);
    border-bottom: 2px solid var(--border-default);
    transition: border-color var(--duration-fast) var(--ease-standard);
  }

  .pane.active .head {
    border-bottom-color: var(--accent-default);
  }

  .body {
    position: relative;
    display: flex;
    flex: 1;
    flex-direction: column;
    min-height: 0;
  }

  .foot {
    display: flex;
    align-items: center;
    height: 30px;
    flex-shrink: 0;
    padding: 0 16px;
    border-top: 1px solid var(--border-default);
    font-size: 11.5px;
    font-weight: var(--font-weight-medium);
    color: var(--text-secondary);
  }

  .crumbs {
    display: flex;
    flex: 1;
    align-items: center;
    min-width: 0;
    gap: 2px;
    overflow: hidden;
    padding: 0 var(--space-xs);
    white-space: nowrap;
  }

  .crumb {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    padding: 2px 6px;
    border: 0;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--text-secondary);
    font: inherit;
    cursor: pointer;
  }

  .crumb:hover {
    background: var(--bg-hover);
  }

  .crumb.last {
    color: var(--text-primary);
    font-weight: var(--font-weight-medium);
  }

  .sep,
  .more {
    color: var(--text-tertiary);
  }

  .icon-btn,
  .text-btn {
    display: inline-grid;
    place-items: center;
    flex-shrink: 0;
    height: 28px;
    border: 0;
    border-radius: var(--radius-md);
    background: transparent;
    color: var(--text-secondary);
    font: inherit;
    cursor: pointer;
    transition: background-color var(--duration-fast) var(--ease-standard);
  }

  .icon-btn {
    width: 28px;
  }

  .text-btn {
    padding: 0 var(--space-sm);
    font-size: 12px;
  }

  .icon-btn:hover:not(:disabled),
  .text-btn:hover:not(:disabled) {
    background: var(--bg-hover);
  }

  .icon-btn.on {
    background: var(--bg-selected);
    color: var(--text-primary);
  }

  .icon-btn:disabled,
  .text-btn:disabled {
    opacity: 0.4;
    cursor: default;
  }

  .icon-btn:focus-visible,
  .text-btn:focus-visible,
  .crumb:focus-visible {
    outline: 2px solid var(--accent-default);
    outline-offset: -2px;
  }
</style>
