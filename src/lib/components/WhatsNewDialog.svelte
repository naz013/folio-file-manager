<script lang="ts">
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { onMount } from "svelte";
  import { REPO } from "$lib/about";
  import { trapFocus } from "$lib/focus";
  import { parseMarkdown, type Span } from "$lib/markdown";
  import { bundledNotes } from "$lib/notes";

  let {
    version,
    onclose,
  }: {
    version: string;
    onclose: () => void;
  } = $props();

  let dialog: HTMLElement;
  /** Where focus was, so closing the dialog gives it back. */
  const opener = document.activeElement as HTMLElement | null;
  onMount(() => {
    dialog.focus();
    return () => opener?.focus?.();
  });

  const notes = $derived(bundledNotes(version));
  const blocks = $derived(parseMarkdown(notes));
  let linkError = $state("");

  async function open(url: string) {
    linkError = "";
    try {
      await openUrl(url);
    } catch (e) {
      linkError = `Could not open ${url}: ${e}`;
    }
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

{#snippet spans(list: Span[])}
  {#each list as s, i (i)}
    {#if s.href}
      {@const url = s.href}
      <button class="link" onclick={() => open(url)}>{s.text}</button>
    {:else if s.code}
      <code>{s.text}</code>
    {:else if s.bold}
      <strong>{s.text}</strong>
    {:else}
      {s.text}
    {/if}
  {/each}
{/snippet}

<div class="backdrop" role="presentation" onpointerdown={onclose}>
  <div
    bind:this={dialog}
    class="dialog"
    use:trapFocus
    role="dialog"
    aria-modal="true"
    aria-labelledby="whatsnew-title"
    tabindex="-1"
    onpointerdown={(e) => e.stopPropagation()}
    {onkeydown}
  >
    <header>
      <h2 id="whatsnew-title">What's new in Folio {version}</h2>
    </header>

    <!-- svelte-ignore a11y_no_noninteractive_tabindex (a scrolling region must take focus so the keys can scroll it) -->
    <div class="notes" role="region" aria-label="Release notes" tabindex="0">
      {#each blocks as block, i (i)}
        {#if block.kind === "heading"}
          <h3>{@render spans(block.spans)}</h3>
        {:else if block.kind === "list"}
          <ul>
            {#each block.items as item, j (j)}
              <li>{@render spans(item)}</li>
            {/each}
          </ul>
        {:else}
          <p>{@render spans(block.spans)}</p>
        {/if}
      {:else}
        <p class="muted">This build has no release notes.</p>
      {/each}
    </div>
    {#if linkError}<p class="error" role="alert">{linkError}</p>{/if}

    <footer>
      <button class="link" onclick={() => open(`${REPO}/releases`)}>All releases</button>
      <button class="done" onclick={onclose}>Close</button>
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
    width: min(520px, calc(100vw - 2 * var(--space-xl)));
    max-height: calc(100vh - 2 * var(--space-xl));
    padding: var(--space-xl);
    border: 1px solid var(--border-default);
    border-radius: var(--radius-xl);
    background: var(--bg-window);
    box-shadow: var(--shadow-popover);
    outline: none;
  }

  h2 {
    margin: 0;
    font-size: var(--font-size-title);
    font-weight: var(--font-weight-semibold);
  }

  .notes {
    overflow-y: auto;
    color: var(--text-secondary);
    line-height: 1.5;
    user-select: text;
  }

  h3 {
    margin: var(--space-md) 0 var(--space-xs);
    color: var(--text-primary);
    font-size: var(--font-size-body);
    font-weight: var(--font-weight-semibold);
  }

  p {
    margin: 0 0 var(--space-sm);
  }

  ul {
    margin: 0 0 var(--space-sm);
    padding-left: var(--space-xl);
  }

  code {
    padding: 0 var(--space-2xs);
    border-radius: var(--radius-sm);
    background: var(--bg-control);
    font-size: var(--font-size-caption);
  }

  .muted {
    color: var(--text-tertiary);
  }

  .error {
    margin: 0;
    color: var(--status-danger);
    font-size: var(--font-size-caption);
  }

  footer {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }

  button {
    padding: var(--space-xs) var(--space-md);
    border-radius: var(--radius-md);
    background: var(--bg-control);
    font-weight: var(--font-weight-medium);
  }

  .link {
    padding: 0;
    background: none;
    color: var(--accent-default);
    font-weight: inherit;
    text-align: left;
  }

  .link:hover {
    text-decoration: underline;
  }

  .done {
    background: var(--accent-default);
    color: var(--text-on-accent);
  }
</style>
