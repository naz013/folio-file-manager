<script lang="ts">
  import { onMount } from "svelte";
  import {
    cleanupLargeOld,
    cleanupTrash,
    onQueueUpdate,
    parentOf,
    scanRoots,
    type BigFile,
  } from "$lib/api/fs";
  import { formatDate, formatSize } from "$lib/format";
  import {
    CLEANUP_AGES_DAYS,
    CLEANUP_SIZES_MB,
    settings,
    updateSettings,
  } from "$lib/settings.svelte";
  import Thumbnail from "./Thumbnail.svelte";

  let {
    onclose,
    onopen,
  }: {
    onclose: () => void;
    /** Show this folder in the browser. */
    onopen: (path: string) => void;
  } = $props();

  /** The list shows this many suggestions, the biggest first. The cap is why the rows are
   * not virtualized: the list is bounded, and `content-visibility` skips the off-screen ones. */
  const LIMIT = 200;

  let dialog: HTMLElement;
  const opener = document.activeElement as HTMLElement | null;

  let files = $state<BigFile[]>([]);
  /** Nothing is picked until the person picks it: cleanup only suggests (ADR 0003). */
  const picked = $state<Record<string, boolean>>({});
  let loading = $state(true);
  let scannedAny = $state(true);
  let error = $state("");
  let loadId = 0;
  /** The Recycle Bin job started from here; the list refreshes when it ends. */
  let trashJob = $state<number | null>(null);

  const name = (p: string) => p.split(/[\\/]/).filter(Boolean).pop() ?? p;
  const sizeLabel = (mb: number) => (mb >= 1000 ? `${mb / 1000} GB` : `${mb} MB`);
  const ageLabel = (days: number) =>
    days >= 365 ? `${days / 365} ${days === 365 ? "year" : "years"}` : `${days} days`;

  const chosen = $derived(files.filter((f) => picked[f.path]));
  const chosenBytes = $derived(chosen.reduce((sum, f) => sum + f.size, 0));
  const totalBytes = $derived(files.reduce((sum, f) => sum + f.size, 0));

  async function load() {
    const id = ++loadId;
    loading = true;
    error = "";
    try {
      const [found, roots] = await Promise.all([
        cleanupLargeOld(settings.cleanupMinMb * 1024 * 1024, settings.cleanupAgeDays, LIMIT),
        scanRoots().catch(() => []),
      ]);
      if (id !== loadId) return;
      files = found;
      scannedAny = roots.length > 0;
      // Only what is still listed can stay picked.
      for (const path of Object.keys(picked)) {
        if (!found.some((f) => f.path === path)) delete picked[path];
      }
    } catch (e) {
      if (id === loadId) error = String(e);
    } finally {
      if (id === loadId) loading = false;
    }
  }

  // The thresholds changed: look again.
  $effect(() => {
    void settings.cleanupMinMb;
    void settings.cleanupAgeDays;
    load();
  });

  let moving = $state(false);

  async function trash() {
    if (moving || !chosen.length) return;
    moving = true;
    try {
      // The core moves only files still exactly as listed and still allowed, as a normal
      // Recycle Bin job (so it can be undone). The list is looked at again when it ends.
      const done = await cleanupTrash(chosen);
      for (const f of chosen) delete picked[f.path];
      if (done.leftAlone) {
        error = `${done.leftAlone === 1 ? "1 file was" : `${done.leftAlone} files were`} left alone: changed since the list was made, or not safe to move. Files that do not fit in the Recycle Bin are reported when the move ends.`;
      }
      if (done.job === null) await load();
      else trashJob = done.job;
      dialog.focus();
    } catch (e) {
      error = String(e);
    } finally {
      moving = false;
    }
  }

  onMount(() => {
    dialog.focus();
    const unlisten = onQueueUpdate((job) => {
      if (job.id !== trashJob || !["done", "failed", "cancelled"].includes(job.state)) return;
      trashJob = null;
      load();
    });
    return () => {
      unlisten.then((u) => u());
      opener?.focus?.();
    };
  });

  function onkeydown(e: KeyboardEvent) {
    // Nothing behind the view may act on keys while it is open.
    e.stopPropagation();
    if (e.key === "Escape") {
      e.preventDefault();
      onclose();
    }
    if (e.key === "Tab") {
      // Keep focus inside the view.
      const items = [...dialog.querySelectorAll<HTMLElement>("button, select, input")].filter(
        (el) => !el.hasAttribute("disabled"),
      );
      const first = items[0];
      const last = items[items.length - 1];
      const at = document.activeElement;
      if (e.shiftKey && (at === first || at === dialog)) {
        e.preventDefault();
        last?.focus();
      } else if (!e.shiftKey && at === last) {
        e.preventDefault();
        first?.focus();
      }
    }
  }
</script>

<div
  bind:this={dialog}
  class="cleanup"
  role="dialog"
  aria-modal="true"
  aria-labelledby="cleanup-title"
  tabindex="-1"
  {onkeydown}
>
  <header>
    <h2 id="cleanup-title">Large &amp; old files</h2>
    <span class="grow"></span>
    <button onclick={onclose} aria-label="Close cleanup">✕</button>
  </header>

  <div class="filters">
    <label>
      Bigger than
      <select
        value={settings.cleanupMinMb}
        onchange={(e) => updateSettings({ cleanupMinMb: Number(e.currentTarget.value) })}
      >
        {#each CLEANUP_SIZES_MB as mb (mb)}
          <option value={mb}>{sizeLabel(mb)}</option>
        {/each}
      </select>
    </label>
    <label>
      Not changed for
      <select
        value={settings.cleanupAgeDays}
        onchange={(e) => updateSettings({ cleanupAgeDays: Number(e.currentTarget.value) })}
      >
        {#each CLEANUP_AGES_DAYS as days (days)}
          <option value={days}>{ageLabel(days)}</option>
        {/each}
      </select>
    </label>
  </div>

  {#if error}<p class="error" role="alert">{error}</p>{/if}

  {#if loading && !files.length}
    <p class="empty" role="status">Looking…</p>
  {:else if !scannedAny}
    <p class="empty" role="status">
      Nothing has been scanned yet. Scan a drive in Storage first; cleanup suggests files from the
      scan.
    </p>
  {:else if !files.length}
    <p class="empty" role="status">
      No file is bigger than {sizeLabel(settings.cleanupMinMb)} and untouched for {ageLabel(
        settings.cleanupAgeDays,
      )}.
    </p>
  {:else}
    <p class="summary" role="status">
      {files.length === LIMIT ? `The ${LIMIT} biggest` : files.length} suggested, {formatSize(
        totalBytes,
      )} in all. Nothing is selected until you choose it.
    </p>
    <ul class="list" aria-label="Suggested files">
      {#each files as f (f.path)}
        <li>
          <label class="row">
            <input
              type="checkbox"
              aria-label="{name(f.path)}, {formatSize(f.size)}"
              bind:checked={picked[f.path]}
            />
            <span class="thumb"><Thumbnail path={f.path} name={name(f.path)} size={40} /></span>
            <span class="text">
              <span class="fname">{name(f.path)}</span>
              <span class="where">{parentOf(f.path) ?? ""}</span>
            </span>
            <span class="muted">{formatDate(f.modified * 1000)}</span>
            <span class="size">{formatSize(f.size)}</span>
          </label>
          <button
            class="show"
            aria-label="Show {name(f.path)} in folder"
            onclick={() => {
              const dir = parentOf(f.path);
              if (dir) onopen(dir);
            }}>Show in folder</button
          >
        </li>
      {/each}
    </ul>
  {/if}

  <footer>
    <p class="note">Files go to the Recycle Bin. The space comes back when you empty it.</p>
    <button class="go" disabled={!chosen.length || moving || trashJob !== null} onclick={trash}>
      {chosen.length
        ? `Move ${chosen.length} ${chosen.length === 1 ? "file" : "files"} (${formatSize(chosenBytes)}) to Recycle Bin`
        : "Move to Recycle Bin"}
    </button>
  </footer>
</div>

<style>
  .cleanup {
    position: fixed;
    inset: 0;
    z-index: 50;
    display: flex;
    flex-direction: column;
    gap: var(--space-md);
    padding: var(--space-xl);
    background: var(--bg-window);
    outline: none;
  }

  header,
  .filters,
  footer {
    display: flex;
    align-items: center;
    gap: var(--space-md);
  }

  h2 {
    margin: 0;
    font-size: var(--font-size-heading);
    font-weight: var(--font-weight-semibold);
  }

  .grow {
    flex: 1;
  }

  label {
    display: flex;
    align-items: center;
    gap: var(--space-sm);
    color: var(--text-secondary);
  }

  select {
    padding: var(--space-xs) var(--space-sm);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-sm);
    background: var(--bg-window);
    color: var(--text-primary);
  }

  .summary,
  .note {
    margin: 0;
    color: var(--text-secondary);
  }

  .empty {
    margin: var(--space-xl) 0;
    color: var(--text-secondary);
    text-align: center;
  }

  .error {
    margin: 0;
    color: var(--status-danger);
  }

  .list {
    flex: 1;
    margin: 0;
    padding: 0;
    overflow-y: auto;
    list-style: none;
    border: 1px solid var(--border-default);
    border-radius: var(--radius-md);
  }

  /* Rows that are off screen are not laid out or painted. */
  .list li {
    display: flex;
    align-items: center;
    gap: var(--space-md);
    padding: var(--space-xs) var(--space-md);
    border-bottom: 1px solid var(--border-default);
    content-visibility: auto;
    contain-intrinsic-size: auto 56px;
  }

  .row {
    flex: 1;
    min-width: 0;
    gap: var(--space-md);
    color: var(--text-primary);
  }

  .thumb {
    display: grid;
    flex-shrink: 0;
    place-items: center;
    width: 40px;
    height: 40px;
  }

  .text {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-width: 0;
  }

  .fname,
  .where {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .where,
  .muted {
    color: var(--text-secondary);
    font-size: var(--font-size-caption);
  }

  .size {
    min-width: 72px;
    text-align: right;
    font-weight: var(--font-weight-medium);
  }

  .show {
    padding: var(--space-xs) var(--space-sm);
    border-radius: var(--radius-sm);
    color: var(--accent-default);
  }

  .show:hover {
    background: var(--bg-hover);
  }

  footer {
    justify-content: space-between;
  }

  .go {
    padding: var(--space-sm) var(--space-lg);
    border-radius: var(--radius-md);
    background: var(--accent-default);
    color: var(--text-on-accent);
    font-weight: var(--font-weight-medium);
  }

  .go:disabled {
    background: var(--bg-control);
    color: var(--text-tertiary);
  }
</style>
