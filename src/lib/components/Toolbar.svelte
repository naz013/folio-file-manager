<script lang="ts">
  import Icon from "./Icon.svelte";
  import { pathSegments } from "$lib/api/fs";

  export type View = "grid" | "list";

  let {
    path,
    view = $bindable(),
    canBack,
    canForward,
    onback,
    onforward,
    onnavigate,
  }: {
    path: string;
    view: View;
    canBack: boolean;
    canForward: boolean;
    onback: () => void;
    onforward: () => void;
    onnavigate: (path: string) => void;
  } = $props();

  let segments = $derived(pathSegments(path));
</script>

<header class="toolbar">
  <button class="icon-btn" disabled={!canBack} onclick={onback} aria-label="Back">
    <Icon name="chevronLeft" />
  </button>
  <button class="icon-btn" disabled={!canForward} onclick={onforward} aria-label="Forward">
    <Icon name="chevronRight" />
  </button>

  <nav class="crumbs">
    {#each segments as seg, i}
      {#if i > 0}<span class="sep"><Icon name="chevronRight" size={12} stroke={2} /></span>{/if}
      <button class="crumb" class:last={i === segments.length - 1} onclick={() => onnavigate(seg.path)}>
        {seg.name}
      </button>
    {/each}
  </nav>

  <div class="switcher" role="group" aria-label="View">
    <button class:active={view === "grid"} onclick={() => (view = "grid")} aria-label="Grid view">
      <Icon name="grid" size={15} />
    </button>
    <button class:active={view === "list"} onclick={() => (view = "list")} aria-label="List view">
      <Icon name="list" size={15} />
    </button>
  </div>
</header>

<style>
  .toolbar {
    display: flex;
    align-items: center;
    gap: 6px;
    height: 52px;
    padding: 0 12px;
    border-bottom: 1px solid var(--border-default);
  }

  .icon-btn {
    display: grid;
    place-items: center;
    width: 30px;
    height: 30px;
    border-radius: var(--radius-md);
    color: var(--text-secondary);
  }

  .icon-btn:hover:not(:disabled) {
    background: var(--bg-hover);
  }

  .icon-btn:disabled {
    color: var(--text-tertiary);
  }

  .crumbs {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 4px;
    min-width: 0;
    padding-left: 6px;
    overflow: hidden;
  }

  .sep {
    display: flex;
    color: var(--text-tertiary);
  }

  .crumb {
    padding: 4px 6px;
    border-radius: var(--radius-sm);
    color: var(--text-secondary);
    white-space: nowrap;
  }

  .crumb:hover {
    background: var(--bg-hover);
  }

  .crumb.last {
    color: var(--text-primary);
    font-weight: var(--font-weight-semibold);
  }

  .switcher {
    display: flex;
    gap: 2px;
    padding: 2px;
    border-radius: 9px;
    background: var(--bg-control);
  }

  .switcher button {
    display: grid;
    place-items: center;
    width: 32px;
    height: 26px;
    border-radius: 7px;
    color: var(--text-secondary);
  }

  .switcher button.active {
    background: var(--bg-window);
    color: var(--text-primary);
    box-shadow: var(--shadow-raised);
  }
</style>
