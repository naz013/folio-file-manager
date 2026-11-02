<script lang="ts" module>
  /** Matches the list's row height, a little tighter for a narrow column. */
  const ROW_H = 28;
  const PAD_Y = 4;
  /** Extra rows rendered above and below the viewport. */
  const OVERSCAN = 3;
  /** Pointer travel (px) before a press turns into a native drag. */
  const DRAG_THRESHOLD = 6;
</script>

<script lang="ts">
  import type { Entry } from "$lib/api/fs";
  import FileIcon from "./FileIcon.svelte";
  import FolderIcon from "./FolderIcon.svelte";
  import Icon from "./Icon.svelte";
  import Thumbnail from "./Thumbnail.svelte";

  let {
    entries,
    selected,
    highlight = null,
    cursorIndex = -1,
    active = false,
    cut,
    renaming = null,
    folderTint,
    thumbnails = false,
    label,
    onitemclick,
    onopen,
    onitemcontextmenu,
    onbackgroundclick,
    onrename,
    oncancelrename,
    ondragout,
  }: {
    entries: Entry[];
    /** Paths that are selected, drawn as the selection. */
    selected: Set<string>;
    /** The folder this column's neighbour on the right shows, marked as the way in. */
    highlight?: string | null;
    /** The item the keyboard is on (an index into `entries`) for screen readers; -1 for none. */
    cursorIndex?: number;
    /** The column the keyboard works in: the one for the open folder. */
    active?: boolean;
    cut?: Set<string>;
    renaming?: string | null;
    folderTint: (path: string) => string | null;
    /** Pictures of the files, which are asked for per file: only the open folder's column wants them. */
    thumbnails?: boolean;
    /** What a screen reader calls the column. */
    label: string;
    onitemclick: (index: number, e: MouseEvent) => void;
    onopen: (entry: Entry) => void;
    onitemcontextmenu?: (index: number, e: MouseEvent) => void;
    onbackgroundclick?: () => void;
    onrename?: (entry: Entry, newName: string) => void;
    oncancelrename?: () => void;
    /** Called once a press on an item moves far enough to become a drag. */
    ondragout?: (paths: string[]) => void;
  } = $props();

  let scroller: HTMLDivElement;
  let scrollTop = $state(0);
  let height = $state(0);
  let press: { x: number; y: number; entry: Entry } | null = null;

  const first = $derived(Math.max(0, Math.floor((scrollTop - PAD_Y) / ROW_H) - OVERSCAN));
  const last = $derived(
    Math.min(entries.length, Math.ceil((scrollTop + height - PAD_Y) / ROW_H) + OVERSCAN),
  );
  const shown = $derived(
    entries.slice(first, last).map((entry, i) => ({ entry, index: first + i })),
  );
  const cursorShown = $derived(active && cursorIndex >= first && cursorIndex < last);

  export function scrollToIndex(index: number) {
    const top = PAD_Y + index * ROW_H;
    if (top < scroller.scrollTop) scroller.scrollTop = top - PAD_Y;
    else if (top + ROW_H > scroller.scrollTop + height)
      scroller.scrollTop = top + ROW_H - height + PAD_Y;
  }

  export function scrollToTop() {
    scroller.scrollTop = 0;
  }

  export function scrollElement(): HTMLElement {
    return scroller;
  }

  /** Puts keyboard focus on the column, so a screen reader follows the cursor. */
  export function focusList() {
    const at = document.activeElement;
    if (!at || at === document.body || scroller.contains(at))
      scroller.focus({ preventScroll: true });
  }

  function pointerDown(e: PointerEvent, entry: Entry) {
    if (ondragout && e.button === 0 && renaming !== entry.path) {
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
    ondragout?.(selected.has(entry.path) ? [...selected] : [entry.path]);
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
    if (e.key === "Enter") onrename?.(entry, input.value);
    else if (e.key === "Escape") oncancelrename?.();
  }
</script>

<!-- Keys are handled at window level in +page.svelte, whichever element has focus. -->
<!-- svelte-ignore a11y_click_events_have_key_events -->
<div
  class="column"
  class:active
  role="listbox"
  aria-label={label}
  aria-multiselectable={active}
  aria-activedescendant={cursorShown ? `col-file-${cursorIndex}` : undefined}
  tabindex={active ? 0 : undefined}
  bind:this={scroller}
  bind:clientHeight={height}
  onscroll={() => (scrollTop = scroller.scrollTop)}
  onclick={(e) => {
    if (!(e.target as HTMLElement).closest(".row")) onbackgroundclick?.();
  }}
  oncontextmenu={(e) => {
    // Right-click on the empty space is for the page's own menu; items stop it below.
    if (!(e.target as HTMLElement).closest(".row")) e.preventDefault();
  }}
>
  <div class="canvas" style:height="{PAD_Y * 2 + entries.length * ROW_H}px">
    {#each shown as { entry, index } (entry.path)}
      {@const isSelected = selected.has(entry.path)}
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <div
        class="row"
        class:selected={isSelected}
        class:way={highlight === entry.path}
        class:cut={cut?.has(entry.path)}
        style:top="{PAD_Y + index * ROW_H}px"
        style:height="{ROW_H}px"
        id={active ? `col-file-${index}` : undefined}
        role="option"
        tabindex="-1"
        aria-selected={isSelected || highlight === entry.path}
        data-drop-path={entry.isDir ? entry.path : undefined}
        onclick={(e) => onitemclick(index, e)}
        ondblclick={() => onopen(entry)}
        onpointerdown={(e) => pointerDown(e, entry)}
        oncontextmenu={(e) => {
          e.preventDefault();
          e.stopPropagation();
          onitemcontextmenu?.(index, e);
        }}
      >
        <span class="icon">
          {#if entry.isDir}<FolderIcon
              size={16}
              tint={folderTint(entry.path)}
            />{:else if thumbnails}<Thumbnail path={entry.path} name="" size={16} />{:else}<FileIcon
              name={entry.name}
              size={16}
            />{/if}
        </span>
        {#if renaming === entry.path}
          <input
            class="rename"
            aria-label="New name"
            value={entry.name}
            use:renameInput={entry}
            onkeydown={(e) => renameKey(e, entry)}
            onblur={(e) => onrename?.(entry, e.currentTarget.value)}
            onclick={(e) => e.stopPropagation()}
            ondblclick={(e) => e.stopPropagation()}
          />
        {:else}
          <span class="name">{entry.name}</span>
        {/if}
        {#if entry.isDir}<span class="more" aria-hidden="true"
            ><Icon name="chevronRight" size={12} stroke={2} /></span
          >{/if}
      </div>
    {:else}
      <p class="empty">Empty</p>
    {/each}
  </div>
</div>

<style>
  .column {
    flex: none;
    width: var(--size-column);
    overflow-x: hidden;
    overflow-y: auto;
    border-right: 1px solid var(--border-default);
  }

  .column:focus-visible {
    outline: 2px solid var(--accent-default);
    outline-offset: -2px;
  }

  .canvas {
    position: relative;
  }

  .row {
    position: absolute;
    left: var(--space-xs);
    right: var(--space-xs);
    display: flex;
    align-items: center;
    gap: var(--space-sm);
    padding: 0 var(--space-sm);
    border-radius: var(--radius-md);
    color: var(--text-primary);
    font-size: var(--font-size-body);
    cursor: default;
    user-select: none;
  }

  .row:hover {
    background: var(--bg-hover);
  }

  /* The way in, in a column the keyboard isn't in, is marked softer than a selection. */
  .row.way {
    background: var(--bg-selected);
  }

  .row.selected {
    background: var(--bg-selected);
  }

  .active .row.selected {
    background: var(--accent-default);
    color: var(--text-on-accent);
  }

  .cut {
    opacity: 0.45;
  }

  .icon {
    display: grid;
    flex: none;
    place-items: center;
    width: var(--space-xl);
  }

  .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .more {
    display: grid;
    flex: none;
    place-items: center;
    opacity: 0.6;
  }

  .rename {
    flex: 1;
    min-width: 0;
    padding: var(--space-2xs) var(--space-xs);
    border: 1px solid var(--accent-default);
    border-radius: var(--radius-sm);
    outline: 3px solid var(--accent-subtle);
    font: inherit;
    color: var(--text-primary);
    background: var(--bg-window);
    user-select: text;
  }

  .empty {
    margin: var(--space-lg) var(--space-md);
    color: var(--text-tertiary);
    font-size: var(--font-size-caption);
  }
</style>
