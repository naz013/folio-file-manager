<script lang="ts" module>
  /** What a person reads for a tag colour: "red" is "Red". */
  const label = (c: string) => c.charAt(0).toUpperCase() + c.slice(1);
</script>

<script lang="ts">
  import { onMount } from "svelte";
  import type { Tag } from "$lib/api/fs";
  import { shadeName, tagColor, TAG_HUES, TAG_SHADES } from "$lib/tags";
  import EmptyState from "./EmptyState.svelte";

  let {
    tags,
    onopen,
    oncreate,
    onrename,
    onrecolor,
    ondelete,
  }: {
    tags: Tag[];
    /** Show the items that carry the tag. */
    onopen: (tag: Tag) => void;
    /** Each resolves to an error message, or null when it worked. */
    oncreate: (name: string, color: string | null) => Promise<string | null>;
    onrename: (id: number, name: string) => Promise<string | null>;
    onrecolor: (id: number, color: string | null) => Promise<string | null>;
    ondelete: (id: number) => Promise<string | null>;
  } = $props();

  const uid = `tagpick-${crypto.randomUUID().slice(0, 8)}`;

  let screen: HTMLElement;
  onMount(() => screen.focus());

  let name = $state("");
  let color = $state<string | null>(null);
  let error = $state("");

  /** The tag being renamed, with the name typed so far. */
  let editing = $state<{ id: number; name: string } | null>(null);
  let editError = $state("");
  /** The tag whose deletion is being confirmed. */
  let confirming = $state<number | null>(null);
  let note = $state("");

  const count = (n: number) => (n === 1 ? "1 item" : `${n} items`);

  async function add() {
    error = (await oncreate(name, color)) ?? "";
    if (!error) name = "";
  }

  async function saveName() {
    if (!editing) return;
    editError = (await onrename(editing.id, editing.name)) ?? "";
    if (!editError) editing = null;
  }

  async function recolor(tag: Tag, to: string | null) {
    note = (await onrecolor(tag.id, to)) ?? "";
  }

  async function remove(tag: Tag) {
    note = (await ondelete(tag.id)) ?? "";
    confirming = null;
  }
</script>

{#snippet picker(
  group: string,
  what: string,
  value: string | null,
  pick: (c: string | null) => void,
)}
  <div class="picker" role="radiogroup" aria-label="Colour of {what}">
    <label class="swatch none" title="No colour">
      <input
        type="radio"
        name={group}
        checked={value === null}
        onchange={() => pick(null)}
        aria-label="No colour"
      />
      <span class="chip"></span>
    </label>
    <div class="palette">
      {#each TAG_HUES as hue (hue)}
        {#each TAG_SHADES as shade (shade)}
          {@const c = shadeName(hue, shade)}
          {@const name = shade === 3 ? label(hue) : `${label(hue)}, shade ${shade} of 5`}
          <label class="swatch" title={name}>
            <input
              type="radio"
              name={group}
              checked={value === c}
              onchange={() => pick(c)}
              aria-label={name}
            />
            <span class="chip" style:background={tagColor(c)}></span>
          </label>
        {/each}
      {/each}
    </div>
  </div>
{/snippet}

<section bind:this={screen} class="tags" aria-labelledby="tags-screen-title" tabindex="-1">
  <div class="page">
    <header>
      <h2 id="tags-screen-title">Tags</h2>
    </header>
    <p class="lead">
      Tags mark files without moving them. Deleting a tag only takes it off the items that have it;
      the files themselves are never touched.
    </p>

    <form
      class="new"
      onsubmit={(e) => {
        e.preventDefault();
        add();
      }}
    >
      <h3>New tag</h3>
      <div class="row">
        <input
          type="text"
          bind:value={name}
          placeholder="Name"
          aria-label="Name of the new tag"
          aria-invalid={error ? true : undefined}
          aria-describedby={error ? `${uid}-error` : undefined}
          maxlength="40"
          oninput={() => (error = "")}
        />
        {@render picker(`${uid}-new`, "the new tag", color, (c) => (color = c))}
        <button type="submit" class="primary">New tag</button>
      </div>
      {#if error}<p id="{uid}-error" class="error" role="alert">{error}</p>{/if}
    </form>

    {#if note}<p class="error" role="alert">{note}</p>{/if}

    <ul class="list">
      {#each tags as tag (tag.id)}
        {@const fixed = tag.id <= 7}
        <li class="tag">
          <div class="line">
            {#if editing?.id === tag.id}
              <span class="dot" style:background={tagColor(tag.color)}></span>
              <input
                type="text"
                class="rename"
                aria-label="New name of {tag.name}"
                aria-invalid={editError ? true : undefined}
                maxlength="40"
                bind:value={editing.name}
                oninput={() => (editError = "")}
                onkeydown={(e) => {
                  if (e.key === "Enter") {
                    e.preventDefault();
                    saveName();
                  } else if (e.key === "Escape") {
                    e.preventDefault();
                    e.stopPropagation();
                    editing = null;
                  }
                }}
              />
              <button class="primary" onclick={saveName}>Save</button>
              <button onclick={() => (editing = null)}>Cancel</button>
            {:else}
              <button class="open" onclick={() => onopen(tag)} title="Show the items with this tag">
                <span class="dot" style:background={tagColor(tag.color)}></span>
                <span class="name">{tag.name}</span>
                <span class="items">{count(tag.items)}</span>
              </button>
              {#if !fixed}
                <button
                  onclick={() => {
                    editing = { id: tag.id, name: tag.name };
                    editError = "";
                    confirming = null;
                  }}
                  aria-label="Rename {tag.name}">Rename</button
                >
                <button
                  class="danger"
                  onclick={() => (confirming = tag.id)}
                  aria-label="Delete {tag.name}">Delete</button
                >
              {/if}
            {/if}
          </div>

          {#if editing?.id === tag.id && editError}
            <p class="error" role="alert">{editError}</p>
          {/if}

          {#if !fixed && editing?.id !== tag.id}
            <div class="recolor">
              <span class="small">Colour</span>
              {@render picker(`${uid}-${tag.id}`, tag.name, tag.color, (c) => recolor(tag, c))}
            </div>
          {/if}

          {#if confirming === tag.id}
            <div class="confirm" role="alertdialog" aria-label="Delete {tag.name}">
              <span>
                Delete "{tag.name}"? {tag.items === 0
                  ? "No item has it."
                  : `${count(tag.items)} will lose this tag.`} The files are not touched.
              </span>
              <div class="buttons">
                <button class="danger primary" onclick={() => remove(tag)}>Delete tag</button>
                <button onclick={() => (confirming = null)}>Cancel</button>
              </div>
            </div>
          {/if}
        </li>
      {/each}
    </ul>

    {#if tags.every((t) => t.id <= 7)}
      <EmptyState
        icon="tag"
        text="Only the seven colour tags so far. Make your own above, or press the tag key on selected files."
        hint
        compact
      />
    {/if}
  </div>
</section>

<style>
  .tags {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    background: var(--bg-window);
    outline: none;
  }

  .page {
    width: min(640px, 100%);
    margin: 0 auto;
    padding: var(--space-xl) var(--space-lg) var(--space-3xl);
    box-sizing: border-box;
  }

  h2 {
    margin: 0;
    font-size: var(--font-size-title);
    font-weight: var(--font-weight-semibold);
  }

  h3 {
    margin: 0 0 var(--space-sm);
    font-size: var(--font-size-body);
    font-weight: var(--font-weight-semibold);
  }

  .lead {
    margin: var(--space-sm) 0 var(--space-lg);
    color: var(--text-tertiary);
    font-size: var(--font-size-caption);
    line-height: 1.5;
  }

  .new {
    margin-bottom: var(--space-lg);
    padding: var(--space-lg);
    border: 1px solid var(--border-default);
    border-radius: var(--radius-lg);
    background: var(--bg-subtle);
  }

  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-sm) var(--space-md);
  }

  input[type="text"] {
    flex: 1 1 160px;
    min-width: 0;
    padding: var(--space-xs) var(--space-sm);
    border: 1px solid var(--border-default);
    border-radius: var(--radius-sm);
    background: var(--bg-window);
    color: var(--text-primary);
  }

  input[aria-invalid="true"] {
    border-color: var(--status-danger);
  }

  button {
    padding: var(--space-2xs) var(--space-sm);
    border-radius: var(--radius-sm);
    background: var(--bg-control);
  }

  button:hover {
    background: var(--bg-hover);
  }

  button.primary {
    background: var(--accent-default);
    color: var(--text-on-accent);
  }

  button.primary:hover {
    background: var(--accent-hover);
  }

  button.danger:not(.primary) {
    color: var(--status-danger);
  }

  button.danger.primary {
    background: var(--status-danger);
    color: var(--text-on-accent);
  }

  .error {
    margin: var(--space-sm) 0 0;
    padding: var(--space-sm) var(--space-md);
    border-left: 3px solid var(--status-danger);
    border-radius: var(--radius-sm);
    background: var(--bg-window);
    color: var(--text-secondary);
    font-size: var(--font-size-caption);
  }

  .list {
    display: grid;
    gap: var(--space-sm);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .tag {
    display: grid;
    gap: var(--space-sm);
    padding: var(--space-sm) var(--space-md);
    border: 1px solid var(--border-default);
    border-radius: var(--radius-lg);
    background: var(--bg-subtle);
  }

  .line {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-xs) var(--space-sm);
  }

  .open {
    display: flex;
    flex: 1 1 160px;
    align-items: center;
    gap: var(--space-sm);
    min-width: 0;
    background: none;
    text-align: left;
  }

  .open:hover .name {
    text-decoration: underline;
  }

  .name {
    min-width: 0;
    overflow-wrap: anywhere;
    color: var(--text-primary);
  }

  .items {
    margin-left: auto;
    color: var(--text-tertiary);
    font-size: var(--font-size-caption);
    white-space: nowrap;
  }

  /* Tag colours mark tags only, never the chrome around them. */
  .dot {
    flex-shrink: 0;
    width: 10px;
    height: 10px;
    border-radius: 50%;
  }

  .recolor {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-sm);
  }

  .small {
    color: var(--text-tertiary);
    font-size: var(--font-size-caption);
  }

  .confirm {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-sm) var(--space-lg);
    padding-top: var(--space-sm);
    border-top: 1px solid var(--border-default);
    font-size: var(--font-size-caption);
  }

  .buttons {
    display: flex;
    gap: var(--space-xs);
  }

  .picker {
    display: flex;
    align-items: flex-start;
    gap: var(--space-sm);
  }

  /* One column for each hue, light at the top to dark at the bottom. */
  .palette {
    display: grid;
    grid-auto-flow: column;
    grid-template-rows: repeat(5, auto);
    gap: var(--space-2xs) var(--space-xs);
  }

  .swatch {
    position: relative;
    display: inline-flex;
    cursor: pointer;
  }

  .swatch input {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    margin: 0;
    opacity: 0;
    cursor: pointer;
  }

  .chip {
    width: 18px;
    height: 18px;
    border-radius: 50%;
    box-sizing: border-box;
    border: 2px solid transparent;
  }

  .swatch.none .chip {
    border: 1.5px dashed var(--text-tertiary);
  }

  .swatch input:checked + .chip {
    border-color: var(--text-primary);
    box-shadow: 0 0 0 2px var(--bg-window) inset;
  }

  .swatch input:focus-visible + .chip {
    outline: 2px solid var(--accent-default);
    outline-offset: 2px;
  }
</style>
