<script lang="ts">
  import { onMount } from "svelte";
  import {
    cleanupDuplicates,
    cleanupDuplicatesCancel,
    cleanupLargeOld,
    cleanupOldDownloads,
    cleanupTrash,
    cleanupTrashDuplicates,
    onQueueUpdate,
    parentOf,
    scanRoots,
    type BigFile,
    type DuplicateGroup,
  } from "$lib/api/fs";
  import { formatDate, formatSize } from "$lib/format";
  import {
    CLEANUP_AGES_DAYS,
    DOWNLOAD_AGES_DAYS,
    DUPLICATE_SIZES_MB,
    CLEANUP_SIZES_MB,
    settings,
    updateSettings,
  } from "$lib/settings.svelte";
  import Thumbnail from "./Thumbnail.svelte";

  let {
    start = "large",
    onclose,
    onopen,
  }: {
    /** Which list to open on. */
    start?: "large" | "downloads" | "duplicates";
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
  // `start` only picks the list the view opens on; after that the person switches it.
  // What is looked for: big files nobody touches, or installers and archives in Downloads.
  // svelte-ignore state_referenced_locally
  let mode = $state<"large" | "downloads" | "duplicates">(start);
  /** The Recycle Bin job started from here; the list refreshes when it ends. */
  let trashJob = $state<number | null>(null);

  const name = (p: string) => p.split(/[\\/]/).filter(Boolean).pop() ?? p;
  const sizeLabel = (mb: number) => (mb >= 1000 ? `${mb / 1000} GB` : `${mb} MB`);
  const ageLabel = (days: number) =>
    days >= 365 ? `${days / 365} ${days === 365 ? "year" : "years"}` : `${days} days`;

  /** Duplicate groups, each with the copy to keep (the newest, until another is chosen) and
   * whether the person left the group alone. */
  let groups = $state.raw<DuplicateGroup[]>([]);
  let complete = $state(true);
  let unreadable = $state(0);
  const keep = $state<Record<string, string>>({});
  const skipped = $state<Record<string, boolean>>({});
  const keptIn = (g: DuplicateGroup) => keep[g.id] ?? g.files[0].path;
  /** What would move: in each group not left alone, every copy but the kept one. */
  const dupRemoval = $derived(
    groups
      .filter((g) => !skipped[g.id])
      .map((g) => ({
        files: g.files,
        remove: g.files.filter((f) => f.path !== keptIn(g)).map((f) => f.path),
      })),
  );

  const chosen = $derived(files.filter((f) => picked[f.path]));
  const chosenCount = $derived(
    mode === "duplicates" ? dupRemoval.reduce((n, g) => n + g.remove.length, 0) : chosen.length,
  );
  const chosenBytes = $derived(
    mode === "duplicates"
      ? dupRemoval.reduce(
          (sum, g) =>
            sum + g.files.filter((f) => g.remove.includes(f.path)).reduce((s, f) => s + f.size, 0),
          0,
        )
      : chosen.reduce((sum, f) => sum + f.size, 0),
  );
  const totalBytes = $derived(files.reduce((sum, f) => sum + f.size, 0));

  async function load() {
    const id = ++loadId;
    // A search for duplicates still running is of no use to a list that replaces it.
    if (mode !== "duplicates") cleanupDuplicatesCancel().catch(() => {});
    loading = true;
    error = "";
    try {
      if (mode === "duplicates") {
        const [found, roots] = await Promise.all([
          cleanupDuplicates(settings.cleanupDupMinMb * 1024 * 1024),
          scanRoots().catch(() => []),
        ]);
        if (id !== loadId) return;
        groups = found.groups;
        complete = found.complete;
        unreadable = found.unreadable;
        files = [];
        scannedAny = roots.length > 0;
        // Choices only stay for groups that are still there.
        for (const key of Object.keys(keep)) {
          const g = found.groups.find((x) => x.id === key);
          if (!g || !g.files.some((f) => f.path === keep[key])) delete keep[key];
        }
        for (const key of Object.keys(skipped)) {
          if (!found.groups.some((x) => x.id === key)) delete skipped[key];
        }
        return;
      }
      const [found, roots] = await Promise.all([
        mode === "large"
          ? cleanupLargeOld(settings.cleanupMinMb * 1024 * 1024, settings.cleanupAgeDays, LIMIT)
          : cleanupOldDownloads(settings.cleanupDownloadsDays, LIMIT),
        scanRoots().catch(() => []),
      ]);
      if (id !== loadId) return;
      files = found;
      groups = [];
      // Only the large-file list comes from the scan; Downloads is read directly.
      scannedAny = mode === "downloads" || roots.length > 0;
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
    void settings.cleanupDownloadsDays;
    void settings.cleanupDupMinMb;
    void mode;
    load();
  });

  let moving = $state(false);

  async function trash() {
    if (moving || !chosenCount) return;
    moving = true;
    try {
      // The core moves only files still exactly as listed and still allowed, as a normal
      // Recycle Bin job (so it can be undone). The list is looked at again when it ends.
      // For duplicates it also makes sure a copy of each group stays.
      const done =
        mode === "duplicates"
          ? await cleanupTrashDuplicates(dupRemoval.filter((g) => g.remove.length))
          : await cleanupTrash(chosen);
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
      cleanupDuplicatesCancel().catch(() => {});
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
    <h2 id="cleanup-title">Cleanup</h2>
    <span class="grow"></span>
    <button onclick={onclose} aria-label="Close cleanup">✕</button>
  </header>

  <div class="modes" role="group" aria-label="What to look for">
    <button
      class:chosen={mode === "large"}
      aria-pressed={mode === "large"}
      onclick={() => (mode = "large")}
    >
      Large &amp; old files
    </button>
    <button
      class:chosen={mode === "downloads"}
      aria-pressed={mode === "downloads"}
      onclick={() => (mode = "downloads")}
    >
      Old downloads
    </button>
    <button
      class:chosen={mode === "duplicates"}
      aria-pressed={mode === "duplicates"}
      onclick={() => (mode = "duplicates")}
    >
      Duplicates
    </button>
  </div>

  <div class="filters">
    {#if mode === "duplicates"}
      <label>
        Files bigger than
        <select
          value={settings.cleanupDupMinMb}
          onchange={(e) => updateSettings({ cleanupDupMinMb: Number(e.currentTarget.value) })}
        >
          {#each DUPLICATE_SIZES_MB as mb (mb)}
            <option value={mb}>{sizeLabel(mb)}</option>
          {/each}
        </select>
      </label>
    {:else if mode === "downloads"}
      <label>
        Installers and archives not changed for
        <select
          value={settings.cleanupDownloadsDays}
          onchange={(e) => updateSettings({ cleanupDownloadsDays: Number(e.currentTarget.value) })}
        >
          {#each DOWNLOAD_AGES_DAYS as days (days)}
            <option value={days}>{ageLabel(days)}</option>
          {/each}
        </select>
      </label>
    {:else}
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
    {/if}
  </div>

  {#if error}<p class="error" role="alert">{error}</p>{/if}

  {#if mode === "duplicates"}
    {#if loading && !groups.length}
      <p class="empty" role="status">Comparing files. This can take a while for big files…</p>
    {:else if !scannedAny}
      <p class="empty" role="status">
        Nothing has been scanned yet. Scan a drive in Storage first; duplicates are looked for among
        the scanned files.
      </p>
    {:else if !groups.length}
      <p class="empty" role="status">No identical files found.</p>
    {:else}
      <p class="summary" role="status">
        {groups.length}
        {groups.length === 1 ? "group" : "groups"} of identical files. The newest copy of each is kept
        (not one in Downloads, Desktop or Temp, if there is another) unless you pick another; the others
        move to the Recycle Bin. At least one copy always stays, and each is compared with the copy that
        stays just before it moves.
        {#if !complete}The search stopped at its limits, so there may be more.{/if}
        {#if unreadable}{unreadable}
          {unreadable === 1 ? "file" : "files"} could not be read and
          {unreadable === 1 ? "was" : "were"} left out.{/if}
      </p>
      <ul class="list groups" aria-label="Groups of identical files">
        {#each groups as g (g.id)}
          {@const kept = keptIn(g)}
          <li class="group" class:left={skipped[g.id]}>
            <div class="ghead">
              <span class="fname">
                {g.files.length} copies, {formatSize(g.size)} each: {formatSize(
                  g.size * (g.files.length - 1),
                )} to gain
              </span>
              <label class="check">
                <input
                  type="checkbox"
                  aria-label="Leave the group with {name(g.files[0].path)} alone"
                  bind:checked={skipped[g.id]}
                />
                Leave this group alone
              </label>
            </div>
            <div role="radiogroup" aria-label="Which copy of {name(g.files[0].path)} to keep">
              {#each g.files as f (f.path)}
                <div class="copy" class:keeping={f.path === kept}>
                  <label class="row">
                    <input
                      type="radio"
                      name="keep-{g.id}"
                      checked={f.path === kept}
                      disabled={skipped[g.id]}
                      aria-label="Keep {name(f.path)} in {parentOf(f.path) ?? ''}"
                      onchange={() => (keep[g.id] = f.path)}
                    />
                    <span class="thumb"
                      ><Thumbnail path={f.path} name={name(f.path)} size={40} /></span
                    >
                    <span class="text">
                      <span class="fname">{name(f.path)}</span>
                      <span class="where">{parentOf(f.path) ?? ""}</span>
                    </span>
                    <span class="muted">{formatDate(f.modified * 1000)}</span>
                    <span class="verdict">
                      {skipped[g.id] ? "left alone" : f.path === kept ? "kept" : "to Recycle Bin"}
                    </span>
                  </label>
                  <button
                    class="show"
                    aria-label="Show {name(f.path)} in folder"
                    onclick={() => {
                      const dir = parentOf(f.path);
                      if (dir) onopen(dir);
                    }}>Show in folder</button
                  >
                </div>
              {/each}
            </div>
          </li>
        {/each}
      </ul>
    {/if}
  {:else if loading && !files.length}
    <p class="empty" role="status">Looking…</p>
  {:else if !scannedAny}
    <p class="empty" role="status">
      Nothing has been scanned yet. Scan a drive in Storage first; cleanup suggests files from the
      scan.
    </p>
  {:else if !files.length}
    <p class="empty" role="status">
      {#if mode === "downloads"}
        No installer or archive in Downloads has been untouched for {ageLabel(
          settings.cleanupDownloadsDays,
        )}.
      {:else}
        No file is bigger than {sizeLabel(settings.cleanupMinMb)} and untouched for {ageLabel(
          settings.cleanupAgeDays,
        )}.
      {/if}
    </p>
  {:else}
    {#if mode === "downloads"}
      <p class="note">
        Archives can be the only copy of something. A folder with other files in it (an unpacked
        program) is skipped.
      </p>
    {/if}
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
    <button class="go" disabled={!chosenCount || moving || trashJob !== null} onclick={trash}>
      {chosenCount
        ? `Move ${chosenCount} ${chosenCount === 1 ? "file" : "files"} (${formatSize(chosenBytes)}) to Recycle Bin`
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

  .modes {
    display: flex;
    gap: var(--space-xs);
    width: fit-content;
    padding: var(--space-2xs);
    border-radius: var(--radius-md);
    background: var(--bg-control);
  }

  .modes button {
    padding: var(--space-xs) var(--space-md);
    border-radius: var(--radius-sm);
    color: var(--text-secondary);
  }

  .modes button.chosen {
    background: var(--bg-window);
    color: var(--text-primary);
    font-weight: var(--font-weight-medium);
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

  .list li.group {
    flex-direction: column;
    align-items: stretch;
    gap: 0;
    padding: var(--space-sm) var(--space-md);
    contain-intrinsic-size: auto 160px;
  }

  .group.left {
    opacity: 0.6;
  }

  .ghead {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-md);
    padding-bottom: var(--space-xs);
  }

  .check {
    gap: var(--space-xs);
  }

  .copy {
    display: flex;
    align-items: center;
    gap: var(--space-md);
    padding: var(--space-2xs) 0;
  }

  .verdict {
    min-width: 96px;
    color: var(--text-secondary);
    text-align: right;
  }

  .copy.keeping .verdict {
    color: var(--accent-default);
    font-weight: var(--font-weight-medium);
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
