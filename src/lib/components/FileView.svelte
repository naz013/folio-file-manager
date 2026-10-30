<script lang="ts">
  import type { Entry } from "$lib/api/fs";
  import { formatDate, formatSize } from "$lib/format";
  import FolderIcon from "./FolderIcon.svelte";
  import FileIcon from "./FileIcon.svelte";
  import type { View } from "./Toolbar.svelte";

  // TODO(0.1): virtualize rendering to meet the 50k-items @ 60 fps budget.
  let {
    entries,
    view,
    selected = $bindable(),
    onopen,
  }: { entries: Entry[]; view: View; selected: string | null; onopen: (entry: Entry) => void } = $props();
</script>

{#if view === "grid"}
  <div class="grid">
    {#each entries as entry (entry.path)}
      <button
        class="tile"
        class:selected={selected === entry.path}
        onclick={() => (selected = entry.path)}
        ondblclick={() => onopen(entry)}>
        <span class="thumb">
          {#if entry.isDir}<FolderIcon size={64} />{:else}<FileIcon name={entry.name} size={56} />{/if}
        </span>
        <span class="name">{entry.name}</span>
      </button>
    {/each}
  </div>
{:else}
  <div class="list">
    <div class="row head">
      <span>Name</span><span>Date modified</span><span class="num">Size</span>
    </div>
    {#each entries as entry, i (entry.path)}
      <button
        class="row"
        class:zebra={i % 2 === 1}
        class:selected={selected === entry.path}
        onclick={() => (selected = entry.path)}
        ondblclick={() => onopen(entry)}>
        <span class="cell-name">
          {#if entry.isDir}<FolderIcon size={18} />{:else}<FileIcon name="" size={18} />{/if}
          <span class="ellipsis">{entry.name}</span>
        </span>
        <span class="muted">{formatDate(entry.modified)}</span>
        <span class="muted num">{entry.isDir ? "—" : formatSize(entry.size)}</span>
      </button>
    {/each}
  </div>
{/if}

<style>
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(132px, 1fr));
    gap: 10px 6px;
    padding: 20px 24px;
  }

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
    width: 112px;
    height: 88px;
    border-radius: 10px;
  }

  .tile.selected .thumb {
    background: var(--bg-hover);
  }

  .name {
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

  .list {
    padding: 8px 16px;
  }

  .row {
    display: grid;
    grid-template-columns: 1fr 190px 90px;
    align-items: center;
    gap: 12px;
    width: 100%;
    height: 32px;
    padding: 0 10px;
    border-radius: 7px;
    text-align: left;
  }

  .row.head {
    height: 28px;
    font-size: var(--font-size-caption);
    font-weight: var(--font-weight-semibold);
    color: var(--text-tertiary);
  }

  .row.zebra {
    background: var(--bg-subtle);
  }

  .row.selected {
    background: var(--accent-default);
    color: var(--text-on-accent);
  }

  .row.selected .muted {
    color: inherit;
  }

  .cell-name {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
  }

  .ellipsis {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .muted {
    color: var(--text-secondary);
    font-size: 12.5px;
  }

  .num {
    text-align: right;
  }
</style>
