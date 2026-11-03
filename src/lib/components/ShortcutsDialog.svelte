<script lang="ts">
  import { onMount } from "svelte";
  import { trapFocus } from "$lib/focus";
  import { shortcutGroups } from "$lib/shortcuts";

  let { onclose }: { onclose: () => void } = $props();

  let input: HTMLInputElement;
  /** Where focus was, so closing the dialog gives it back. */
  const opener = document.activeElement as HTMLElement | null;
  onMount(() => {
    input.focus();
    return () => opener?.focus?.();
  });

  /** Written for this system: Cmd and Option on a Mac. */
  const groups = shortcutGroups();
  let query = $state("");

  /** What matches the filter: a shortcut is found by what it does or by its keys. */
  const shown = $derived.by(() => {
    const words = query.toLowerCase().split(/\s+/).filter(Boolean);
    return groups
      .map((group) => ({
        title: group.title,
        items: group.items.filter((item) => {
          const text = `${item.does} ${item.keys} ${group.title}`.toLowerCase();
          return words.every((w) => text.includes(w));
        }),
      }))
      .filter((group) => group.items.length);
  });
  const count = $derived(shown.reduce((n, g) => n + g.items.length, 0));

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
    aria-labelledby="shortcuts-title"
    tabindex="-1"
    onpointerdown={(e) => e.stopPropagation()}
    {onkeydown}
  >
    <header>
      <h2 id="shortcuts-title">Keyboard shortcuts</h2>
      <button class="done" onclick={onclose}>Close</button>
    </header>

    <input
      bind:this={input}
      type="search"
      class="filter"
      aria-label="Filter shortcuts"
      placeholder="Filter, such as copy or ctrl+t"
      spellcheck="false"
      bind:value={query}
    />
    <p class="count" role="status">
      {count === 0 ? "No shortcut matches." : `${count} shortcut${count === 1 ? "" : "s"}`}
    </p>

    <div class="groups">
      {#each shown as group (group.title)}
        <section aria-labelledby="group-{group.title}">
          <h3 id="group-{group.title}">{group.title}</h3>
          <ul>
            {#each group.items as item (item.keys + item.does)}
              <li>
                <span>{item.does}</span>
                <kbd>{item.keys}</kbd>
              </li>
            {/each}
          </ul>
        </section>
      {/each}
    </div>
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
    width: min(560px, calc(100vw - 2 * var(--space-xl)));
    max-height: calc(100vh - 2 * var(--space-xl));
    padding: var(--space-xl);
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
    margin-bottom: var(--space-md);
  }

  h2 {
    margin: 0;
    font-size: var(--font-size-title);
    font-weight: var(--font-weight-semibold);
  }

  .done {
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

  .count {
    margin: var(--space-xs) 0 var(--space-sm);
    color: var(--text-tertiary);
    font-size: var(--font-size-caption);
  }

  .groups {
    overflow-y: auto;
  }

  section + section {
    margin-top: var(--space-md);
  }

  h3 {
    margin: 0 0 var(--space-xs);
    color: var(--text-secondary);
    font-size: var(--font-size-caption);
    font-weight: var(--font-weight-semibold);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  li {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: var(--space-lg);
    padding: var(--space-xs) var(--space-sm);
    border-radius: var(--radius-sm);
    font-size: var(--font-size-body);
  }

  li:hover {
    background: var(--bg-hover);
  }

  kbd {
    flex: none;
    color: var(--text-secondary);
    font: inherit;
    font-size: var(--font-size-caption);
    white-space: pre;
  }
</style>
