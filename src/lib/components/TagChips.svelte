<script lang="ts">
  import type { Tag } from "$lib/api/fs";
  import { tagColor } from "$lib/tags";

  let {
    tags,
    active,
    ontoggle,
    onclear,
  }: {
    /** The tags that can filter this folder: those on its items, and any filter in use. */
    tags: Tag[];
    /** Ids of the tags the list is filtered by. */
    active: number[];
    ontoggle: (id: number) => void;
    onclear: () => void;
  } = $props();
</script>

<div class="chips" role="group" aria-label="Filter by tag">
  {#each tags as tag (tag.id)}
    <button
      class="chip"
      class:on={active.includes(tag.id)}
      aria-pressed={active.includes(tag.id)}
      onclick={() => ontoggle(tag.id)}
    >
      <span class="dot" style:background={tagColor(tag.color)}></span>
      {tag.name}
    </button>
  {/each}
  {#if active.length}
    <button class="clear" onclick={onclear}>Clear filter</button>
  {/if}
</div>

<style>
  .chips {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-xs);
    padding: var(--space-xs) var(--space-md);
    border-bottom: 1px solid var(--border-default);
  }

  .chip {
    display: inline-flex;
    align-items: center;
    gap: var(--space-xs);
    padding: var(--space-2xs) var(--space-sm);
    border: 1px solid var(--border-default);
    border-radius: 999px;
    color: var(--text-secondary);
    font-size: var(--font-size-caption);
  }

  .chip:hover {
    background: var(--bg-hover);
  }

  .chip.on {
    border-color: var(--accent-default);
    background: var(--accent-subtle);
    color: var(--text-primary);
  }

  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
  }

  .clear {
    padding: var(--space-2xs) var(--space-sm);
    color: var(--text-secondary);
    font-size: var(--font-size-caption);
    text-decoration: underline;
  }
</style>
