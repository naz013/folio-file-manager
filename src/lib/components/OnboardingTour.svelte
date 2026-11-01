<script lang="ts">
  import { onMount } from "svelte";

  let {
    onclose,
  }: {
    /** The tour ended, finished or skipped: either way it is not shown again. */
    onclose: () => void;
  } = $props();

  const steps = [
    {
      title: "Work in tabs",
      text: "Open a new tab with Ctrl+T, close one with Ctrl+W, right-click a folder to open it in a new tab, and drag a tab out to make a new window. Folio brings your tabs back the next time you start it.",
    },
    {
      title: "Tag instead of filing",
      text: "Select files, right-click and choose Tags… to colour-label them. Pick a tag in the sidebar to see everything with it, from any folder. Tags stay with your files and never change them.",
    },
    {
      title: "Clean up safely",
      text: "Press Ctrl+K and type “clean” to find large, old, duplicate and unfinished files. Folio only suggests; you review the list, and everything goes to the Recycle Bin, so you can always get it back.",
    },
  ];

  let step = $state(0);
  const last = $derived(step === steps.length - 1);

  let dialog: HTMLElement;
  onMount(() => dialog.focus());

  function onkeydown(e: KeyboardEvent) {
    // Nothing behind the dialog may act on keys while it is open.
    e.stopPropagation();
    if (e.key === "Escape") {
      e.preventDefault();
      onclose();
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
    aria-labelledby="tour-title"
    aria-describedby="tour-text"
    tabindex="-1"
    {onkeydown}
  >
    <p class="count">Welcome to Folio · {step + 1} of {steps.length}</p>
    <h2 id="tour-title">{steps[step].title}</h2>
    <p id="tour-text" aria-live="polite">{steps[step].text}</p>
    <div class="actions">
      <button onclick={onclose}>Skip</button>
      <span class="spacer"></span>
      {#if step > 0}<button onclick={() => step--}>Back</button>{/if}
      <button class="primary" onclick={() => (last ? onclose() : step++)}>
        {last ? "Done" : "Next"}
      </button>
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
    width: min(460px, calc(100vw - 2 * var(--space-xl)));
    padding: var(--space-xl);
    border: 1px solid var(--border-default);
    border-radius: var(--radius-xl);
    background: var(--bg-window);
    box-shadow: var(--shadow-popover);
    outline: none;
  }

  .count {
    margin: 0 0 var(--space-xs);
    color: var(--text-secondary);
    font-size: var(--font-size-caption);
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
    align-items: center;
    gap: var(--space-sm);
  }

  .spacer {
    flex: 1;
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
