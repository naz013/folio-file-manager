<script lang="ts" module>
  /** Width and height of a filmstrip tile, and the space between two. The strip's own height,
   * `--size-filmstrip` in tokens.css, is a tile and its room above and below. */
  const TILE = 72;
  const GAP = 6;
  const STEP = TILE + GAP;
  const PAD_X = 8;
  /** Extra tiles rendered to the left and right of the strip's window. */
  const OVERSCAN = 6;
  /** Pointer travel (px) before a press turns into a native drag. */
  const DRAG_THRESHOLD = 6;
  /** The largest picture the thumbnail protocol makes. */
  const PREVIEW_PX = 512;
  /** How long the cursor rests on a file before the large picture changes to it. */
  const PREVIEW_SETTLE_MS = 100;
</script>

<script lang="ts">
  import { onDestroy } from "svelte";
  import { thumbUrl, type Entry } from "$lib/api/fs";
  import { formatDate, formatSize } from "$lib/format";
  import FileIcon from "./FileIcon.svelte";
  import FolderIcon from "./FolderIcon.svelte";
  import Thumbnail from "./Thumbnail.svelte";

  let {
    entries,
    selection,
    cursorIndex,
    cut,
    renaming,
    folderTint,
    onitemclick,
    onopen,
    onitemcontextmenu,
    onbackgroundclick,
    onbackgroundcontextmenu,
    onrename,
    oncancelrename,
    ondragout,
  }: {
    /** The folder's items in their order, folders and files alike: the filmstrip is the folder. */
    entries: Entry[];
    selection: Set<string>;
    cursorIndex: number;
    cut: Set<string>;
    renaming: string | null;
    folderTint: (path: string) => string | null;
    onitemclick: (index: number, e: MouseEvent) => void;
    onopen: (entry: Entry) => void;
    onitemcontextmenu: (index: number, e: MouseEvent) => void;
    onbackgroundclick: () => void;
    onbackgroundcontextmenu: (e: MouseEvent) => void;
    onrename: (entry: Entry, newName: string) => void;
    oncancelrename: () => void;
    ondragout: (paths: string[]) => void;
  } = $props();

  let strip: HTMLDivElement;
  let scrollLeft = $state(0);
  let width = $state(0);
  let press: { x: number; y: number; entry: Entry } | null = null;

  // The strip opens at the cursor, once its width is known.
  let placed = false;
  $effect(() => {
    if (width > 0 && !placed) {
      placed = true;
      scrollToIndex(currentIndex);
    }
  });

  // The rename box lives in a tile, and only tiles in view exist: bring the item being
  // renamed into view, or the box would never appear and the keys stay off.
  $effect(() => {
    const at = renaming === null ? -1 : entries.findIndex((e) => e.path === renaming);
    if (at >= 0) scrollToIndex(at);
  });

  // A mouse wheel turns the strip, which has no vertical scroll of its own.
  function wheelAcross(node: HTMLElement) {
    const turn = (e: WheelEvent) => {
      if (Math.abs(e.deltaY) <= Math.abs(e.deltaX)) return;
      e.preventDefault();
      node.scrollLeft += e.deltaY;
    };
    node.addEventListener("wheel", turn, { passive: false });
    return { destroy: () => node.removeEventListener("wheel", turn) };
  }

  onDestroy(() => endPress());

  /** The item the large picture shows: the one the keyboard is on, or the first. */
  const current = $derived(entries[Math.max(cursorIndex, 0)] ?? null);
  const currentIndex = $derived(Math.max(cursorIndex, 0));

  // Only the tiles in the strip's window exist, so 10,000 photos cost as much as 20.
  const first = $derived(Math.max(0, Math.floor((scrollLeft - PAD_X) / STEP) - OVERSCAN));
  const last = $derived(
    Math.min(entries.length, Math.ceil((scrollLeft + width - PAD_X) / STEP) + OVERSCAN),
  );
  const shown = $derived(
    entries.slice(first, last).map((entry, i) => ({ entry, index: first + i })),
  );

  /** Pictures that failed (a file the shell cannot draw): shown as the file's icon instead. */
  let failed = $state<string | null>(null);
  /** The file the large picture is made for: it follows the cursor once that has rested, so holding an arrow key doesn't ask for a 512 px picture per step. */
  let bigPath = $state<string | null>(null);
  $effect(() => {
    const next = current && !current.isDir ? current.path : null;
    if (bigPath === null || next === null) {
      bigPath = next;
      return;
    }
    const timer = setTimeout(() => (bigPath = next), PREVIEW_SETTLE_MS);
    return () => clearTimeout(timer);
  });
  const bigSrc = $derived(bigPath ? thumbUrl(bigPath, PREVIEW_PX) : null);
  const bigFailed = $derived(failed === bigSrc);

  export function scrollToIndex(index: number) {
    const left = PAD_X + index * STEP;
    if (left < strip.scrollLeft) strip.scrollLeft = left - PAD_X;
    else if (left + TILE > strip.scrollLeft + width) strip.scrollLeft = left + TILE - width + PAD_X;
  }

  export function scrollToTop() {
    strip.scrollLeft = 0;
  }

  export function scrollElement(): HTMLElement {
    return strip;
  }

  /** The keys move along the strip, one item at a time. */
  export function columns(): number {
    return 1;
  }

  /** Puts keyboard focus on the strip, so a screen reader follows the cursor. */
  export function focusList() {
    const at = document.activeElement;
    if (!at || at === document.body || strip.contains(at)) strip.focus({ preventScroll: true });
  }

  function pointerDown(e: PointerEvent, entry: Entry) {
    if (e.button === 0 && renaming !== entry.path) {
      press = { x: e.clientX, y: e.clientY, entry };
      // Only a press can turn into a drag, so only then is the pointer watched.
      window.addEventListener("pointermove", pointerMove);
      window.addEventListener("pointerup", endPress);
    }
  }

  function endPress() {
    press = null;
    window.removeEventListener("pointermove", pointerMove);
    window.removeEventListener("pointerup", endPress);
  }

  function pointerMove(e: PointerEvent) {
    if (!press) return;
    if (!(e.buttons & 1)) {
      endPress();
      return;
    }
    if (Math.hypot(e.clientX - press.x, e.clientY - press.y) < DRAG_THRESHOLD) return;
    const { entry } = press;
    endPress();
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

<div class="gallery">
  <!-- Keys are handled at window level in +page.svelte, whichever element has focus. -->
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div
    class="stage"
    onclick={(e) => {
      if (e.target === e.currentTarget) onbackgroundclick();
    }}
    ondblclick={() => cursorIndex >= 0 && current && onopen(current)}
    oncontextmenu={(e) => {
      e.preventDefault();
      // The picture is the item the cursor is on; with no cursor it is the folder's own space.
      if (cursorIndex >= 0 && current) onitemcontextmenu(currentIndex, e);
      else onbackgroundcontextmenu(e);
    }}
  >
    {#if current}
      {#if current.isDir}
        <FolderIcon size={160} tint={folderTint(current.path)} />
      {:else if bigSrc && !bigFailed}
        <img
          class="big"
          src={bigSrc}
          alt={current.name}
          draggable="false"
          onerror={() => (failed = bigSrc)}
        />
      {:else}
        <FileIcon name={current.name} size={160} />
      {/if}
    {:else}
      <p class="empty">Nothing here</p>
    {/if}
  </div>

  {#if current}
    <p class="caption">
      <strong>{current.name}</strong>
      <span>
        {entries.length ? `${currentIndex + 1} of ${entries.length.toLocaleString()}` : ""}
        {#if !current.isDir}· {formatSize(current.size)}{/if}
        · {formatDate(current.modified)}
      </span>
    </p>
  {/if}

  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div
    class="strip"
    role="listbox"
    aria-label="Files"
    aria-orientation="horizontal"
    aria-multiselectable="true"
    aria-activedescendant={cursorIndex >= first && cursorIndex < last
      ? `film-file-${cursorIndex}`
      : undefined}
    tabindex="0"
    bind:this={strip}
    bind:clientWidth={width}
    use:wheelAcross
    onscroll={() => (scrollLeft = strip.scrollLeft)}
    onclick={(e) => {
      if (!(e.target as HTMLElement).closest(".tile")) onbackgroundclick();
    }}
    oncontextmenu={(e) => {
      e.preventDefault();
      if (!(e.target as HTMLElement).closest(".tile")) onbackgroundcontextmenu(e);
    }}
  >
    <div class="canvas" style:width="{PAD_X * 2 + entries.length * STEP}px">
      {#each shown as { entry, index } (entry.path)}
        {@const isSelected = selection.has(entry.path)}
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <div
          class="tile"
          class:selected={isSelected}
          class:here={index === currentIndex}
          class:cut={cut.has(entry.path)}
          style:left="{PAD_X + index * STEP}px"
          style:width="{TILE}px"
          style:height="{TILE}px"
          id="film-file-{index}"
          role="option"
          tabindex="-1"
          title={entry.name}
          aria-label={entry.name}
          aria-selected={isSelected}
          aria-setsize={entries.length}
          aria-posinset={index + 1}
          data-drop-path={entry.isDir ? entry.path : undefined}
          onclick={(e) => onitemclick(index, e)}
          ondblclick={() => onopen(entry)}
          onpointerdown={(e) => pointerDown(e, entry)}
          oncontextmenu={(e) => {
            e.preventDefault();
            e.stopPropagation();
            onitemcontextmenu(index, e);
          }}
        >
          {#if entry.isDir}
            <FolderIcon size={40} tint={folderTint(entry.path)} />
          {:else}
            <Thumbnail path={entry.path} name={entry.name} size={TILE - 8} />
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
          {/if}
        </div>
      {/each}
    </div>
  </div>
</div>

<style>
  .gallery {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-height: 0;
  }

  .stage {
    display: grid;
    flex: 1;
    place-items: center;
    min-height: 0;
    padding: var(--space-lg);
    overflow: hidden;
  }

  .big {
    max-width: 100%;
    max-height: 100%;
    object-fit: contain;
    border-radius: var(--radius-md);
    user-select: none;
  }

  .caption {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-xs) var(--space-md);
    justify-content: center;
    margin: 0;
    padding: 0 var(--space-lg) var(--space-sm);
    font-size: var(--font-size-caption);
    color: var(--text-secondary);
  }

  .caption strong {
    color: var(--text-primary);
    font-weight: var(--font-weight-semibold);
    overflow-wrap: anywhere;
  }

  .strip {
    flex: none;
    height: var(--size-filmstrip);
    overflow-x: auto;
    overflow-y: hidden;
    border-top: 1px solid var(--border-default);
  }

  .strip:focus-visible {
    outline: 2px solid var(--accent-default);
    outline-offset: -2px;
  }

  .canvas {
    position: relative;
    height: 100%;
  }

  .tile {
    position: absolute;
    top: 50%;
    display: grid;
    place-items: center;
    box-sizing: border-box;
    transform: translateY(-50%);
    border: 2px solid transparent;
    border-radius: var(--radius-md);
    overflow: hidden;
    cursor: default;
    user-select: none;
  }

  .tile:hover {
    background: var(--bg-hover);
  }

  .tile.selected {
    background: var(--bg-selected);
  }

  .tile.here {
    border-color: var(--accent-default);
  }

  .cut {
    opacity: 0.45;
  }

  .tile :global(img) {
    max-width: 100%;
    max-height: 100%;
  }

  .rename {
    position: absolute;
    right: 0;
    bottom: 0;
    left: 0;
    min-width: 0;
    padding: var(--space-2xs);
    border: 1px solid var(--accent-default);
    border-radius: var(--radius-sm);
    font: inherit;
    font-size: var(--font-size-caption);
    color: var(--text-primary);
    background: var(--bg-window);
    user-select: text;
  }

  .empty {
    margin: 0;
    color: var(--text-tertiary);
    font-size: var(--font-size-caption);
  }
</style>
