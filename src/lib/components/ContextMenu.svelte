<script lang="ts" module>
  export interface MenuEntry {
    label: string;
    /** The keyboard shortcut that does the same, shown at the right. */
    hint?: string;
    /** Who the entry comes from (a plugin's name), shown at the right as plain text. */
    source?: string;
    disabled?: boolean;
    danger?: boolean;
    run: () => void;
  }

  export type MenuItem =
    | "separator"
    | MenuEntry
    /**
     * Entries from one source under a heading Folio draws, so they can't pass for its own. The
     * heading labels the group for screen readers; keyboard movement runs through it as usual.
     */
    | { group: string; entries: MenuEntry[] };
</script>

<script lang="ts">
  import { onMount, tick } from "svelte";

  let {
    items,
    x,
    y,
    onclose,
  }: {
    items: MenuItem[];
    x: number;
    y: number;
    onclose: () => void;
  } = $props();

  let el: HTMLElement;
  let left = $state(0);
  let top = $state(0);
  let placed = $state(false);
  /** Where focus was, so closing the menu gives it back. */
  const opener = document.activeElement as HTMLElement | null;

  const enabled = () => [...el.querySelectorAll<HTMLElement>("button:not(:disabled)")];

  onMount(async () => {
    left = x;
    top = y;
    await tick();
    // Keep the whole menu on screen: open up or to the left when there is no room.
    // Measured without transforms: the pop-in animation has it slightly scaled down.
    const box = { width: el.offsetWidth, height: el.offsetHeight };
    left = Math.max(0, Math.min(x, window.innerWidth - box.width));
    top = Math.max(0, y + box.height > window.innerHeight ? y - box.height : y);
    placed = true;
    // A hidden element can't take focus: let it show first.
    await tick();
    enabled()[0]?.focus();
  });

  function onkeydown(e: KeyboardEvent) {
    // Nothing behind the menu may act on these keys.
    e.stopPropagation();
    if (e.key === "Escape" || e.key === "Tab") {
      e.preventDefault();
      dismiss();
      return;
    }
    const all = enabled();
    const at = all.indexOf(document.activeElement as HTMLElement);
    let next: HTMLElement | undefined;
    if (e.key === "ArrowDown") next = all[(at + 1) % all.length];
    else if (e.key === "ArrowUp")
      next = all[at < 0 ? all.length - 1 : (at - 1 + all.length) % all.length];
    else if (e.key === "Home") next = all[0];
    else if (e.key === "End") next = all[all.length - 1];
    else return;
    e.preventDefault();
    next?.focus();
  }

  function choose(item: MenuEntry) {
    onclose();
    // An action that moves focus itself (renaming) does so after this.
    opener?.focus?.();
    item.run();
  }

  /** Escape or Tab: back to where the user was. */
  function dismiss() {
    onclose();
    opener?.focus?.();
  }
</script>

<svelte:window
  onpointerdown={onclose}
  onblur={onclose}
  onresize={onclose}
  oncontextmenu={(e) => e.preventDefault()}
/>

<!-- Pointer events inside the menu must not reach the window handler that closes it. -->
<div
  bind:this={el}
  class="menu"
  role="menu"
  aria-label="Actions"
  aria-orientation="vertical"
  tabindex="-1"
  style:visibility={placed ? "visible" : "hidden"}
  style:left="{left}px"
  style:top="{top}px"
  onpointerdown={(e) => e.stopPropagation()}
  {onkeydown}
>
  {#each items as item, i (i)}
    {#if item === "separator"}
      <div class="sep" role="separator"></div>
    {:else if "group" in item}
      <div class="group" role="group" aria-label={item.group}>
        <div class="heading" aria-hidden="true">{item.group}</div>
        {#each item.entries as entry, j (j)}
          {@render row(entry)}
        {/each}
      </div>
    {:else}
      {@render row(item)}
    {/if}
  {/each}
</div>

{#snippet row(item: MenuEntry)}
  <button
    role="menuitem"
    class:danger={item.danger}
    disabled={item.disabled}
    aria-label={item.source ? `${item.label}, from ${item.source}` : undefined}
    onclick={() => choose(item)}
  >
    <span>{item.label}</span>
    {#if item.hint}<kbd>{item.hint}</kbd>{/if}
    {#if item.source}<span class="source">{item.source}</span>{/if}
  </button>
{/snippet}

<style>
  .menu {
    position: fixed;
    z-index: 60;
    min-width: 220px;
    max-height: calc(100vh - 2 * var(--space-md));
    overflow-y: auto;
    padding: var(--space-2xs);
    border: 1px solid var(--border-default);
    border-radius: var(--radius-lg);
    background: var(--bg-window);
    box-shadow: var(--shadow-popover);
    outline: none;
  }

  button {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: var(--space-xl);
    width: 100%;
    padding: var(--space-xs) var(--space-sm);
    border-radius: var(--radius-sm);
    color: var(--text-primary);
    text-align: left;
  }

  button:hover:not(:disabled),
  button:focus {
    background: var(--bg-hover);
    outline: none;
  }

  button:focus-visible {
    outline: 2px solid var(--accent-default);
    outline-offset: -2px;
  }

  button:disabled {
    color: var(--text-tertiary);
  }

  button.danger {
    color: var(--status-danger);
  }

  kbd {
    color: var(--text-tertiary);
    font: inherit;
    font-size: var(--font-size-caption);
  }

  .heading {
    padding: var(--space-xs) var(--space-sm) var(--space-2xs);
    color: var(--text-tertiary);
    font-size: var(--font-size-caption);
    font-weight: var(--font-weight-semibold);
    text-transform: uppercase;
  }

  .source {
    min-width: 0;
    max-width: 14em;
    overflow: hidden;
    color: var(--text-tertiary);
    font-size: var(--font-size-caption);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  button > span:first-child {
    min-width: 0;
  }

  .sep {
    height: 1px;
    margin: var(--space-2xs) var(--space-xs);
    background: var(--border-default);
  }
</style>
