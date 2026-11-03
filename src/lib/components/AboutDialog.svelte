<script lang="ts">
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { onMount } from "svelte";
  import { appInfo, type AppInfo } from "$lib/api/fs";
  import { LICENSE, LINKS, versionLine } from "$lib/about";
  import { trapFocus } from "$lib/focus";
  import { checkForUpdate, updateStatusText, updates } from "$lib/updates.svelte";
  import Icon from "./Icon.svelte";
  import LicensesDialog from "./LicensesDialog.svelte";

  let { onclose, onwhatsnew }: { onclose: () => void; onwhatsnew: () => void } = $props();

  let dialog: HTMLElement;
  /** Where focus was, so closing the dialog gives it back. */
  const opener = document.activeElement as HTMLElement | null;
  onMount(() => {
    dialog.focus();
    appInfo().then(
      (i) => (info = i),
      (e) => (error = String(e)),
    );
    return () => {
      clearTimeout(copiedTimer);
      opener?.focus?.();
    };
  });

  let licensesOpen = $state(false);
  let info = $state<AppInfo | null>(null);
  let error = $state("");
  let copied = $state(false);
  let copiedTimer: ReturnType<typeof setTimeout>;
  /** A link that could not be opened, or a copy that failed. */
  let linkError = $state("");

  /** What the manual check found, in words. */
  const updateStatus = $derived(updateStatusText());

  async function copy() {
    if (!info) return;
    try {
      await navigator.clipboard.writeText(versionLine(info));
      copied = true;
      clearTimeout(copiedTimer);
      copiedTimer = setTimeout(() => (copied = false), 2000);
    } catch (e) {
      linkError = `Could not copy: ${e}`;
    }
  }

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

<div class="backdrop" role="presentation" onpointerdown={onclose}>
  <div
    bind:this={dialog}
    class="dialog"
    use:trapFocus
    role="dialog"
    aria-modal="true"
    aria-labelledby="about-title"
    tabindex="-1"
    onpointerdown={(e) => e.stopPropagation()}
    {onkeydown}
  >
    <header>
      <span class="logo"><Icon name="folder" size={22} stroke={2.2} /></span>
      <div>
        <h2 id="about-title">About Folio</h2>
        <p class="tagline">A lightweight, beautiful file manager</p>
      </div>
    </header>

    {#if info}
      <dl>
        <dt>Version</dt>
        <dd>{info.version}</dd>
        <dt>Commit</dt>
        <dd>{info.commit}</dd>
        <dt>Built</dt>
        <dd>{info.buildDate}</dd>
        <dt>Tauri</dt>
        <dd>{info.tauri}</dd>
        <dt>WebView</dt>
        <dd>{info.webview ?? "unknown"}</dd>
        <dt>System</dt>
        <dd>{info.os} {info.arch}</dd>
      </dl>
    {:else if error}
      <p class="error" role="alert">{error}</p>
    {:else}
      <p class="muted" role="status">Loading…</p>
    {/if}

    <div class="actions">
      <button onclick={copy} disabled={!info}>{copied ? "Copied" : "Copy version"}</button>
      <button
        onclick={() => checkForUpdate(true)}
        disabled={updates.phase === "checking" || updates.phase === "downloading"}
      >
        Check for updates
      </button>
      <button onclick={onwhatsnew}>What's new</button>
      <button onclick={() => (licensesOpen = true)}>Open-source licenses</button>
    </div>
    <p class="status" role="status">{updateStatus}</p>

    <p class="license">
      Folio is free software under the <strong>{LICENSE}</strong> license. You may use, study, change
      and share it; if you run a changed version for others, you must offer them its source.
    </p>

    <div class="links" role="group" aria-label="Links">
      {#each LINKS as link (link.url)}
        <button class="link" onclick={() => open(link.url)}>{link.label}</button>
      {/each}
    </div>
    {#if linkError}<p class="error" role="alert">{linkError}</p>{/if}

    <footer>
      <button class="done" onclick={onclose}>Close</button>
    </footer>
  </div>
</div>

{#if licensesOpen}
  <LicensesDialog onclose={() => (licensesOpen = false)} />
{/if}

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
    width: min(420px, calc(100vw - 2 * var(--space-xl)));
    max-height: calc(100vh - 2 * var(--space-xl));
    padding: var(--space-xl);
    overflow-y: auto;
    border: 1px solid var(--border-default);
    border-radius: var(--radius-xl);
    background: var(--bg-window);
    box-shadow: var(--shadow-popover);
    outline: none;
  }

  header {
    display: flex;
    align-items: center;
    gap: var(--space-md);
  }

  .logo {
    display: grid;
    place-items: center;
    width: var(--space-3xl);
    height: var(--space-3xl);
    border-radius: var(--radius-lg);
    background: var(--accent-default);
    color: var(--text-on-accent);
  }

  h2 {
    margin: 0;
    font-size: var(--font-size-title);
    font-weight: var(--font-weight-semibold);
  }

  .tagline,
  .muted {
    margin: 0;
    color: var(--text-secondary);
    font-size: var(--font-size-caption);
  }

  dl {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: var(--space-xs) var(--space-lg);
    margin: var(--space-lg) 0;
    font-size: var(--font-size-body);
  }

  dt {
    color: var(--text-tertiary);
  }

  dd {
    margin: 0;
    overflow-wrap: anywhere;
    user-select: text;
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-sm);
  }

  button {
    padding: var(--space-xs) var(--space-md);
    border-radius: var(--radius-md);
    background: var(--bg-control);
    font-weight: var(--font-weight-medium);
  }

  button:disabled {
    color: var(--text-tertiary);
  }

  .status {
    min-height: 1.2em;
    margin: var(--space-xs) 0 0;
    color: var(--text-secondary);
    font-size: var(--font-size-caption);
  }

  .license {
    margin: var(--space-md) 0;
    color: var(--text-secondary);
    font-size: var(--font-size-caption);
  }

  .links {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-xs);
  }

  .link {
    background: none;
    color: var(--accent-default);
  }

  .link:hover {
    background: var(--bg-hover);
  }

  .error {
    margin: var(--space-xs) 0 0;
    color: var(--status-danger);
    font-size: var(--font-size-caption);
  }

  footer {
    display: flex;
    justify-content: flex-end;
    margin-top: var(--space-lg);
  }

  .done {
    background: var(--accent-default);
    color: var(--text-on-accent);
  }
</style>
