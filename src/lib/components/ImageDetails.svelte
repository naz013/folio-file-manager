<script lang="ts">
  import { readImageInfo } from "$lib/api/fs";
  import { facts, placeText, type ImageInfo } from "$lib/imageinfo";

  let {
    path,
    layout,
  }: {
    path: string;
    /** `line` is one row of facts for Quick Look; `list` is labelled rows for the inspector. */
    layout: "line" | "list";
  } = $props();

  /** How long a file must stay selected before its details are read. */
  const WAIT_MS = 150;

  let info = $state<ImageInfo | null>(null);
  /**
   * Whether the person asked to see the place. Callers key this component on the file, so
   * another file starts hidden, and coming back to this one does not show it again unasked.
   */
  let located = $state(false);

  $effect(() => {
    const file = path;
    const withPlace = located;
    // Asking for the place keeps what is shown meanwhile.
    let current = true;
    // A moment's wait, so moving through a folder with the arrow keys asks only for the file
    // it stops on. Asking for the place needs no wait: the person just clicked.
    const timer = setTimeout(
      () => {
        readImageInfo(file, withPlace)
          .then((i) => current && (info = i))
          // Details are a courtesy: a failure shows nothing.
          .catch(() => current && (info = null));
      },
      withPlace ? 0 : WAIT_MS,
    );
    return () => {
      current = false;
      clearTimeout(timer);
    };
  });

  const rows = $derived(info ? facts(info) : []);
</script>

{#snippet place(i: ImageInfo)}
  {#if i.location}<span class="place">{placeText(i.location)}</span>{/if}
  <!-- One button for both, so keyboard focus stays on it when it changes. -->
  <button
    class="link"
    onclick={() => {
      located = !located;
      // Hiding takes the place off the page at once, not when the next answer comes.
      if (!located && info) info = { ...info, location: null };
    }}
  >
    {i.location ? "Hide location" : "Show location"}
  </button>
{/snippet}

{#if info && (rows.length || info.hasLocation)}
  {#if layout === "line"}
    <div class="line" role="group" aria-label="Picture details">
      {#each rows as row (row.label)}
        <span class="fact" title={row.label} aria-label="{row.label}: {row.text}">{row.text}</span>
      {/each}
      {#if info.hasLocation}<span class="fact">{@render place(info)}</span>{/if}
    </div>
  {:else}
    <section class="list">
      <h3>Picture</h3>
      <dl>
        {#each rows as row (row.label)}
          <dt>{row.label}</dt>
          <dd>{row.text}</dd>
        {/each}
        {#if info.hasLocation}
          <dt>Location</dt>
          <dd>{@render place(info)}</dd>
        {/if}
      </dl>
    </section>
  {/if}
{/if}

<style>
  .line {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2xs) var(--space-md);
    padding: var(--space-xs) var(--space-lg);
    border-top: 1px solid var(--border-default);
    color: var(--text-secondary);
    font-size: var(--font-size-caption);
  }

  .fact {
    display: inline-flex;
    align-items: baseline;
    gap: var(--space-xs);
    min-width: 0;
  }

  .list h3 {
    margin: 0 0 var(--space-xs);
    color: var(--text-tertiary);
    font-size: var(--font-size-caption);
    font-weight: var(--font-weight-semibold);
    text-transform: uppercase;
  }

  dl {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: var(--space-xs) var(--space-md);
    margin: 0;
    font-size: var(--font-size-caption);
  }

  dt {
    color: var(--text-secondary);
  }

  dd {
    margin: 0;
    min-width: 0;
    text-align: right;
    overflow-wrap: anywhere;
    user-select: text;
  }

  .place {
    color: var(--text-primary);
  }

  .link {
    padding: 0;
    color: var(--accent-default);
    font: inherit;
    text-decoration: underline;
    cursor: pointer;
  }

  .link:focus-visible {
    outline: 2px solid var(--accent-default);
    outline-offset: 2px;
    border-radius: var(--radius-sm);
  }
</style>
