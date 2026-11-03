<script lang="ts" module>
  export type MenuItem =
    | "separator"
    | {
        label: string;
        /** The keyboard shortcut that does the same, shown at the right. */
        hint?: string;
        disabled?: boolean;
        danger?: boolean;
        run: () => void;
      };
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

  function choose(item: Exclude<MenuItem, "separator">) {
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
    {:else}
      <button
        role="menuitem"
        class:danger={item.danger}
        disabled={item.disabled}
        onclick={() => choose(item)}
      >
        <span>{item.label}</span>
        {#if item.hint}<kbd>{item.hint}</kbd>{/if}
      </button>
    {/if}
  {/each}
</div>

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

  .sep {
    height: 1px;
    margin: var(--space-2xs) var(--space-xs);
    background: var(--border-default);
  }
</style>
