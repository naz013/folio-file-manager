<script lang="ts">
  import { onMount } from "svelte";
  import { trapFocus } from "$lib/focus";
  import { findTopics } from "$lib/help";
  import { updateSettings } from "$lib/settings.svelte";

  let {
    onclose,
    onshortcuts,
  }: {
    onclose: () => void;
    /** Show the list of keyboard shortcuts in its place. */
    onshortcuts: () => void;
  } = $props();

  let input: HTMLInputElement;
  /** Where focus was, so closing the dialog gives it back. */
  const opener = document.activeElement as HTMLElement | null;
  onMount(() => {
    input.focus();
    return () => opener?.focus?.();
  });

  let query = $state("");
  const topics = $derived(findTopics(query));
  /** The topic being read; the first match while none is picked or the picked one is filtered out. */
  let pickedId = $state("");
  const topic = $derived(topics.find((t) => t.id === pickedId) ?? topics[0]);

  /** The welcome tour is shown by the page once it is no longer marked as seen. */
  function replayTour() {
    updateSettings({ onboarded: false });
    onclose();
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
    class="dialog"
    use:trapFocus
    role="dialog"
    aria-modal="true"
    aria-labelledby="help-title"
    tabindex="-1"
    onpointerdown={(e) => e.stopPropagation()}
    {onkeydown}
  >
    <header>
      <h2 id="help-title">Help</h2>
      <button class="done" onclick={onclose}>Close</button>
    </header>

    <input
      bind:this={input}
      type="search"
      class="filter"
      aria-label="Search help"
      placeholder="Search help, such as undo or tags"
      spellcheck="false"
      bind:value={query}
    />

    <div class="split">
      <nav aria-label="Help topics">
        {#each topics as t (t.id)}
          <button class:current={t.id === topic?.id} onclick={() => (pickedId = t.id)}>
            {t.title}
          </button>
        {/each}
        <p class="count" role="status">{topics.length === 0 ? "No topic matches." : ""}</p>
      </nav>

      <article>
        {#if topic}
          <h3>{topic.title}</h3>
          {#each topic.body as paragraph, i (i)}
            <p>{paragraph}</p>
          {/each}
        {/if}
      </article>
    </div>

    <footer>
      <button onclick={onshortcuts}>Keyboard shortcuts</button>
      <button onclick={replayTour}>Replay the tour</button>
    </footer>
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
    display: flex;
    flex-direction: column;
    gap: var(--space-md);
    width: min(680px, calc(100vw - 2 * var(--space-xl)));
    height: min(520px, calc(100vh - 2 * var(--space-xl)));
    padding: var(--space-xl);
    border: 1px solid var(--border-default);
    border-radius: var(--radius-xl);
    background: var(--bg-window);
    box-shadow: var(--shadow-popover);
    outline: none;
  }

  header,
  footer {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: var(--space-md);
  }

  footer {
    justify-content: flex-end;
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

  .done,
  footer button {
    padding: var(--space-xs) var(--space-md);
    border-radius: var(--radius-md);
    background: var(--bg-control);
    font-weight: var(--font-weight-medium);
  }

  .filter {
    padding: var(--space-xs) var(--space-sm);
    border: 1px solid var(--border-default);
    border-radius: var(--radius-md);
    background: var(--bg-control);
    color: var(--text-primary);
    font: inherit;
  }

  .split {
    display: grid;
    grid-template-columns: minmax(150px, 220px) 1fr;
    gap: var(--space-lg);
    flex: 1;
    min-height: 0;
  }

  nav,
  article {
    overflow-y: auto;
  }

  nav {
    display: flex;
    flex-direction: column;
    gap: var(--space-2xs);
  }

  nav button {
    padding: var(--space-xs) var(--space-sm);
    border-radius: var(--radius-md);
    text-align: left;
  }

  nav button:hover {
    background: var(--bg-hover);
  }

  nav button.current {
    background: var(--accent-subtle);
    font-weight: var(--font-weight-medium);
  }

  .count {
    margin: 0;
    padding: 0 var(--space-sm);
    color: var(--text-tertiary);
    font-size: var(--font-size-caption);
  }

  article p {
    margin: 0 0 var(--space-sm);
    color: var(--text-secondary);
    line-height: 1.5;
    user-select: text;
  }
</style>
