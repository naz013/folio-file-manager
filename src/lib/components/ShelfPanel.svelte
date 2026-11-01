<script lang="ts">
  import { parentOf, type ShelfItem } from "$lib/api/fs";
  import { formatSize } from "$lib/format";
  import Thumbnail from "./Thumbnail.svelte";
  import FolderIcon from "./FolderIcon.svelte";

  let {
    items,
    open,
    ontoggle,
    onpaste,
    onremove,
    onmode,
    onclear,
    ondrag,
  }: {
    items: ShelfItem[];
    open: boolean;
    ontoggle: () => void;
    /** Paste these items into the folder shown. */
    onpaste: (items: ShelfItem[]) => void;
    /** Take these off the shelf; the files stay where they are. */
    onremove: (paths: string[]) => void;
    onmode: (item: ShelfItem, mode: "copy" | "cut") => void;
    onclear: () => void;
    /** An item was pulled out of the panel: start dragging it. */
    ondrag: (item: ShelfItem) => void;
  } = $props();

  /** The shelf grouped by the folder each item came from, in the order they first appear. */
  let groups = $derived.by(() => {
    const order: string[] = [];
    const by: Record<string, ShelfItem[]> = {};
    for (const item of items) {
      const dir = parentOf(item.path) ?? item.path;
      if (!(dir in by)) {
        by[dir] = [];
        order.push(dir);
      }
      by[dir].push(item);
    }
    return order.map((dir) => ({ dir, items: by[dir] }));
  });

  const nameOf = (path: string) => path.split(/[\\/]/).filter(Boolean).pop() ?? path;

  /** A press on a row that moves far enough pulls the item out as a drag. */
  let press: { x: number; y: number; item: ShelfItem } | null = null;

  function down(e: PointerEvent, item: ShelfItem) {
    // A press on a button is a click, not the start of a drag.
    if ((e.target as HTMLElement).closest("button")) return;
    if (e.button === 0 && item.exists) press = { x: e.clientX, y: e.clientY, item };
  }

  function move(e: PointerEvent) {
    if (!press) return;
    if (!(e.buttons & 1)) {
      press = null;
      return;
    }
    if (Math.hypot(e.clientX - press.x, e.clientY - press.y) < 5) return;
    const { item } = press;
    press = null;
    ondrag(item);
  }
</script>

<svelte:window onpointermove={move} onpointerup={() => (press = null)} />

{#if open}
  <section class="panel" aria-label="Clipboard shelf">
    <header>
      <strong>Shelf</strong>
      <span class="count">{items.length} {items.length === 1 ? "item" : "items"}</span>
      <span class="grow"></span>
      <button
        class="primary"
        disabled={!items.some((i) => i.exists)}
        onclick={() => onpaste(items)}
        title="Paste everything here (Ctrl+V)"
      >
        Paste all
      </button>
      <button onclick={onclear} disabled={!items.length}>Clear</button>
      <button onclick={ontoggle} aria-label="Close the shelf" title="Close (the items stay)">
        ✕
      </button>
    </header>

    <div class="body">
      {#each groups as group (group.dir)}
        <div class="group">
          <div class="group-head">
            <span class="dir" title={group.dir}>{nameOf(group.dir)}</span>
            <button
              onclick={() => onpaste(group.items)}
              aria-label="Paste the items from {nameOf(group.dir)}"
            >
              Paste
            </button>
            <button
              onclick={() => onremove(group.items.map((i) => i.path))}
              aria-label="Remove the items from {nameOf(group.dir)} from the shelf"
            >
              Remove
            </button>
          </div>
          {#each group.items as item (item.path)}
            <div
              class="row"
              role="group"
              class:missing={!item.exists}
              onpointerdown={(e) => down(e, item)}
            >
              <span class="thumb">
                {#if item.isDir}<FolderIcon size={22} />{:else}<Thumbnail
                    path={item.path}
                    name={nameOf(item.path)}
                    size={22}
                  />{/if}
              </span>
              <span class="name" title={item.exists ? item.path : "Not found: " + item.path}>
                {nameOf(item.path)}
              </span>
              <span class="size">{item.exists && !item.isDir ? formatSize(item.size) : ""}</span>
              <button
                class="mode"
                class:cut={item.mode === "cut"}
                aria-label="{item.mode === 'cut'
                  ? 'Will move'
                  : 'Will copy'}: switch to {item.mode === 'cut' ? 'copy' : 'move'}"
                onclick={() => onmode(item, item.mode === "cut" ? "copy" : "cut")}
                title="Click to switch between move and copy"
              >
                {item.mode === "cut" ? "Move" : "Copy"}
              </button>
              {#if item.exists}
                <button
                  class="paste"
                  onclick={() => onpaste([item])}
                  aria-label="Paste {nameOf(item.path)} here"
                  title="Paste only this item here"
                >
                  Paste
                </button>
              {:else}
                <span class="sr">(not found)</span>
              {/if}
              <button
                class="x"
                onclick={() => onremove([item.path])}
                aria-label="Remove {nameOf(item.path)} from the shelf"
              >
                ✕
              </button>
            </div>
          {/each}
        </div>
      {:else}
        <p class="empty">Ctrl+X or Ctrl+C adds items here. Ctrl+V pastes them all.</p>
      {/each}
    </div>
  </section>
{:else if items.length}
  <button
    class="fab"
    onclick={ontoggle}
    aria-label="Open the clipboard shelf ({items.length} {items.length === 1 ? 'item' : 'items'})"
  >
    Shelf
    <span class="bubble">{items.length}</span>
  </button>
{/if}

<style>
  .panel,
  .fab {
    position: fixed;
    right: var(--space-lg);
    bottom: calc(var(--space-xl) * 2);
    z-index: 45;
  }

  .fab {
    display: inline-flex;
    align-items: center;
    gap: var(--space-sm);
    padding: var(--space-sm) var(--space-md);
    border: 1px solid var(--border-default);
    border-radius: 999px;
    background: var(--bg-window);
    box-shadow: var(--shadow-popover);
    font-weight: var(--font-weight-medium);
  }

  .bubble {
    min-width: 20px;
    padding: 0 var(--space-xs);
    border-radius: 999px;
    background: var(--accent-default);
    color: var(--text-on-accent);
    font-size: var(--font-size-caption);
    text-align: center;
  }

  .panel {
    display: grid;
    grid-template-rows: auto 1fr;
    width: min(380px, calc(100vw - 2 * var(--space-lg)));
    max-height: min(480px, 70vh);
    border: 1px solid var(--border-default);
    border-radius: var(--radius-xl);
    background: var(--bg-window);
    box-shadow: var(--shadow-popover);
    overflow: hidden;
  }

  header {
    display: flex;
    align-items: center;
    gap: var(--space-sm);
    padding: var(--space-sm) var(--space-md);
    border-bottom: 1px solid var(--border-default);
  }

  .grow {
    flex: 1;
  }

  .count {
    color: var(--text-secondary);
    font-size: var(--font-size-caption);
  }

  header button,
  .group-head button {
    padding: var(--space-2xs) var(--space-sm);
    border-radius: var(--radius-sm);
    background: var(--bg-control);
    font-size: var(--font-size-caption);
  }

  header button:disabled {
    color: var(--text-tertiary);
  }

  .primary {
    background: var(--accent-default);
    color: var(--text-on-accent);
  }

  .primary:disabled {
    background: var(--bg-control);
  }

  .body {
    overflow-y: auto;
    padding: var(--space-xs);
  }

  .group-head {
    display: flex;
    align-items: center;
    gap: var(--space-xs);
    padding: var(--space-xs) var(--space-sm);
    color: var(--text-tertiary);
    font-size: var(--font-size-caption);
  }

  .dir {
    flex: 1;
    overflow: hidden;
    font-weight: var(--font-weight-semibold);
    text-overflow: ellipsis;
    text-transform: uppercase;
    white-space: nowrap;
  }

  .row {
    display: flex;
    align-items: center;
    gap: var(--space-sm);
    padding: var(--space-2xs) var(--space-sm);
    border-radius: var(--radius-md);
  }

  .row:hover {
    background: var(--bg-hover);
  }

  .row.missing {
    opacity: 0.45;
  }

  .thumb {
    display: grid;
    place-items: center;
    width: 22px;
    height: 22px;
  }

  .name {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .row.missing .name {
    text-decoration: line-through;
  }

  .size {
    color: var(--text-tertiary);
    font-size: var(--font-size-caption);
    font-variant-numeric: tabular-nums;
  }

  .mode {
    padding: 0 var(--space-sm);
    border-radius: 999px;
    background: var(--accent-subtle);
    color: var(--text-primary);
    font-size: var(--font-size-caption);
  }

  .mode.cut {
    background: var(--bg-control);
  }

  .paste {
    padding: 0 var(--space-xs);
    border-radius: var(--radius-sm);
    color: var(--text-secondary);
    font-size: var(--font-size-caption);
  }

  .paste:hover {
    background: var(--bg-hover);
  }

  .sr {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
  }

  .x {
    padding: 0 var(--space-xs);
    color: var(--text-tertiary);
  }

  .empty {
    margin: var(--space-lg);
    color: var(--text-secondary);
    font-size: var(--font-size-caption);
  }
</style>
