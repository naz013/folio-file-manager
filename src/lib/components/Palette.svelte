<script lang="ts" module>
  export interface PaletteItem {
    /** What is shown, and matched against. */
    label: string;
    /** The place or shortcut, at the right. */
    detail?: string;
    kind: "folder" | "action";
    /** Position among recent folders (0 = latest), for ranking; absent for the rest. */
    recent?: number;
    run: () => void;
  }
</script>

<script lang="ts">
  import { onMount, tick } from "svelte";
  import { fuzzyScore } from "$lib/palette";

  let {
    items,
    onclose,
  }: {
    items: PaletteItem[];
    onclose: () => void;
  } = $props();

  let query = $state("");
  let chosen = $state(0);
  /** An item was run: it decides where focus goes, not the palette. */
  let ran = false;
  let input: HTMLInputElement;
  let list: HTMLElement;
  /** Where focus was, so closing gives it back. */
  const opener = document.activeElement as HTMLElement | null;

  onMount(() => {
    input.focus();
    return () => {
      if (!ran) opener?.focus?.();
    };
  });

  const MAX = 40;

  /** Recent folders first when nothing is typed; otherwise the best matches. */
  let results = $derived.by(() => {
    if (!query.trim()) {
      const recent = items
        .filter((i) => i.kind === "folder" && i.recent !== undefined)
        .sort((a, b) => (a.recent ?? 0) - (b.recent ?? 0))
        .slice(0, 8);
      return [...recent, ...items.filter((i) => i.kind === "action")].slice(0, MAX);
    }
    return items
      .map((item) => {
        const score = fuzzyScore(query, item.label);
        // Where you were lately ranks above the same match elsewhere.
        const boost = item.recent === undefined ? 0 : Math.max(0, 1.5 - item.recent * 0.15);
        return { item, score: score === null ? null : score + boost };
      })
      .filter((m): m is { item: PaletteItem; score: number } => m.score !== null)
      .sort((a, b) => b.score - a.score)
      .slice(0, MAX)
      .map((m) => m.item);
  });

  async function move(delta: number) {
    if (!results.length) return;
    chosen = (chosen + delta + results.length) % results.length;
    await tick();
    list.querySelector<HTMLElement>("[aria-selected=true]")?.scrollIntoView({ block: "nearest" });
  }

  function run(item: PaletteItem | undefined) {
    if (!item) return;
    ran = true;
    onclose();
    item.run();
  }

  function onkeydown(e: KeyboardEvent) {
    // Nothing behind the palette may act on keys while it is open.
    e.stopPropagation();
    if (e.key === "Escape") {
      e.preventDefault();
      onclose();
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      move(1);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      move(-1);
    } else if (e.key === "Enter") {
      // Enter that confirms an input method's text is not "go".
      if (e.isComposing) return;
      e.preventDefault();
      run(results[chosen]);
    } else if (e.key === "Tab") {
      e.preventDefault();
    }
  }
</script>

<div class="backdrop" role="presentation" onpointerdown={onclose}>
  <div
    class="palette"
    role="dialog"
    aria-modal="true"
    aria-label="Command palette"
    tabindex="-1"
    onpointerdown={(e) => {
      e.stopPropagation();
      // Clicking the list or the footer must not take focus from the input.
      if (e.target !== input) e.preventDefault();
    }}
    {onkeydown}
  >
    <input
      bind:this={input}
      bind:value={query}
      role="combobox"
      aria-expanded="true"
      aria-autocomplete="list"
      aria-haspopup="listbox"
      oninput={() => (chosen = 0)}
      aria-controls="palette-results"
      aria-activedescendant={results.length ? `palette-${chosen}` : undefined}
      placeholder="Go to a folder or run an action"
      spellcheck="false"
      autocomplete="off"
    />
    <ul id="palette-results" role="listbox" bind:this={list}>
      {#each results as item, i (item.kind + item.label + (item.detail ?? ""))}
        <!-- Keys are handled on the input (combobox pattern); the rows are for the pointer. -->
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <li
          id="palette-{i}"
          role="option"
          aria-selected={i === chosen}
          class:chosen={i === chosen}
          onpointermove={() => (chosen = i)}
          onclick={() => run(item)}
        >
          <span class="kind">{item.kind === "folder" ? "Folder" : "Action"}</span>
          <span class="label">{item.label}</span>
          {#if item.detail}<span class="detail">{item.detail}</span>{/if}
        </li>
      {:else}
        <li class="none" role="option" aria-disabled="true" aria-selected="false">
          Nothing matches “{query}”
        </li>
      {/each}
    </ul>
    <footer>↑ ↓ to choose · Enter to go · Esc to close</footer>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 65;
    display: grid;
    align-items: start;
    justify-items: center;
    padding-top: 12vh;
    background: var(--scrim);
  }

  .palette {
    display: grid;
    grid-template-rows: auto 1fr auto;
    width: min(560px, calc(100vw - 2 * var(--space-xl)));
    max-height: 60vh;
    border: 1px solid var(--border-default);
    border-radius: var(--radius-xl);
    background: var(--bg-window);
    box-shadow: var(--shadow-popover);
    overflow: hidden;
    outline: none;
  }

  input {
    padding: var(--space-md) var(--space-lg);
    border: 0;
    border-bottom: 1px solid var(--border-default);
    background: transparent;
    color: var(--text-primary);
    font: inherit;
    font-size: var(--font-size-title);
    outline: none;
  }

  ul {
    margin: 0;
    padding: var(--space-xs);
    overflow-y: auto;
    list-style: none;
  }

  li {
    display: flex;
    align-items: baseline;
    gap: var(--space-md);
    padding: var(--space-xs) var(--space-md);
    border-radius: var(--radius-md);
  }

  li.chosen {
    background: var(--bg-hover);
  }

  .kind {
    width: 3.5em;
    color: var(--text-tertiary);
    font-size: var(--font-size-caption);
  }

  .label {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .detail {
    max-width: 50%;
    overflow: hidden;
    color: var(--text-tertiary);
    font-size: var(--font-size-caption);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .none {
    color: var(--text-tertiary);
  }

  footer {
    padding: var(--space-xs) var(--space-lg);
    border-top: 1px solid var(--border-default);
    color: var(--text-secondary);
    font-size: var(--font-size-caption);
    text-align: center;
  }
</style>
