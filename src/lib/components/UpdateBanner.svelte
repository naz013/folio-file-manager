<script lang="ts">
  import {
    clearUpdateError,
    installDidFail,
    installUpdate,
    postponeUpdate,
    updates,
  } from "$lib/updates.svelte";

  /** Shown for a version on offer, while it downloads, and for a failure. Never a dialog. */
  const shown = $derived(
    updates.phase === "downloading" ||
      (updates.phase === "failed" && installDidFail()) ||
      (updates.phase === "available" && !updates.hidden),
  );
</script>

{#if shown}
  <div class="banner" role="status" aria-live="polite">
    {#if updates.phase === "downloading"}
      <span>
        Installing Folio {updates.version}{updates.percent === null
          ? "…"
          : ` (${updates.percent}%)`}
      </span>
    {:else if updates.phase === "failed"}
      <span class="problem">{updates.error}</span>
      <button onclick={clearUpdateError}>Close</button>
    {:else}
      <span>Folio {updates.version} is available.</span>
      <button onclick={postponeUpdate}>Later</button>
      <button class="primary" onclick={installUpdate}>Update and restart</button>
    {/if}
  </div>
{/if}

<style>
  .banner {
    position: fixed;
    right: var(--space-lg);
    bottom: var(--space-lg);
    z-index: 50;
    display: flex;
    align-items: center;
    gap: var(--space-sm);
    max-width: min(480px, calc(100vw - 2 * var(--space-lg)));
    padding: var(--space-sm) var(--space-md);
    border: 1px solid var(--border-default);
    border-radius: var(--radius-lg);
    background: var(--bg-window);
    box-shadow: var(--shadow-popover);
  }

  .problem {
    color: var(--text-secondary);
  }

  button {
    padding: var(--space-xs) var(--space-md);
    border-radius: var(--radius-md);
    background: var(--bg-control);
    white-space: nowrap;
  }

  .primary {
    background: var(--accent-default);
    color: var(--text-on-accent);
    font-weight: var(--font-weight-medium);
  }
</style>
