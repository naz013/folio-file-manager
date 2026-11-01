<script lang="ts">
  import { onMount, tick } from "svelte";
  import { pdfCompress, pdfMerge, type Entry, type PdfPreset } from "$lib/api/fs";
  import { formatSize } from "$lib/format";
  import Icon from "./Icon.svelte";

  let {
    pdfs,
    onclose,
  }: {
    /** The PDFs to work on: the ones selected, or the one being previewed. */
    pdfs: Entry[];
    /** Closed, whether or not a job was started. */
    onclose: () => void;
  } = $props();

  const presets: { id: PdfPreset; label: string; text: string }[] = [
    { id: "lossless", label: "Lossless", text: "Repacks the file. Looks identical; saves little." },
    { id: "high", label: "High quality", text: "Pictures re-saved at high quality." },
    { id: "balanced", label: "Balanced", text: "Pictures re-saved at medium quality." },
    { id: "small", label: "Small", text: "Pictures at 70% size and lower quality." },
    { id: "smallest", label: "Smallest", text: "Pictures at 35% size and low quality." },
  ];

  // The dialog works on the files it was opened with; the list does not change under it.
  // svelte-ignore state_referenced_locally
  const canMerge = pdfs.length >= 2;
  // Merging needs two files, so a single file opens on Compress.
  let mode = $state<"merge" | "compress">(canMerge ? "merge" : "compress");
  /** The order the files are merged in. */
  // svelte-ignore state_referenced_locally
  let order = $state<Entry[]>([...pdfs]);
  let preset = $state<PdfPreset>("balanced");
  // svelte-ignore state_referenced_locally
  let target = $state<Entry>(pdfs[0]);
  let mergedName = $state("Merged.pdf");
  let error = $state("");
  let busy = $state(false);

  const stem = (name: string) => name.replace(/\.pdf$/i, "");
  // Follows the chosen PDF; typing over it keeps the typed name until another PDF is chosen.
  let compressedName = $derived(`${stem(target.name)} (compressed).pdf`);

  const withExtension = (typed: string) =>
    /\.pdf$/i.test(typed.trim()) ? typed.trim() : `${typed.trim()}.pdf`;

  function move(i: number, by: -1 | 1) {
    const j = i + by;
    if (j < 0 || j >= order.length) return;
    const next = [...order];
    [next[i], next[j]] = [next[j], next[i]];
    order = next;
    // The button that moved the item is disabled at an end; focus follows the item.
    tick().then(
      () =>
        dialog
          .querySelector<HTMLElement>(`[data-move="${by}"][data-i="${j}"]:not(:disabled)`)
          ?.focus() ?? dialog.querySelector<HTMLElement>(`[data-i="${j}"]:not(:disabled)`)?.focus(),
    );
  }

  async function start() {
    error = "";
    busy = true;
    try {
      if (mode === "merge") {
        await pdfMerge(
          order.map((e) => e.path),
          withExtension(mergedName),
        );
      } else {
        await pdfCompress(target.path, preset, withExtension(compressedName));
      }
      onclose();
    } catch (e) {
      error = String(e);
      busy = false;
    }
  }

  let dialog: HTMLElement;
  /** Where focus was, so closing the dialog gives it back. */
  const opener = document.activeElement as HTMLElement | null;
  onMount(() => {
    dialog.focus();
    return () => opener?.focus?.();
  });

  function onkeydown(e: KeyboardEvent) {
    // Nothing behind the dialog may act on keys while it is open.
    e.stopPropagation();
    if (e.key === "Escape") {
      e.preventDefault();
      onclose();
    }
    if (e.key === "Enter" && (e.target as HTMLElement).tagName === "INPUT" && !busy) {
      e.preventDefault();
      start();
    }
    if (e.key === "Tab") {
      const items = [
        ...dialog.querySelectorAll<HTMLElement>("button:not(:disabled), input, select"),
      ];
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
    aria-labelledby="pdf-title"
    tabindex="-1"
    {onkeydown}
  >
    <h2 id="pdf-title">PDF tools</h2>

    <div class="tabs" role="group" aria-label="Tool">
      <button
        class:chosen={mode === "merge"}
        aria-pressed={mode === "merge"}
        disabled={!canMerge}
        title={canMerge ? undefined : "Select two or more PDFs in the folder to merge them"}
        onclick={() => (mode = "merge")}
      >
        Merge
      </button>
      <button
        class:chosen={mode === "compress"}
        aria-pressed={mode === "compress"}
        onclick={() => (mode = "compress")}
      >
        Compress
      </button>
    </div>

    {#if mode === "merge"}
      <p class="note">Joined top to bottom. The originals are not changed.</p>
      <ol class="files">
        {#each order as file, i (file.path)}
          <li>
            <span class="name">{file.name}</span>
            <span class="size">{formatSize(file.size)}</span>
            <button
              aria-label="Move {file.name} up"
              data-move="-1"
              data-i={i}
              disabled={i === 0}
              onclick={() => move(i, -1)}
            >
              <Icon name="chevronUp" size={14} />
            </button>
            <button
              aria-label="Move {file.name} down"
              data-move="1"
              data-i={i}
              disabled={i === order.length - 1}
              onclick={() => move(i, 1)}
            >
              <Icon name="chevronDown" size={14} />
            </button>
          </li>
        {/each}
      </ol>
      <label class="field">
        Save as
        <input bind:value={mergedName} spellcheck="false" />
      </label>
    {:else}
      {#if pdfs.length > 1}
        <label class="field">
          PDF
          <select
            value={target.path}
            onchange={(e) =>
              (target = pdfs.find((p) => p.path === e.currentTarget.value) ?? target)}
          >
            {#each pdfs as file (file.path)}
              <option value={file.path}>{file.name} ({formatSize(file.size)})</option>
            {/each}
          </select>
        </label>
      {:else}
        <p class="note">{target.name} · {formatSize(target.size)}</p>
      {/if}
      <fieldset>
        <legend>Quality</legend>
        {#each presets as p (p.id)}
          <label class="preset">
            <input type="radio" name="preset" value={p.id} bind:group={preset} />
            <span><strong>{p.label}</strong> {p.text}</span>
          </label>
        {/each}
      </fieldset>
      <p class="note">
        A new file is saved next to the original. Only JPEG pictures are shrunk; text and drawings
        stay as they are.
      </p>
      <label class="field">
        Save as
        <input bind:value={compressedName} spellcheck="false" />
      </label>
    {/if}

    {#if error}<p class="error" role="alert">{error}</p>{/if}

    <div class="actions">
      <button onclick={onclose}>Cancel</button>
      <button class="primary" disabled={busy} onclick={start}>
        {mode === "merge" ? "Merge" : "Compress"}
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
    display: flex;
    flex-direction: column;
    gap: var(--space-md);
    width: min(520px, calc(100vw - 2 * var(--space-xl)));
    max-height: calc(100vh - 2 * var(--space-xl));
    overflow-y: auto;
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

  .tabs {
    display: flex;
    gap: var(--space-xs);
  }

  .tabs button.chosen {
    background: var(--accent-subtle);
    color: var(--accent-default);
    font-weight: var(--font-weight-medium);
  }

  .note {
    margin: 0;
    color: var(--text-secondary);
  }

  .files {
    margin: 0;
    padding: 0;
    list-style: none;
    border: 1px solid var(--border-default);
    border-radius: var(--radius-md);
  }

  .files li {
    display: flex;
    align-items: center;
    gap: var(--space-xs);
    padding: var(--space-xs) var(--space-sm);
  }

  .files li + li {
    border-top: 1px solid var(--border-default);
  }

  .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .size {
    color: var(--text-secondary);
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: var(--space-xs);
    color: var(--text-secondary);
  }

  input:not([type="radio"]),
  select {
    padding: var(--space-xs) var(--space-sm);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-md);
    background: var(--bg-window);
    color: var(--text-primary);
    font: inherit;
    user-select: text;
  }

  fieldset {
    margin: 0;
    padding: 0;
    border: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-xs);
  }

  legend {
    padding: 0;
    margin-bottom: var(--space-xs);
    color: var(--text-secondary);
  }

  .preset {
    display: flex;
    align-items: baseline;
    gap: var(--space-sm);
  }

  .error {
    margin: 0;
    padding-left: var(--space-sm);
    border-left: 3px solid var(--bg-danger);
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

  button:disabled {
    color: var(--text-tertiary);
  }

  .files button {
    padding: var(--space-2xs) var(--space-xs);
  }

  .primary {
    background: var(--accent-default);
    color: var(--text-on-accent);
    font-weight: var(--font-weight-medium);
  }

  .primary:disabled {
    color: var(--text-on-accent);
    opacity: 0.6;
  }
</style>
