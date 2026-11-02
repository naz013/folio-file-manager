<script lang="ts">
  import { onMount } from "svelte";
  import type { SearchQuery } from "$lib/api/fs";
  import Icon from "./Icon.svelte";

  let {
    folder,
    status,
    onquery,
    onstop,
    onclose,
  }: {
    /** Name of the folder searched, with everything below it. */
    folder: string;
    /** How the search is going; null before one has started. */
    status: { running: boolean; found: number; truncated: boolean } | null;
    /** The search to run, or null when there is nothing to look for. */
    onquery: (query: SearchQuery | null) => void;
    onstop: () => void;
    onclose: () => void;
  } = $props();

  const UNITS = [
    { label: "KB", bytes: 1024 },
    { label: "MB", bytes: 1024 ** 2 },
    { label: "GB", bytes: 1024 ** 3 },
  ];

  let text = $state("");
  let min = $state("");
  let max = $state("");
  let unit = $state(1);
  let input: HTMLInputElement;

  onMount(() => input.focus());

  /** Puts the cursor back in the name box, with its text selected. */
  export function focus() {
    input.focus();
    input.select();
  }

  /** Bytes for a size box, or null when it is empty or not a number. */
  function bytes(value: string): number | null {
    const n = Number(value);
    return value.trim() !== "" && Number.isFinite(n) && n >= 0
      ? Math.round(n * UNITS[unit].bytes)
      : null;
  }

  // Search once typing pauses; a change starts a new search and ends the one running.
  $effect(() => {
    const query: SearchQuery = {
      text,
      minSize: bytes(min),
      maxSize: bytes(max),
      hidden: false,
    };
    const empty = query.text.trim() === "" && query.minSize === null && query.maxSize === null;
    const timer = setTimeout(() => onquery(empty ? null : query), 250);
    return () => clearTimeout(timer);
  });

  function onkeydown(e: KeyboardEvent) {
    // Only Escape is handled here; other keys, tab shortcuts included, go on to the page.
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      onclose();
    }
  }
</script>

<div class="bar" role="search" aria-label="Search in {folder}">
  <Icon name="search" size={15} />
  <input
    bind:this={input}
    bind:value={text}
    class="text"
    spellcheck="false"
    autocomplete="off"
    placeholder="Search in {folder} and its subfolders (use * and ?)"
    aria-label="Name"
    {onkeydown}
  />
  <label class="size">
    Larger than
    <input bind:value={min} inputmode="decimal" aria-label="Minimum size" {onkeydown} />
  </label>
  <label class="size">
    Smaller than
    <input bind:value={max} inputmode="decimal" aria-label="Maximum size" {onkeydown} />
  </label>
  <select bind:value={unit} aria-label="Size unit" {onkeydown}>
    {#each UNITS as u, i (u.label)}
      <option value={i}>{u.label}</option>
    {/each}
  </select>
  <span class="status" role="status" aria-live="polite">
    {#if status}
      {#if status.running}
        Searching… {status.found.toLocaleString()} found
      {:else if status.truncated}
        {status.found.toLocaleString()}+ found. Narrow the search to see the rest.
      {:else}
        {status.found.toLocaleString()} found
      {/if}
    {/if}
  </span>
  {#if status?.running}
    <button class="stop" onclick={onstop}>Stop</button>
  {/if}
  <button class="close" onclick={onclose} aria-label="Close search" title="Close search (Esc)">
    <Icon name="x" size={14} />
  </button>
</div>

<style>
  .bar {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: var(--space-sm);
    padding: var(--space-xs) var(--space-md);
    border-bottom: 1px solid var(--border-default);
    color: var(--text-secondary);
    font-size: var(--font-size-caption);
  }

  input,
  select {
    height: var(--size-control-sm);
    padding: 0 var(--space-sm);
    border: 1px solid var(--border-default);
    border-radius: var(--radius-md);
    background: var(--bg-control);
    color: var(--text-primary);
  }

  input:focus-visible,
  select:focus-visible {
    outline: 2px solid var(--accent-default);
    outline-offset: -1px;
  }

  .text {
    flex: 1 1 calc(var(--space-3xl) * 7);
    min-width: calc(var(--space-3xl) * 5);
  }

  .size {
    display: inline-flex;
    align-items: center;
    gap: var(--space-xs);
  }

  .size input {
    width: calc(var(--space-3xl) * 2);
  }

  .status {
    flex: 0 1 auto;
    margin-left: auto;
    color: var(--text-tertiary);
  }

  .stop,
  .close {
    height: var(--size-control-sm);
    padding: 0 var(--space-sm);
    border-radius: var(--radius-md);
    color: var(--text-secondary);
  }

  .stop:hover,
  .close:hover {
    background: var(--bg-hover);
  }
</style>
