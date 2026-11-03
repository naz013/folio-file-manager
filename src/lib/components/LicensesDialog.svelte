<script lang="ts">
  import { onMount } from "svelte";
  import { trapFocus } from "$lib/focus";
  import { filterPackages, type LicenseData } from "$lib/licenses";

  let { onclose }: { onclose: () => void } = $props();

  let search: HTMLInputElement;
  /** Where focus was, so closing the dialog gives it back. */
  const opener = document.activeElement as HTMLElement | null;

  let data = $state<LicenseData | null>(null);
  let error = $state("");
  let query = $state("");
  /** The package whose license text is open. */
  let open = $state<string | null>(null);

  onMount(() => {
    search.focus();
    // The list is large, so it is loaded when asked for, not with the app.
    import("$lib/licenses.json").then(
      (m) => (data = m.default as LicenseData),
      (e) => (error = `Could not load the licenses: ${e}`),
    );
    return () => opener?.focus?.();
  });

  const shown = $derived(data ? filterPackages(data.packages, query) : []);
  const keyOf = (p: { name: string; version: string }) => `${p.name}@${p.version}`;

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
    aria-labelledby="licenses-title"
    tabindex="-1"
    onpointerdown={(e) => e.stopPropagation()}
    {onkeydown}
  >
    <h2 id="licenses-title">Open-source licenses</h2>
    <p class="intro">
      Folio is built on the software below. Each is free software under its own license, shown here
      and in <code>THIRD-PARTY-LICENSES.txt</code> next to the app.
    </p>

    <input
      bind:this={search}
      bind:value={query}
      type="search"
      placeholder="Search by package or license"
      aria-label="Search the licenses"
    />

    {#if error}
      <p class="error" role="alert">{error}</p>
    {:else if !data}
      <p class="note" role="status">Loading…</p>
    {:else if data.packages.length === 0}
      <p class="note" role="status">
        This build has no license list. Release builds include it (<code>npm run licenses</code>).
      </p>
    {:else}
      <p class="note" role="status">
        {shown.length === data.packages.length
          ? `${shown.length} packages`
          : `${shown.length} of ${data.packages.length} packages`}
      </p>
      <!-- About 350 rows in a dialog that is rarely open: not worth virtualizing. -->
      <ul>
        {#each shown as p (keyOf(p))}
          {@const k = keyOf(p)}
          <li>
            <button
              class="pkg"
              aria-expanded={open === k}
              onclick={() => (open = open === k ? null : k)}
            >
              <span class="name">{p.name} <span class="version">{p.version}</span></span>
              <span class="license">{p.license}</span>
            </button>
            {#if open === k}
              <pre>{p.text
                  ? data.texts[p.text]
                  : `${p.license}. This package ships no license file.${p.repo ? `\n${p.repo}` : ""}`}</pre>
            {/if}
          </li>
        {/each}
      </ul>
      {#if shown.length === 0}
        <p class="note">No package matches.</p>
      {/if}
    {/if}

    <footer>
      <button class="done" onclick={onclose}>Close</button>
    </footer>
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
    display: flex;
    flex-direction: column;
    width: min(640px, calc(100vw - 2 * var(--space-xl)));
    height: min(680px, calc(100vh - 2 * var(--space-xl)));
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

  .intro,
  .note {
    margin: var(--space-xs) 0;
    color: var(--text-secondary);
    font-size: var(--font-size-caption);
  }

  code {
    font-family: var(--font-mono);
  }

  input {
    padding: var(--space-xs) var(--space-md);
    border: 1px solid var(--border-default);
    border-radius: var(--radius-md);
    background: var(--bg-subtle);
  }

  ul {
    flex: 1;
    min-height: 0;
    margin: 0;
    padding: 0;
    overflow-y: auto;
    list-style: none;
  }

  .pkg {
    display: flex;
    justify-content: space-between;
    gap: var(--space-md);
    width: 100%;
    padding: var(--space-xs) var(--space-sm);
    border-radius: var(--radius-md);
    text-align: left;
  }

  .version,
  .license {
    color: var(--text-tertiary);
  }

  .license {
    flex-shrink: 0;
    font-size: var(--font-size-caption);
  }

  pre {
    margin: var(--space-xs) var(--space-sm) var(--space-sm);
    padding: var(--space-sm);
    max-height: 260px;
    overflow: auto;
    border-radius: var(--radius-md);
    background: var(--bg-subtle);
    font-family: var(--font-mono);
    font-size: var(--font-size-caption);
    white-space: pre-wrap;
    user-select: text;
  }

  .error {
    margin: var(--space-sm) 0;
    color: var(--status-danger);
    font-size: var(--font-size-caption);
  }

  footer {
    display: flex;
    justify-content: flex-end;
    margin-top: var(--space-md);
  }

  .done {
    padding: var(--space-xs) var(--space-lg);
    border-radius: var(--radius-md);
    background: var(--accent-default);
    color: var(--text-on-accent);
    font-weight: var(--font-weight-medium);
  }
</style>
