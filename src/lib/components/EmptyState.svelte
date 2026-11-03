<script lang="ts">
  import { settings } from "$lib/settings.svelte";
  import Icon, { type IconName } from "./Icon.svelte";

  let {
    icon,
    text,
    action,
    onaction,
    hint = false,
    compact = false,
  }: {
    icon: IconName;
    /** One line: what is empty, and what to do about it. */
    text: string;
    /** The label of a button that fixes the emptiness, such as "New file". */
    action?: string;
    onaction?: () => void;
    /** A first-use hint, which Settings can hide. What is said about a result (nothing found) is not one. */
    hint?: boolean;
    /** For a small area such as a sidebar section or a panel: no big icon, no padding. */
    compact?: boolean;
  } = $props();
</script>

{#if !hint || settings.showHints}
  <div class="empty" class:compact role={compact ? undefined : "status"}>
    <span class="icon" aria-hidden="true"
      ><Icon name={icon} size={compact ? 14 : 28} stroke={compact ? 1.9 : 1.5} /></span
    >
    <p>{text}</p>
    {#if action && onaction}
      <button onclick={onaction}>{action}</button>
    {/if}
  </div>
{/if}

<style>
  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--space-sm);
    padding: var(--space-xl);
    color: var(--text-secondary);
    text-align: center;
    /* A hint never gets in the way of what is under it, except its own button. */
    pointer-events: none;
  }

  .icon {
    color: var(--text-tertiary);
  }

  p {
    margin: 0;
    max-width: 52ch;
  }

  button {
    padding: var(--space-xs) var(--space-md);
    border-radius: var(--radius-md);
    background: var(--bg-control);
    color: var(--text-primary);
    font-weight: var(--font-weight-medium);
    pointer-events: auto;
  }

  button:hover {
    background: var(--bg-hover);
  }

  .compact {
    flex-direction: row;
    align-items: flex-start;
    gap: var(--space-sm);
    padding: var(--space-xs) var(--space-sm);
    font-size: var(--font-size-caption);
    text-align: left;
  }

  .compact .icon {
    padding-top: 2px;
  }

  .compact p {
    max-width: none;
  }
</style>
