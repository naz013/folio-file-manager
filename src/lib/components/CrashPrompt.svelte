<script lang="ts">
  import { onMount } from "svelte";

  let {
    onanswer,
    ondismiss,
  }: {
    /** The user's answer: keep crash reports on this computer, or not. */
    onanswer: (on: boolean) => void;
    /** Closed without an answer (Esc): the question comes back at the next start. */
    ondismiss: () => void;
  } = $props();

  let dialog: HTMLElement;
  onMount(() => dialog.focus());

  function onkeydown(e: KeyboardEvent) {
    // Nothing behind the dialog may act on keys while it is open.
    e.stopPropagation();
    if (e.key === "Escape") {
      e.preventDefault();
      ondismiss();
    }
    if (e.key === "Tab") {
      const items = [...dialog.querySelectorAll<HTMLElement>("button")];
      const at = document.activeElement;
      if (e.shiftKey && (at === items[0] || at === dialog)) {
        e.preventDefault();
        items[items.length - 1]?.focus();
      } else if (!e.shiftKey && at === items[items.length - 1]) {
        e.preventDefault();
        items[0]?.focus();
      }
    }
  }
</script>

<div class="backdrop" role="presentation">
  <div
    bind:this={dialog}
    class="dialog"
    role="dialog"
    aria-modal="true"
    aria-labelledby="crash-title"
    aria-describedby="crash-text"
    tabindex="-1"
    {onkeydown}
  >
    <h2 id="crash-title">Keep crash reports?</h2>
    <p id="crash-text">
      If Folio crashes, it can save a short report on this computer: the error, the version and
      where in the code it failed, which may include file paths. Nothing is sent anywhere. You can
      attach a report to a bug if you choose to, and change this or delete the reports in Settings.
    </p>
    <div class="actions">
      <button onclick={() => onanswer(false)}>No thanks</button>
      <button class="primary" onclick={() => onanswer(true)}>Keep reports</button>
    </div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 60;
    display: grid;
    place-items: center;
    background: var(--scrim);
  }

  .dialog {
    width: min(440px, calc(100vw - 2 * var(--space-xl)));
    padding: var(--space-xl);
    border: 1px solid var(--border-default);
    border-radius: var(--radius-xl);
    background: var(--bg-window);
    box-shadow: var(--shadow-popover);
    outline: none;
  }

  h2 {
    margin: 0 0 var(--space-sm);
    font-size: var(--font-size-title);
    font-weight: var(--font-weight-semibold);
  }

  p {
    margin: 0 0 var(--space-lg);
    color: var(--text-secondary);
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-sm);
  }

  button {
    padding: var(--space-xs) var(--space-md);
    border-radius: var(--radius-md);
    background: var(--bg-control);
  }

  .primary {
    background: var(--accent-default);
    color: var(--text-on-accent);
    font-weight: var(--font-weight-medium);
  }
</style>
