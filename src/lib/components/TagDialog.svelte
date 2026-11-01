<script lang="ts">
  import { onMount } from "svelte";
  import type { Tag } from "$lib/api/fs";
  import { tagColor, TAG_COLORS } from "$lib/tags";

  let {
    tags,
    have,
    count,
    ontoggle,
    oncreate,
    ondelete,
    onclose,
  }: {
    tags: Tag[];
    /** For each tag id: whether all, some or none of the selected items have it. */
    have: Record<number, "all" | "some" | "none">;
    /** How many items are selected. */
    count: number;
    ontoggle: (id: number) => void;
    /** Resolves to an error message, or null when the tag was added. */
    oncreate: (name: string, color: string | null) => Promise<string | null>;
    ondelete: (id: number) => void;
    onclose: () => void;
  } = $props();

  let dialog: HTMLElement;
  /** Where focus was, so closing the dialog gives it back. */
  const opener = document.activeElement as HTMLElement | null;
  onMount(() => {
    dialog.focus();
    return () => opener?.focus?.();
  });

  let name = $state("");
  let color = $state<string | null>(null);
  let error = $state("");

  async function add() {
    error = (await oncreate(name, color)) ?? "";
    if (!error) name = "";
  }

  function onkeydown(e: KeyboardEvent) {
    // Nothing behind the dialog may act on keys while it is open.
    e.stopPropagation();
    if (e.key === "Escape") {
      e.preventDefault();
      onclose();
    }
  }
</script>

<div class="backdrop" role="presentation" onpointerdown={onclose}>
  <div
    bind:this={dialog}
    class="dialog"
    role="dialog"
    aria-modal="true"
    aria-labelledby="tags-title"
    tabindex="-1"
    onpointerdown={(e) => e.stopPropagation()}
    {onkeydown}
  >
    <header>
      <h2 id="tags-title">Tags for {count === 1 ? "1 item" : `${count} items`}</h2>
      <button class="done" onclick={onclose}>Done</button>
    </header>

    <ul>
      {#each tags as tag (tag.id)}
        <li>
          <button
            class="tag"
            role="checkbox"
            aria-checked={have[tag.id] === "all" ? true : have[tag.id] === "some" ? "mixed" : false}
            onclick={() => ontoggle(tag.id)}
          >
            <span class="box" data-state={have[tag.id]} aria-hidden="true">
              {have[tag.id] === "all" ? "✓" : have[tag.id] === "some" ? "–" : ""}
            </span>
            <span class="dot" style:background={tagColor(tag.color)}></span>
            <span class="label">{tag.name}</span>
            {#if tag.id <= 7}<kbd>{tag.id}</kbd>{/if}
          </button>
          {#if tag.id > 7}
            <button
              class="remove"
              aria-label="Delete the tag {tag.name}"
              title="Delete the tag from every file"
              onclick={() => ondelete(tag.id)}
            >
              ✕
            </button>
          {/if}
        </li>
      {/each}
    </ul>

    <form
      onsubmit={(e) => {
        e.preventDefault();
        add();
      }}
    >
      <input
        aria-label="New tag name"
        placeholder="New tag"
        maxlength="40"
        spellcheck="false"
        bind:value={name}
        oninput={() => (error = "")}
      />
      <div class="swatches" role="group" aria-label="Colour">
        <button
          type="button"
          class="swatch none"
          class:chosen={color === null}
          aria-label="No colour"
          aria-pressed={color === null}
          onclick={() => (color = null)}
        ></button>
        {#each TAG_COLORS as c (c)}
          <button
            type="button"
            class="swatch"
            class:chosen={color === c}
            style:background={tagColor(c)}
            aria-label={c}
            aria-pressed={color === c}
            onclick={() => (color = c)}
          ></button>
        {/each}
      </div>
      <button class="add" type="submit" disabled={!name.trim()}>Add</button>
    </form>
    {#if error}<p class="error" role="alert">{error}</p>{/if}
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 55;
    display: grid;
    place-items: center;
    background: var(--scrim);
  }

  .dialog {
    width: min(420px, calc(100vw - 2 * var(--space-xl)));
    max-height: calc(100vh - 2 * var(--space-xl));
    padding: var(--space-xl);
    overflow-y: auto;
    border: 1px solid var(--border-default);
    border-radius: var(--radius-xl);
    background: var(--bg-window);
    box-shadow: var(--shadow-popover);
    outline: none;
  }

  header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: var(--space-md);
  }

  h2 {
    margin: 0;
    font-size: var(--font-size-title);
    font-weight: var(--font-weight-semibold);
  }

  .done,
  .add {
    padding: var(--space-xs) var(--space-md);
    border-radius: var(--radius-md);
    background: var(--accent-default);
    color: var(--text-on-accent);
    font-weight: var(--font-weight-medium);
  }

  .add:disabled {
    background: var(--bg-control);
    color: var(--text-tertiary);
  }

  ul {
    margin: var(--space-md) 0;
    padding: 0;
    list-style: none;
  }

  li {
    display: flex;
    align-items: center;
  }

  .tag {
    display: flex;
    flex: 1;
    align-items: center;
    gap: var(--space-sm);
    padding: var(--space-xs) var(--space-sm);
    border-radius: var(--radius-sm);
    text-align: left;
  }

  .tag:hover,
  .remove:hover {
    background: var(--bg-hover);
  }

  .box {
    display: grid;
    place-items: center;
    width: 16px;
    height: 16px;
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-sm);
    font-size: var(--font-size-caption);
    line-height: 1;
  }

  .box:not([data-state="none"]) {
    border-color: var(--accent-default);
    background: var(--accent-default);
    color: var(--text-on-accent);
  }

  .dot {
    width: 10px;
    height: 10px;
    border-radius: 50%;
  }

  .label {
    flex: 1;
  }

  kbd {
    color: var(--text-tertiary);
    font: inherit;
    font-size: var(--font-size-caption);
  }

  .remove {
    padding: var(--space-xs) var(--space-sm);
    border-radius: var(--radius-sm);
    color: var(--text-tertiary);
  }

  form {
    display: flex;
    align-items: center;
    gap: var(--space-sm);
    padding-top: var(--space-md);
    border-top: 1px solid var(--border-default);
  }

  input {
    flex: 1;
    min-width: 0;
    padding: var(--space-xs) var(--space-sm);
    border: 1px solid var(--border-default);
    border-radius: var(--radius-md);
    background: var(--bg-control);
    color: var(--text-primary);
    font: inherit;
  }

  .swatches {
    display: flex;
    gap: var(--space-2xs);
  }

  .swatch {
    width: 14px;
    height: 14px;
    border-radius: 50%;
    border: 2px solid transparent;
  }

  .swatch.none {
    border-color: var(--border-strong);
  }

  .swatch.chosen {
    outline: 2px solid var(--accent-default);
    outline-offset: 1px;
  }

  .error {
    margin: var(--space-xs) 0 0;
    color: var(--status-danger);
    font-size: var(--font-size-caption);
  }
</style>
