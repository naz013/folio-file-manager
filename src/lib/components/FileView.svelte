<script lang="ts" module>
  // Grid geometry (matches the Penpot "File Tile" component at scale 1; `$lib/gridScale` has
  // the tile's size).
  const GAP_X = 6;
  const GAP_Y = 8;
  const GRID_PAD_X = 24;
  const GRID_PAD_Y = 20;
  // List geometry (matches "File Row").
  const ROW_H = 32;
  /** How close to the top or bottom edge (px) a rubber band scrolls the list, and how fast. */
  const EDGE_ZONE = 24;
  const EDGE_STEP = 24;
  const LIST_PAD_X = 16;
  const LIST_PAD_Y = 4;
  /** Extra rows rendered above and below the viewport. */
  const OVERSCAN = 3;
  /** Pointer travel (px) before a press turns into a native drag. */
  const DRAG_THRESHOLD = 6;
</script>

<script lang="ts">
  import { tick, untrack } from "svelte";
  import type { Entry } from "$lib/api/fs";
  import { fadeIn } from "$lib/animate";
  import { formatDate, formatSize } from "$lib/format";
  import {
    DEFAULT_SCALE,
    MAX_SCALE,
    MIN_SCALE,
    STEP,
    clampScale,
    fetchSize,
    scrollToKeep,
    stepScale,
    tileSize,
    type Geometry,
  } from "$lib/gridScale";
  import { animatesList } from "$lib/motion";
  import { reducedMotion, settings, updateSettings } from "$lib/settings.svelte";
  import { createZoomEase, wheelFactor } from "$lib/zoom";
  import type { SortKey, SortState } from "$lib/sort";
  import { SORT_HEADERS as headers } from "$lib/sort";
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
    onbackgroundcontextmenu,
    onopentab,
    dropTarget,
    springPath,
    springMs,
    tagColors,
    folderTint,
    onband,
    sort,
    onsort,
    folderSize,
    cursorIndex = -1,
  }: {
    /** The item the keyboard is on (an index into `entries`), for screen readers; -1 for none. */
    cursorIndex?: number;
    /** The selection a rubber band drawn over the empty space gives: these paths. */
    onband: (paths: string[]) => void;
    /** What the list is ordered by, shown on the column headers. */
    sort: SortState;
    onsort: (key: SortKey) => void;
    /** A folder's size from the scan cache, or undefined if it hasn't been scanned. */
    folderSize: (path: string) => number | undefined;
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
    /** Right-click on the empty space around the items. */
    onbackgroundcontextmenu: (e: MouseEvent) => void;
    /** Middle-click on a folder: open it in a new background tab. */
    onopentab: (path: string) => void;
    /** Folder path currently under a native drag, highlighted as drop target. */
    dropTarget: string | null;
    /** Folder whose spring-loading countdown is running, and its length. */
    springPath: string | null;
    springMs: number;
    /** The tags on the item at this path: each tag's name and CSS colour. */
    tagColors: (path: string) => { name: string; color: string }[];
    /** The CSS colour a folder's icon takes from its first colour tag, if it has one. */
    folderTint: (path: string) => string | null;
  } = $props();

  /** Folio builds a copy under this name and only renames it once it is complete. */
  const isPartial = (name: string) => name.toLowerCase().endsWith(".folio-partial");

  let scroller: HTMLDivElement;
  let scrollTop = $state(0);
  let width = $state(0);
  let height = $state(0);
  let press: { x: number; y: number; entry: Entry } | null = null;
  let canvas: HTMLDivElement;

  /** A rubber band being drawn, in canvas coordinates (scrolling included). */
  let band = $state<{ x0: number; y0: number; x1: number; y1: number } | null>(null);
  /** Where a press on the empty space began, until it has moved far enough to be a band. */
  let bandStart: { x: number; y: number; base: string[] } | null = null;
  /** The click that ends a band must not clear what the band selected. */
  let banded = false;

  let grid = $derived(view === "grid");

  // Grid and list cross-fade when switched. Items are placed by the virtual scroller, so only
  // the canvas fades, once; a folder too big to animate switches instantly.
  let shownGrid: boolean | null = null;
  $effect(() => {
    const was = shownGrid;
    shownGrid = grid;
    if (was === null || was === grid) return;
    if (canvas && animatesList(entries.length, reducedMotion())) fadeIn(canvas);
  });
  // ---- The size of the grid's tiles ----
  //
  // `live` follows a pinch or a key as it happens; the pictures are asked for at the size it had
  // when it last held still (`settled`), so a gesture scales the ones on screen and asks for
  // none, and a sharper size is loaded once it stops. The setting is saved at that point too.
  let live = $state(settings.gridScale);
  let settled = $state(settings.gridScale);
  let tile = $derived(tileSize(live));
  let ask = $derived(fetchSize(tileSize(settled).picture));

  // The setting changed somewhere else (Settings, the other pane): follow it. What this view
  // saves itself comes back equal and changes nothing.
  $effect(() => {
    const v = settings.gridScale;
    untrack(() => {
      if (v === live) return;
      ease.stop();
      // Around the middle of the window, so what is being looked at stays about where it was.
      const before = geometry();
      const point = { x: width / 2, y: scroller.scrollTop + height / 2 };
      live = settled = v;
      // A list has no tiles to keep in place. The grid's are re-placed first (flushSync is
      // not for an effect).
      if (!grid) return;
      tick().then(() => {
        scroller.scrollTop = scrollToKeep(
          before,
          geometry(),
          point,
          height / 2,
          entries.length,
          scroller.scrollHeight,
          scroller.clientHeight,
        );
        scrollTop = scroller.scrollTop;
      });
    });
  });

  $effect(() => {
    const scale = live;
    const timer = setTimeout(() => {
      settled = scale;
      if (settings.gridScale !== scale) updateSettings({ gridScale: scale });
    }, 200);
    return () => clearTimeout(timer);
  });

  let padX = $derived(grid ? GRID_PAD_X : LIST_PAD_X);
  let padY = $derived(grid ? GRID_PAD_Y : LIST_PAD_Y);
  let rowH = $derived(grid ? tile.h + GAP_Y : ROW_H);
  let cols = $derived(
    grid ? Math.max(1, Math.floor((width - padX * 2 + GAP_X) / (tile.w + GAP_X))) : 1,
  );
  const geometry = (): Geometry => ({
    padX,
    padY,
    tileW: tile.w,
    gapX: GAP_X,
    rowH,
    cols,
  });

  /** Sets the scale, keeping the item under (x, y), measured from the list's corner. */
  function setScale(z: number, x: number, y: number) {
    const next = clampScale(z);
    const before = geometry();
    // Where the list will be, worked out from the numbers: the tiles then render once, in the
    // right window, instead of once for the new size and again for the new scroll.
    const t = tileSize(next);
    const after: Geometry = {
      ...before,
      tileW: t.w,
      rowH: t.h + GAP_Y,
      cols: Math.max(1, Math.floor((width - padX * 2 + GAP_X) / (t.w + GAP_X))),
    };
    const contentH = padY * 2 + Math.ceil(entries.length / after.cols) * after.rowH;
    const top = scrollToKeep(
      before,
      after,
      { x, y: scroller.scrollTop + y },
      y,
      entries.length,
      contentH,
      height,
    );
    live = next;
    scrollTop = top;
    // The list is as tall as it will be once it has rendered: only then can it scroll there.
    tick().then(() => (scroller.scrollTop = top));
  }

  /** Pinch and Ctrl+wheel ask for a scale; it eases there over a few frames. */
  const ease = createZoomEase({
    get: () => live,
    min: MIN_SCALE,
    max: MAX_SCALE,
    apply: setScale,
  });

  /** Bigger (1), smaller (-1) or the usual size (0), for the keys. False when the list is not a grid. */
  export function zoomGrid(dir: number): boolean {
    if (!grid) return false;
    const [x, y] = [width / 2, height / 2];
    const still = reducedMotion();
    if (dir === 0) ease.to(DEFAULT_SCALE, x, y, still);
    else if (still) ease.to(stepScale(live, dir), x, y, true);
    else ease.by(dir > 0 ? STEP : 1 / STEP, x, y);
    return true;
  }

  /** Pinch on a trackpad arrives as Ctrl+wheel; a touch screen pinches with two fingers. Only
   *  the grid takes either: in a list the gestures are left alone. */
  function pinch(node: HTMLElement) {
    let spread = 0;
    // Measured when a pinch starts, not at every touch move, which would force a layout each
    // time the tiles have just been changed.
    let box = node.getBoundingClientRect();
    const at = (x: number, y: number): [number, number] => [x - box.left, y - box.top];
    const onwheel = (e: WheelEvent) => {
      if (!e.ctrlKey || !grid) return;
      e.preventDefault();
      // Fingers on the screen are pinching; a wheel event then must not pull against them.
      if (spread) return;
      box = node.getBoundingClientRect();
      const [x, y] = at(e.clientX, e.clientY);
      ease.by(wheelFactor(e, height), x, y);
    };
    const apart = (e: TouchEvent) =>
      Math.hypot(
        e.touches[0].clientX - e.touches[1].clientX,
        e.touches[0].clientY - e.touches[1].clientY,
      );
    const ontouchstart = (e: TouchEvent) => {
      spread = grid && e.touches.length === 2 ? apart(e) : 0;
      if (spread) {
        ease.stop();
        box = node.getBoundingClientRect();
      }
    };
    const ontouchmove = (e: TouchEvent) => {
      if (!grid || e.touches.length !== 2 || !spread) return;
      e.preventDefault();
      const d = apart(e);
      const [x, y] = at(
        (e.touches[0].clientX + e.touches[1].clientX) / 2,
        (e.touches[0].clientY + e.touches[1].clientY) / 2,
      );
      setScale(live * (d / spread), x, y);
      spread = d;
    };
    const onend = (e: TouchEvent) => {
      if (e.touches.length < 2) spread = 0;
    };
    node.addEventListener("wheel", onwheel, { passive: false });
    node.addEventListener("touchstart", ontouchstart, { passive: true });
    node.addEventListener("touchmove", ontouchmove, { passive: false });
    node.addEventListener("touchend", onend);
    node.addEventListener("touchcancel", onend);
    return {
      destroy() {
        ease.stop();
        node.removeEventListener("wheel", onwheel);
        node.removeEventListener("touchstart", ontouchstart);
        node.removeEventListener("touchmove", ontouchmove);
        node.removeEventListener("touchend", onend);
        node.removeEventListener("touchcancel", onend);
      },
    };
  }
  let rows = $derived(Math.ceil(entries.length / cols));
  let firstRow = $derived(Math.max(0, Math.floor((scrollTop - padY) / rowH) - OVERSCAN));
  let lastRow = $derived(Math.min(rows, Math.ceil((scrollTop + height - padY) / rowH) + OVERSCAN));
  let visible = $derived(
    entries
      .slice(firstRow * cols, lastRow * cols)
      .map((entry, i) => ({ entry, index: firstRow * cols + i })),
  );

  /** The cursor's item exists in the page right now (the list shows only a window of it). */
  const cursorShown = $derived(
    cursorIndex >= 0 &&
      cursorIndex < entries.length &&
      cursorIndex >= firstRow * cols &&
      cursorIndex < lastRow * cols,
  );

  export function columns(): number {
    return cols;
  }

  /**
   * Puts keyboard focus on the list, which is what lets a screen reader follow the cursor
   * (`aria-activedescendant`). Focus on something else that takes keys is left alone.
   */
  export function focusList() {
    const at = document.activeElement;
    if (!at || at === document.body || scroller.contains(at))
      scroller.focus({ preventScroll: true });
  }

  export function scrollToIndex(index: number) {
    const top = padY + Math.floor(index / cols) * rowH;
    if (top < scroller.scrollTop) scroller.scrollTop = top - padY;
    else if (top + rowH > scroller.scrollTop + height)
      scroller.scrollTop = top + rowH - height + padY;
  }

  export function scrollElement(): HTMLElement {
    return scroller;
  }

  export function scrollToTop() {
    scroller.scrollTop = 0;
  }

  function backgroundClick(e: MouseEvent) {
    if (banded) {
      banded = false;
      return;
    }
    if (!(e.target as HTMLElement).closest(".tile, .row")) onbackgroundclick();
  }

  /** The point under the pointer in canvas coordinates. */
  function canvasPoint(e: PointerEvent) {
    const box = canvas.getBoundingClientRect();
    return { x: e.clientX - box.left, y: e.clientY - box.top };
  }

  function bandDown(e: PointerEvent) {
    // Only the empty space starts a band, and not the scrollbar.
    const bare = e.target === scroller || e.target === canvas;
    if (e.button !== 0 || !bare || e.offsetX > scroller.clientWidth) return;
    // Starting a band takes focus off a rename field or the path bar.
    (document.activeElement as HTMLElement | null)?.blur?.();
    const { x, y } = canvasPoint(e);
    // With Ctrl or Shift the band adds to what is selected.
    bandStart = { x, y, base: e.ctrlKey || e.shiftKey || e.metaKey ? [...selection] : [] };
    scroller.setPointerCapture(e.pointerId);
  }

  /** The items a rectangle touches, from the layout alone, so no item needs to be on screen. */
  function indicesIn(left: number, top: number, right: number, bottom: number): number[] {
    const found: number[] = [];
    const first = Math.max(0, Math.floor((top - padY) / rowH));
    const last = Math.min(rows - 1, Math.floor((bottom - padY) / rowH));
    for (let r = first; r <= last; r++) {
      const itemTop = padY + r * rowH;
      if (bottom < itemTop || top > itemTop + (grid ? tile.h : ROW_H)) continue;
      for (let c = 0; c < cols; c++) {
        if (grid) {
          const itemLeft = padX + c * (tile.w + GAP_X);
          if (right < itemLeft || left > itemLeft + tile.w) continue;
        }
        const index = r * cols + c;
        if (index < entries.length) found.push(index);
      }
    }
    return found;
  }

  /** Where the pointer was last seen, in client coordinates, while a band is being drawn. */
  let lastPointer = { x: 0, y: 0 };
  let bandFrame = 0;

  /** Redraws the band and the selection from the last pointer position. */
  function updateBand() {
    if (!bandStart) return;
    const box = canvas.getBoundingClientRect();
    const x = lastPointer.x - box.left;
    const y = lastPointer.y - box.top;
    if (!band && Math.hypot(x - bandStart.x, y - bandStart.y) < DRAG_THRESHOLD) return;
    band = { x0: bandStart.x, y0: bandStart.y, x1: x, y1: y };
    const hit = indicesIn(
      Math.min(band.x0, band.x1),
      Math.min(band.y0, band.y1),
      Math.max(band.x0, band.x1),
      Math.max(band.y0, band.y1),
    ).map((i) => entries[i].path);
    onband([...new Set([...bandStart.base, ...hit])]);
  }

  /** While a band is drawn the list scrolls when the pointer rests near an edge, and the band
   * follows the list however it scrolled (the wheel included). */
  function bandLoop() {
    bandFrame = 0;
    if (!bandStart) return;
    if (band) {
      const box = scroller.getBoundingClientRect();
      if (lastPointer.y < box.top + EDGE_ZONE) scroller.scrollTop -= EDGE_STEP;
      else if (lastPointer.y > box.bottom - EDGE_ZONE) scroller.scrollTop += EDGE_STEP;
      updateBand();
    }
    bandFrame = requestAnimationFrame(bandLoop);
  }

  function bandMove(e: PointerEvent) {
    if (!bandStart) return;
    if (!(e.buttons & 1)) {
      bandUp();
      return;
    }
    lastPointer = { x: e.clientX, y: e.clientY };
    updateBand();
    if (!bandFrame) bandFrame = requestAnimationFrame(bandLoop);
  }

  function bandUp() {
    cancelAnimationFrame(bandFrame);
    bandFrame = 0;
    if (band) {
      // The click that ends the drag comes right after; it must not clear the selection.
      banded = true;
      setTimeout(() => (banded = false), 0);
    }
    band = null;
    bandStart = null;
  }

  function position(index: number) {
    const row = Math.floor(index / cols);
    const col = index % cols;
    return grid
      ? `top:${padY + row * rowH}px;left:${padX + col * (tile.w + GAP_X)}px;width:${tile.w}px;height:${tile.h}px`
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
  <div class="list-head" role="row" style:padding="0 {LIST_PAD_X + 10}px">
    {#each headers as [key, label] (key)}
      <span
        role="columnheader"
        aria-sort={sort.key === key ? (sort.desc ? "descending" : "ascending") : "none"}
      >
        <button
          class:num={key === "size"}
          class:active={sort.key === key}
          onclick={() => onsort(key)}
        >
          {label}{sort.key === key ? (sort.desc ? " ↓" : " ↑") : ""}
        </button>
      </span>
    {/each}
  </div>
{/if}

<svelte:window onpointermove={pointerMove} onpointerup={() => (press = null)} />

<!-- Keys are handled at window level in +page.svelte, whichever element has focus. -->
<!-- svelte-ignore a11y_click_events_have_key_events -->
<div
  class="scroller"
  class:grid
  style={grid
    ? `--thumb-w:${tile.thumbW}px;--thumb-h:${tile.thumbH}px;--tile-name:${tile.thumbW + 8}px`
    : undefined}
  use:pinch
  role="listbox"
  aria-label="Files"
  aria-multiselectable="true"
  aria-activedescendant={cursorShown ? `file-${cursorIndex}` : undefined}
  tabindex="0"
  bind:this={scroller}
  bind:clientWidth={width}
  bind:clientHeight={height}
  onpointerdown={bandDown}
  onmousedown={(e) => {
    // No text selection while a band is drawn over the empty space.
    if (e.target === scroller || e.target === canvas) e.preventDefault();
  }}
  onpointermove={bandMove}
  onpointerup={bandUp}
  onpointercancel={bandUp}
  onscroll={() => (scrollTop = scroller.scrollTop)}
  onclick={backgroundClick}
  oncontextmenu={(e) => {
    e.preventDefault();
    onbackgroundcontextmenu(e);
  }}
>
  <div class="canvas" bind:this={canvas} style:height="{padY * 2 + rows * rowH}px">
    {#if band}
      <div
        class="band"
        style:left="{Math.min(band.x0, band.x1)}px"
        style:top="{Math.min(band.y0, band.y1)}px"
        style:width="{Math.abs(band.x1 - band.x0)}px"
        style:height="{Math.abs(band.y1 - band.y0)}px"
      ></div>
    {/if}
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
        class:springing={springPath === entry.path}
        style="{position(index)};--spring-ms:{springMs}ms"
        data-drop-path={entry.isDir ? entry.path : undefined}
        id="file-{index}"
        role="option"
        tabindex="-1"
        aria-selected={selected}
        onclick={(e) => onitemclick(index, e)}
        ondblclick={() => onopen(entry)}
        onpointerdown={(e) => pointerDown(e, entry)}
        onmousedown={(e) => e.button === 1 && e.preventDefault()}
        onauxclick={(e) => e.button === 1 && entry.isDir && onopentab(entry.path)}
        oncontextmenu={(e) => {
          e.preventDefault();
          e.stopPropagation();
          onitemcontextmenu(index, e);
        }}
      >
        {#if grid}
          <span class="thumb">
            {#if entry.isDir}<FolderIcon
                size={tile.folder}
                tint={folderTint(entry.path)}
              />{:else}<Thumbnail
                path={entry.path}
                name={entry.name}
                size={tile.picture}
                {ask}
              />{/if}
          </span>
        {:else}
          <span class="icon">
            {#if entry.isDir}<FolderIcon size={18} tint={folderTint(entry.path)} />{:else}<Thumbnail
                path={entry.path}
                name=""
                size={18}
              />{/if}
          </span>
        {/if}

        {#if renaming === entry.path}
          <input
            class="rename"
            aria-label="New name"
            value={entry.name}
            use:renameInput={entry}
            onkeydown={(e) => renameKey(e, entry)}
            onblur={(e) => onrename(entry, e.currentTarget.value)}
            onclick={(e) => e.stopPropagation()}
            ondblclick={(e) => e.stopPropagation()}
          />
        {:else}
          {@const dots = tagColors(entry.path)}
          <span class="name"
            >{entry.name}{#if isPartial(entry.name)}<span
                class="partial"
                title="An unfinished copy made by Folio. It is left behind when a copy is stopped, and can be cleaned up from Cleanup."
                >incomplete copy</span
              >{/if}{#if dots.length}<span class="dots" title={dots.map((d) => d.name).join(", ")}
                >{#each dots as d (d.name)}<i style:background={d.color}></i>{/each}</span
              >{/if}</span
          >
        {/if}

        {#if !grid}
          <span class="muted">{formatDate(entry.modified)}</span>
          {@const folder = entry.isDir ? folderSize(entry.path) : undefined}
          <span class="muted num"
            >{entry.isDir
              ? folder === undefined
                ? "—"
                : formatSize(folder)
              : formatSize(entry.size)}</span
          >
        {/if}
      </div>
    {/each}
  </div>
</div>

<style>
  .scroller:focus-visible {
    outline-offset: -2px;
  }

  .scroller {
    flex: 1;
    overflow-y: auto;
    overflow-x: hidden;
  }

  /* One finger scrolls; the two-finger pinch is the grid's. */
  .scroller.grid {
    touch-action: pan-y;
  }

  .band {
    position: absolute;
    z-index: 2;
    border: 1px solid var(--accent-default);
    border-radius: var(--radius-xs);
    background: var(--accent-subtle);
    opacity: 0.6;
    pointer-events: none;
  }

  .canvas {
    position: relative;
  }

  .tile,
  .row {
    position: absolute;
  }

  .partial {
    margin-left: var(--space-xs);
    padding: 0 var(--space-xs);
    border-radius: var(--radius-xs);
    background: var(--bg-control);
    color: var(--text-secondary);
    font-size: var(--font-size-caption);
    vertical-align: middle;
    white-space: nowrap;
  }

  .dots {
    display: inline-flex;
    gap: var(--space-2xs);
    margin-left: var(--space-xs);
    vertical-align: middle;
  }

  .dots i {
    width: 8px;
    height: 8px;
    border-radius: 50%;
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
    width: var(--thumb-w, 112px);
    height: var(--thumb-h, 88px);
    border-radius: 10px;
    transition: background-color var(--duration-fast) var(--ease-standard);
  }

  .tile.selected .thumb,
  .tile.drop .thumb {
    background: var(--bg-hover);
  }

  .tile.drop .thumb {
    outline: 2px solid var(--accent-default);
  }

  .tile .name {
    max-width: var(--tile-name, 120px);
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
    width: var(--tile-name, 120px);
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

  .list-head button {
    width: 100%;
    padding: 0;
    text-align: left;
    font: inherit;
    color: inherit;
  }

  .list-head button.num {
    text-align: right;
  }

  .list-head button:hover,
  .list-head button.active {
    color: var(--text-primary);
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
    transition: background-color var(--duration-fast) var(--ease-standard);
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
    /* A faint line along the bottom edge keeps neighbouring selected rows apart. */
    background:
      linear-gradient(
        to top,
        color-mix(in srgb, var(--text-on-accent) 30%, transparent) 1px,
        transparent 1px
      ),
      var(--accent-default);
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
