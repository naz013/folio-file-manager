<script lang="ts">
  import { tick } from "svelte";
  import { listDir, pathSegments, type Entry } from "$lib/api/fs";
  import { formatDate, formatSize } from "$lib/format";
  import ColumnList from "./ColumnList.svelte";
  import Thumbnail from "./Thumbnail.svelte";

  let {
    path,
    entries,
    selection,
    cursorIndex,
    cut,
    renaming,
    showHidden,
    folderTint,
    onitemclick,
    onopen,
    onitemcontextmenu,
    onbackgroundclick,
    onrename,
    oncancelrename,
    ondragout,
    onselectin,
  }: {
    /** The open folder: the last of the columns the keyboard works in. */
    path: string;
    /** What the open folder shows, in its order. */
    entries: Entry[];
    selection: Set<string>;
    cursorIndex: number;
    cut: Set<string>;
    renaming: string | null;
    showHidden: boolean;
    folderTint: (path: string) => string | null;
    onitemclick: (index: number, e: MouseEvent) => void;
    onopen: (entry: Entry) => void;
    onitemcontextmenu: (index: number, e: MouseEvent) => void;
    onbackgroundclick: () => void;
    onrename: (entry: Entry, newName: string) => void;
    oncancelrename: () => void;
    ondragout: (paths: string[]) => void;
    /** Open `dir` and select `item` in it: a click in a column that is not the open one. */
    onselectin: (dir: string, item: string) => void;
  } = $props();

  /** How long the cursor rests on a folder before its contents are read for the column on the right. */
  const PEEK_DELAY_MS = 100;

  let current: ColumnList | undefined = $state();
  let strip: HTMLDivElement | undefined = $state();

  /** The folders from the root down to the open one; each but the last has a column of its own. */
  const chain = $derived(pathSegments(path));
  const ancestors = $derived(chain.slice(0, -1));

  /** The one selected item, whose contents or preview fill the column on the right. */
  const peek = $derived(
    selection.size === 1 && cursorIndex >= 0 && selection.has(entries[cursorIndex]?.path ?? "")
      ? entries[cursorIndex]
      : null,
  );
  const peekFolder = $derived(peek?.isDir ? peek.path : null);

  /** What each folder shown to the left or right of the open one holds. */
  let lists = $state.raw<Record<string, Entry[]>>({});
  /** The newest read of each folder: an answer to an older one is dropped. */
  const newest: Record<string, number> = {};
  let reads = 0;

  function read(dir: string) {
    const id = ++reads;
    newest[dir] = id;
    listDir(dir)
      .then((list) => newest[dir] === id && (lists = { ...lists, [dir]: list }))
      .catch(() => newest[dir] === id && !lists[dir] && (lists = { ...lists, [dir]: [] }));
  }

  // The folders above the open one are read again when the open folder changes. What one held
  // before stays on screen until its new list arrives, so a column doesn't flash empty.
  $effect(() => {
    for (const dir of ancestors) read(dir.path);
  });

  // The peeked folder is read once the cursor rests: moving through a list of folders with
  // the arrow keys doesn't read each one.
  $effect(() => {
    const dir = peekFolder;
    if (!dir) return;
    const timer = setTimeout(() => read(dir), PEEK_DELAY_MS);
    return () => clearTimeout(timer);
  });

  // Only what is on screen is kept.
  $effect(() => {
    const keep = new Set([...ancestors.map((a) => a.path), peekFolder]);
    const dropped = Object.keys(lists).filter((dir) => !keep.has(dir));
    if (!dropped.length) return;
    for (const dir of dropped) delete newest[dir];
    lists = Object.fromEntries(Object.entries(lists).filter(([dir]) => keep.has(dir)));
  });

  /** Each folder's items without the hidden ones, worked out when its list or the setting changes. */
  const columnItems = $derived(
    Object.fromEntries(
      Object.entries(lists).map(([dir, list]) => [
        dir,
        list.filter((e) => showHidden || !e.hidden),
      ]),
    ),
  );
  const shown = (dir: string) => columnItems[dir] ?? [];

  // The newest columns are on the right: keep them in view.
  $effect(() => {
    void path;
    // Only a new column moves the view, not a step of the cursor between files.
    void !!peekFolder;
    void !!peek;
    tick().then(() => strip && (strip.scrollLeft = strip.scrollWidth));
  });

  const none = new Set<string>();

  export function focusList() {
    current?.focusList();
  }
  export function scrollToIndex(index: number) {
    current?.scrollToIndex(index);
  }
  export function scrollToTop() {
    current?.scrollToTop();
  }
  export function scrollElement(): HTMLElement {
    // The open folder's column is always shown.
    return current!.scrollElement();
  }
  /** The keys move through one column, up and down. */
  export function columns(): number {
    return 1;
  }
</script>

<div class="strip" bind:this={strip}>
  {#each ancestors as dir, i (dir.path)}
    {@const list = shown(dir.path)}
    <ColumnList
      entries={list}
      selected={none}
      highlight={chain[i + 1].path}
      label="Contents of {dir.name}"
      {folderTint}
      onitemclick={(index) => onselectin(dir.path, list[index].path)}
      {onopen}
    />
  {/each}

  <ColumnList
    bind:this={current}
    thumbnails
    {entries}
    selected={selection}
    {cursorIndex}
    {cut}
    {renaming}
    active
    label="Files"
    {folderTint}
    {onitemclick}
    {onopen}
    {onitemcontextmenu}
    {onbackgroundclick}
    {onrename}
    {oncancelrename}
    {ondragout}
  />

  {#if peek && peekFolder}
    {#if lists[peekFolder]}
      {@const list = shown(peekFolder)}
      <ColumnList
        entries={list}
        selected={none}
        label="Contents of {peek.name}"
        {folderTint}
        onitemclick={(index) => onselectin(peekFolder, list[index].path)}
        {onopen}
      />
    {/if}
  {:else if peek}
    <section class="preview" aria-label="Preview of {peek.name}">
      <div class="thumb">
        <Thumbnail path={peek.path} name={peek.name} size={160} />
      </div>
      <h3>{peek.name}</h3>
      <dl>
        <dt>Size</dt>
        <dd>{formatSize(peek.size)}</dd>
        <dt>Modified</dt>
        <dd>{formatDate(peek.modified)}</dd>
      </dl>
      <p class="hint">Space for Quick Look · Enter to open</p>
    </section>
  {:else if selection.size > 1}
    <section class="preview" aria-label="Selection">
      <h3>{selection.size} items selected</h3>
    </section>
  {/if}
</div>

<style>
  .strip {
    display: flex;
    flex: 1;
    min-height: 0;
    overflow-x: auto;
    overflow-y: hidden;
  }

  .preview {
    display: flex;
    flex: none;
    flex-direction: column;
    gap: var(--space-md);
    align-items: center;
    width: calc(var(--size-column) + var(--space-3xl));
    padding: var(--space-xl) var(--space-lg);
    overflow-y: auto;
  }

  .thumb {
    display: grid;
    place-items: center;
    width: 100%;
    min-height: calc(var(--space-3xl) * 4);
  }

  h3 {
    margin: 0;
    max-width: 100%;
    font-size: var(--font-size-body);
    font-weight: var(--font-weight-semibold);
    text-align: center;
    overflow-wrap: anywhere;
  }

  dl {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: var(--space-xs) var(--space-md);
    margin: 0;
    font-size: var(--font-size-caption);
  }

  dt {
    color: var(--text-tertiary);
  }

  dd {
    margin: 0;
    color: var(--text-secondary);
  }

  .hint {
    margin: 0;
    color: var(--text-tertiary);
    font-size: var(--font-size-caption);
  }
</style>
