<script lang="ts" module>
  // Grid geometry (matches the Penpot "File Tile" component).
  const TILE_W = 132;
  const TILE_H = 150;
  const GAP_X = 6;
  const GAP_Y = 8;
  const GRID_PAD_X = 24;
  const GRID_PAD_Y = 20;
  // List geometry (matches "File Row").
  const ROW_H = 32;
  const LIST_PAD_X = 16;
  const LIST_PAD_Y = 4;
  /** Extra rows rendered above and below the viewport. */
  const OVERSCAN = 3;
  /** Pointer travel (px) before a press turns into a native drag. */
  const DRAG_THRESHOLD = 6;
</script>

<script lang="ts">
  import type { Entry } from "$lib/api/fs";
  import { formatDate, formatSize } from "$lib/format";
  import FolderIcon from "./FolderIcon.svelte";
  import Thumbnail from "./Thumbnail.svelte";
  import type { View } from "./Toolbar.svelte";

  let {
    entries,
    view,
    selection,
    cut,
    renaming,
    onitemclick,
    onbackgroundclick,
    onopen,
    onrename,
    oncancelrename,
    ondragout,
    onitemcontextmenu,
    dropTarget,
  }: {
    entries: Entry[];
    view: View;
    selection: Set<string>;
    /** Paths currently cut to the clipboard (rendered dimmed). */
    cut: Set<string>;
    renaming: string | null;
    onitemclick: (index: number, e: MouseEvent) => void;
    onbackgroundclick: () => void;
    onopen: (entry: Entry) => void;
    onrename: (entry: Entry, newName: string) => void;
    oncancelrename: () => void;
    /** Called once a press on an item moves far enough to become a drag. */
    ondragout: (paths: string[]) => void;
    onitemcontextmenu: (index: number, e: MouseEvent) => void;
    /** Folder path currently under a native drag, highlighted as drop target. */
    dropTarget: string | null;
  } = $props();

  let scroller: HTMLDivElement;
  let scrollTop = $state(0);
  let width = $state(0);
  let height = $state(0);
  let press: { x: number; y: number; entry: Entry } | null = null;

  let grid = $derived(view === "grid");
  let padX = $derived(grid ? GRID_PAD_X : LIST_PAD_X);
  let padY = $derived(grid ? GRID_PAD_Y : LIST_PAD_Y);
  let rowH = $derived(grid ? TILE_H + GAP_Y : ROW_H);
  let cols = $derived(grid ? Math.max(1, Math.floor((width - padX * 2 + GAP_X) / (TILE_W + GAP_X))) : 1);
  let rows = $derived(Math.ceil(entries.length / cols));
  let firstRow = $derived(Math.max(0, Math.floor((scrollTop - padY) / rowH) - OVERSCAN));
  let lastRow = $derived(Math.min(rows, Math.ceil((scrollTop + height - padY) / rowH) + OVERSCAN));
  let visible = $derived(
    entries.slice(firstRow * cols, lastRow * cols).map((entry, i) => ({ entry, index: firstRow * cols + i })),
  );

  export function columns(): number {
    return cols;
  }

  export function scrollToIndex(index: number) {
    const top = padY + Math.floor(index / cols) * rowH;
    if (top < scroller.scrollTop) scroller.scrollTop = top - padY;
    else if (top + rowH > scroller.scrollTop + height) scroller.scrollTop = top + rowH - height + padY;
  }

  export function scrollToTop() {
    scroller.scrollTop = 0;
  }

  function backgroundClick(e: MouseEvent) {
    if (!(e.target as HTMLElement).closest(".tile, .row")) onbackgroundclick();
  }

  function position(index: number) {
    const row = Math.floor(index / cols);
    const col = index % cols;
    return grid
      ? `top:${padY + row * rowH}px;left:${padX + col * (TILE_W + GAP_X)}px;width:${TILE_W}px;height:${TILE_H}px`
      : `top:${padY + row * rowH}px;left:${padX}px;right:${padX}px;height:${ROW_H}px`;
  }

  function pointerDown(e: PointerEvent, entry: Entry) {
    if (e.button === 0 && renaming !== entry.path) press = { x: e.clientX, y: e.clientY, entry };
  }

  function pointerMove(e: PointerEvent) {
    if (!press) return;
    if (!(e.buttons & 1)) {
      press = null;
      return;
    }
    if (Math.hypot(e.clientX - press.x, e.clientY - press.y) < DRAG_THRESHOLD) return;
    const { entry } = press;
    press = null;
    ondragout(selection.has(entry.path) ? [...selection] : [entry.path]);
  }

  function renameInput(node: HTMLInputElement, entry: Entry) {
    node.focus();
    // Select the name without its extension, like Explorer and Finder.
    const dot = entry.isDir ? -1 : entry.name.lastIndexOf(".");
    node.setSelectionRange(0, dot > 0 ? dot : entry.name.length);
  }

  function renameKey(e: KeyboardEvent, entry: Entry) {
    e.stopPropagation();
    const input = e.currentTarget as HTMLInputElement;
    if (e.key === "Enter") onrename(entry, input.value);
    else if (e.key === "Escape") oncancelrename();
  }
</script>

{#if !grid}
  <div class="list-head" style:padding="0 {LIST_PAD_X + 10}px">
    <span>Name</span><span>Date modified</span><span class="num">Size</span>
  </div>
{/if}

<svelte:window onpointermove={pointerMove} onpointerup={() => (press = null)} />

<div
  class="scroller"
  bind:this={scroller}
  bind:clientWidth={width}
  bind:clientHeight={height}
  onscroll={() => (scrollTop = scroller.scrollTop)}
  onclick={backgroundClick}
  role="presentation">
  <div class="canvas" style:height="{padY * 2 + rows * rowH}px">
    {#each visible as { entry, index } (entry.path)}
      {@const selected = selection.has(entry.path)}
      <!-- Keyboard navigation for items is handled at window level in +page.svelte. -->
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <div
        class={grid ? "tile" : "row"}
        class:zebra={!grid && index % 2 === 1}
        class:selected
        class:cut={cut.has(entry.path)}
        class:drop={dropTarget === entry.path}
        style={position(index)}
        data-drop-path={entry.isDir ? entry.path : undefined}
        role="option"
        tabindex="-1"
        aria-selected={selected}
        onclick={(e) => onitemclick(index, e)}
        ondblclick={() => onopen(entry)}
        onpointerdown={(e) => pointerDown(e, entry)}
        oncontextmenu={(e) => {
          e.preventDefault();
          onitemcontextmenu(index, e);
        }}>
        {#if grid}
          <span class="thumb">
            {#if entry.isDir}<FolderIcon size={64} />{:else}<Thumbnail path={entry.path} name={entry.name} size={72} />{/if}
          </span>
        {:else}
          <span class="icon">
            {#if entry.isDir}<FolderIcon size={18} />{:else}<Thumbnail path={entry.path} name="" size={18} />{/if}
          </span>
        {/if}

        {#if renaming === entry.path}
          <input
            class="rename"
            value={entry.name}
            use:renameInput={entry}
            onkeydown={(e) => renameKey(e, entry)}
            onblur={(e) => onrename(entry, e.currentTarget.value)}
            onclick={(e) => e.stopPropagation()}
            ondblclick={(e) => e.stopPropagation()} />
        {:else}
          <span class="name">{entry.name}</span>
        {/if}

        {#if !grid}
          <span class="muted">{formatDate(entry.modified)}</span>
          <span class="muted num">{entry.isDir ? "—" : formatSize(entry.size)}</span>
        {/if}
      </div>
    {/each}
  </div>
</div>

<style>
  .scroller {
    flex: 1;
    overflow-y: auto;
    overflow-x: hidden;
  }

  .canvas {
    position: relative;
  }

  .tile,
  .row {
    position: absolute;
  }

  /* Grid */
  .tile {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    padding: 6px;
    border-radius: var(--radius-lg);
  }

  .thumb {
    display: grid;
    place-items: center;
    flex-shrink: 0;
    width: 112px;
    height: 88px;
    border-radius: 10px;
  }

  .tile.selected .thumb,
  .tile.drop .thumb {
    background: var(--bg-hover);
  }

  .tile.drop .thumb {
    outline: 2px solid var(--accent-default);
  }

  .tile .name {
    max-width: 120px;
    padding: 2px 6px;
    border-radius: var(--radius-sm);
    font-size: 12px;
    font-weight: var(--font-weight-medium);
    text-align: center;
    overflow-wrap: anywhere;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }

  .tile.selected .name {
    background: var(--accent-default);
    color: var(--text-on-accent);
  }

  .cut {
    opacity: 0.45;
  }

  .rename {
    width: 120px;
    padding: 2px 4px;
    border: 1px solid var(--accent-default);
    border-radius: var(--radius-sm);
    outline: 3px solid var(--accent-subtle);
    font: inherit;
    font-size: 12px;
    color: var(--text-primary);
    background: var(--bg-window);
    user-select: text;
  }

  .tile .rename {
    text-align: center;
  }

  /* List */
  .list-head,
  .row {
    display: grid;
    grid-template-columns: 26px 1fr 190px 90px;
    align-items: center;
    column-gap: 10px;
  }

  .list-head {
    grid-template-columns: 1fr 190px 90px;
    height: 30px;
    flex-shrink: 0;
    border-bottom: 1px solid var(--border-default);
    font-size: var(--font-size-caption);
    font-weight: var(--font-weight-semibold);
    color: var(--text-tertiary);
  }

  .row {
    padding: 0 10px;
    border-radius: 7px;
  }

  .row .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .row .rename {
    width: 100%;
    font-size: var(--font-size-body);
  }

  .row.zebra {
    background: var(--bg-subtle);
  }

  .row.drop {
    outline: 2px solid var(--accent-default);
    outline-offset: -2px;
  }

  .row.selected {
    background: var(--accent-default);
    color: var(--text-on-accent);
  }

  .row.selected .muted {
    color: inherit;
  }

  .icon {
    display: flex;
    align-items: center;
  }

  .muted {
    color: var(--text-secondary);
    font-size: 12.5px;
  }

  .num {
    text-align: right;
  }
</style>
