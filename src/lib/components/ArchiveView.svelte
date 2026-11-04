<script lang="ts">
  import { SvelteSet } from "svelte/reactivity";
  import { listArchive } from "$lib/api/fs";
  import {
    buildTree,
    counts,
    flatten,
    folderKeys,
    type ArchiveListing,
    type Node,
  } from "$lib/archive";
  import { formatSize } from "$lib/format";

  let { path }: { path: string } = $props();

  /** How long a file must stay shown before its list is read. */
  const WAIT_MS = 150;
  const ROW_H = 26;
  /** The rows that stay in the page above and below the ones in view. */
  const OVERSCAN = 8;
  /** An archive with no more lines than this starts with every folder open. */
  const OPEN_ALL_UP_TO = 300;

  // Never changed after they are made, so not made reactive node by node.
  let listing = $state.raw<ArchiveListing | null>(null);
  let failed = $state<string | null>(null);
  let tree = $state.raw<Node[]>([]);
  /** The keys of the folders drawn open. */
  const open = new SvelteSet<string>();

  function openOnly(keys: string[]) {
    open.clear();
    for (const k of keys) open.add(k);
  }
  let current = $state(0);
  let top = $state(0);
  let height = $state(300);
  let box: HTMLElement | undefined = $state();

  $effect(() => {
    const file = path;
    let live = true;
    listing = null;
    failed = null;
    // A moment's wait, so moving through archives in Quick Look with the arrows asks only for
    // the one it stops on.
    const timer = setTimeout(() => {
      listArchive(file)
        .then((l) => {
          if (!live) return;
          listing = l;
          tree = buildTree(l.entries);
          const keys = folderKeys(tree);
          openOnly(l.entries.length <= OPEN_ALL_UP_TO ? keys : []);
          current = 0;
        })
        .catch((e) => live && (failed = String(e)));
    }, WAIT_MS);
    return () => {
      live = false;
      clearTimeout(timer);
    };
  });

  const rows = $derived(flatten(tree, open));
  const first = $derived(Math.max(0, Math.floor(top / ROW_H) - OVERSCAN));
  const last = $derived(Math.min(rows.length, Math.ceil((top + height) / ROW_H) + OVERSCAN));
  const shown = $derived(rows.slice(first, last));
  const total = $derived(counts(tree));

  function toggle(index: number) {
    const row = rows[index];
    if (!row?.node.isDir) return;
    if (!open.delete(row.node.key)) open.add(row.node.key);
  }

  function reveal(index: number) {
    current = Math.max(0, Math.min(rows.length - 1, index));
    if (!box) return;
    const y = current * ROW_H;
    if (y < box.scrollTop) box.scrollTop = y;
    else if (y + ROW_H > box.scrollTop + box.clientHeight) {
      box.scrollTop = y + ROW_H - box.clientHeight;
    }
  }

  function onkeydown(e: KeyboardEvent) {
    const row = rows[current];
    switch (e.key) {
      case "ArrowDown":
        reveal(current + 1);
        break;
      case "ArrowUp":
        reveal(current - 1);
        break;
      case "Home":
        reveal(0);
        break;
      case "End":
        reveal(rows.length - 1);
        break;
      case "PageDown":
        reveal(current + Math.max(1, Math.floor(height / ROW_H) - 1));
        break;
      case "PageUp":
        reveal(current - Math.max(1, Math.floor(height / ROW_H) - 1));
        break;
      case "ArrowRight":
        if (row?.node.isDir && !row.open) toggle(current);
        else reveal(current + 1);
        break;
      case "ArrowLeft":
        if (row?.node.isDir && row.open) {
          toggle(current);
        } else if (row && row.depth > 0) {
          // To the folder it is in: the nearest line above that is one level up.
          let i = current - 1;
          while (i > 0 && rows[i].depth >= row.depth) i--;
          reveal(i);
        }
        break;
      case "Enter":
      case " ":
        toggle(current);
        break;
      default:
        return;
    }
    // Quick Look moves between files with the arrows: inside the list they are for the list.
    e.preventDefault();
    e.stopPropagation();
  }

  const size = (n: Node) => (n.isDir && !n.files ? "" : formatSize(n.size));
</script>

<div class="archive">
  {#if failed}
    <p class="note">{failed}</p>
  {:else if !listing}
    <p class="note">Loading…</p>
  {:else}
    <!-- Space and Enter on these buttons are the buttons': Quick Look must not take them. -->
    <header
      role="presentation"
      onkeydown={(e) => {
        if (e.target instanceof HTMLButtonElement && (e.key === " " || e.key === "Enter")) {
          e.stopPropagation();
        }
      }}
    >
      <span class="what">
        {listing.format.toUpperCase()} ·
        {total.files} file{total.files === 1 ? "" : "s"}{#if total.folders}, {total.folders}
          folder{total.folders === 1 ? "" : "s"}{/if} · {formatSize(total.bytes)} unpacked
      </span>
      <span class="actions">
        <button onclick={() => openOnly(folderKeys(tree))}>Open all</button>
        <button onclick={() => openOnly([])}>Close all</button>
      </span>
    </header>
    {#if listing.message}
      <p class="banner" class:danger={listing.status === "damaged"} role="status">
        {listing.message}
      </p>
    {/if}
    {#if listing.truncated}
      <p class="banner" role="status">
        Showing the first {listing.entries.length.toLocaleString()}{#if listing.total}
          of {listing.total.toLocaleString()}{/if} entries
      </p>
    {/if}
    {#if rows.length}
      <div
        bind:this={box}
        class="list"
        role="tree"
        tabindex="0"
        aria-label="Contents of the archive"
        aria-activedescendant={current >= first && current < last ? `arch-${current}` : undefined}
        bind:clientHeight={height}
        onscroll={(e) => (top = e.currentTarget.scrollTop)}
        {onkeydown}
      >
        <div class="space" style:height="{rows.length * ROW_H}px">
          {#each shown as row, i (row.node.key)}
            {@const index = first + i}
            <!-- svelte-ignore a11y_click_events_have_key_events -->
            <div
              id="arch-{index}"
              class="row"
              class:current={index === current}
              role="treeitem"
              tabindex="-1"
              aria-level={row.depth + 1}
              aria-posinset={index + 1}
              aria-setsize={rows.length}
              aria-selected={index === current}
              aria-expanded={row.node.isDir ? row.open : undefined}
              style:top="{index * ROW_H}px"
              onclick={() => {
                current = index;
                toggle(index);
              }}
            >
              <span class="name" style:padding-left="{row.depth * 16 + 8}px">
                <span class="twisty" aria-hidden="true">
                  {row.node.isDir ? (row.open ? "▾" : "▸") : ""}
                </span>
                <span class="label" class:folder={row.node.isDir} title={row.node.key}>
                  {row.node.name}{#if row.node.encrypted}<span class="lock"> · protected</span>{/if}
                </span>
              </span>
              <span class="size">{size(row.node)}</span>
              <span class="date">{row.node.modified ?? ""}</span>
            </div>
          {/each}
        </div>
      </div>
    {:else if listing.status === "ok"}
      <p class="note">This archive is empty</p>
    {/if}
  {/if}
</div>

<style>
  .archive {
    display: flex;
    flex-direction: column;
    width: 100%;
    height: 100%;
    min-height: 0;
    color: var(--text-primary);
    font-size: var(--font-size-body);
  }

  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-md);
    padding: var(--space-xs) var(--space-md);
    border-bottom: 1px solid var(--border-default);
    color: var(--text-secondary);
    font-size: var(--font-size-caption);
  }

  .actions {
    display: flex;
    gap: var(--space-xs);
  }

  .actions button {
    padding: 2px var(--space-sm);
    border-radius: var(--radius-sm);
    color: var(--text-primary);
    background: var(--bg-control);
  }

  .banner {
    margin: 0;
    padding: var(--space-xs) var(--space-md);
    border-bottom: 1px solid var(--border-default);
    background: color-mix(in srgb, var(--status-warning) 14%, transparent);
    font-size: var(--font-size-caption);
  }

  .banner.danger {
    background: color-mix(in srgb, var(--status-danger) 14%, transparent);
  }

  .note {
    margin: auto;
    padding: var(--space-lg);
    color: var(--text-secondary);
    text-align: center;
  }

  .list {
    flex: 1;
    min-height: 0;
    overflow: auto;
    outline: none;
  }

  .list:focus-visible {
    outline: 2px solid var(--accent-default);
    outline-offset: -2px;
  }

  .space {
    position: relative;
    min-width: max-content;
  }

  .row {
    position: absolute;
    left: 0;
    right: 0;
    display: grid;
    grid-template-columns: minmax(180px, 1fr) 90px 130px;
    align-items: center;
    gap: var(--space-md);
    height: 26px;
    padding-right: var(--space-md);
    cursor: default;
  }

  .row:hover {
    background: var(--bg-hover);
  }

  .row.current {
    background: var(--bg-selected, var(--bg-hover));
  }

  .name {
    display: flex;
    align-items: center;
    min-width: 0;
  }

  .twisty {
    flex: none;
    width: 14px;
    color: var(--text-tertiary);
  }

  .label {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .label.folder {
    font-weight: var(--font-weight-semibold);
  }

  .lock {
    color: var(--status-warning);
    font-weight: normal;
  }

  .size,
  .date {
    color: var(--text-secondary);
    font-size: var(--font-size-caption);
    white-space: nowrap;
  }

  .size {
    text-align: right;
  }
</style>
