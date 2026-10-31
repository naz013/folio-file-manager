<script lang="ts">
  import { onMount } from "svelte";
  import type { Choice, Conflict } from "$lib/api/fs";
  import { formatDate, formatSize } from "$lib/format";
  import FolderIcon from "./FolderIcon.svelte";
  import Thumbnail from "./Thumbnail.svelte";

  let {
    conflict,
    remaining,
    verb,
    ondecide,
    oncancel,
  }: {
    conflict: Conflict;
    /** Conflicts still to come after this one. */
    remaining: number;
    verb: "copy" | "move";
    ondecide: (choice: Choice, all: boolean) => void;
    oncancel: () => void;
  } = $props();

  let all = $state(false);
  let keepBoth: HTMLButtonElement;

  // Keep both is the safe default: nothing is replaced or lost.
  onMount(() => keepBoth.focus());

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      oncancel();
    }
  }

  /** What "newer" means for the two sides, so the choice is easy to make. */
  let hint = $derived.by(() => {
    const [a, b] = [conflict.sourceModified, conflict.existingModified];
    if (a == null || b == null || a === b) return "";
    return a > b ? "The incoming one is newer." : "The existing one is newer.";
  });
</script>

<svelte:window {onkeydown} />

<div class="backdrop" role="presentation">
  <div class="dialog" role="alertdialog" aria-labelledby="conflict-title" aria-modal="true">
    <h2 id="conflict-title">“{conflict.name}” already exists here</h2>
    <p class="sub">Choose what to do with the {verb === "copy" ? "copy" : "move"}.</p>

    <div class="compare">
      {#each [{ title: "Existing", dir: conflict.existingIsDir, path: conflict.existing, size: conflict.existingSize, at: conflict.existingModified }, { title: "Incoming", dir: conflict.sourceIsDir, path: conflict.source, size: conflict.sourceSize, at: conflict.sourceModified }] as side (side.title)}
        <div class="side">
          <span class="which">{side.title}</span>
          <div class="preview">
            {#if side.dir}
              <FolderIcon size={64} />
            {:else}
              <Thumbnail path={side.path} name={conflict.name} size={64} />
            {/if}
          </div>
          <span class="meta">{side.dir ? "Folder" : formatSize(side.size)}</span>
          <span class="meta">{formatDate(side.at)}</span>
        </div>
      {/each}
    </div>
    {#if hint}<p class="hint">{hint}</p>{/if}

    <div class="buttons">
      <button
        class="danger"
        disabled={!conflict.canReplace}
        onclick={() => ondecide("replace", all)}
      >
        Replace
      </button>
      <button onclick={() => ondecide("skip", all)}>Skip</button>
      <button class="primary" bind:this={keepBoth} onclick={() => ondecide("both", all)}>
        Keep both
      </button>
    </div>
    <p class="note">
      {#if conflict.canReplace && conflict.existingIsDir}
        Replace moves the whole existing folder, and everything in it, to the Recycle Bin. Folders
        are not merged.
      {:else if conflict.canReplace}
        Replace moves the existing item to the Recycle Bin.
      {:else}
        This one can’t be replaced here: it is, or holds, the item itself, or the drive has no
        Recycle Bin.
      {/if}
    </p>

    <div class="foot">
      {#if remaining > 0}
        <label>
          <input type="checkbox" bind:checked={all} />
          Do this for the other {remaining}
          {remaining === 1 ? "conflict" : "conflicts"}
        </label>
      {:else}
        <span></span>
      {/if}
      <button class="link" onclick={oncancel}>Cancel {verb}</button>
    </div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 50;
    display: grid;
    place-items: center;
    background: rgb(0 0 0 / 0.35);
  }

  .dialog {
    width: min(480px, calc(100vw - 2 * var(--space-lg)));
    padding: var(--space-xl);
    border: 1px solid var(--border-default);
    border-radius: var(--radius-xl);
    background: var(--bg-window);
    box-shadow: var(--shadow-popover);
  }

  h2 {
    margin: 0;
    font-size: var(--font-size-title);
    font-weight: var(--font-weight-semibold);
    overflow-wrap: anywhere;
  }

  .sub,
  .note,
  .hint {
    margin: var(--space-xs) 0 0;
    color: var(--text-secondary);
    font-size: var(--font-size-caption);
  }

  .hint {
    text-align: center;
  }

  .compare {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--space-md);
    margin-top: var(--space-lg);
  }

  .side {
    display: grid;
    justify-items: center;
    gap: var(--space-2xs);
    padding: var(--space-md);
    border-radius: var(--radius-lg);
    background: var(--bg-subtle);
  }

  .which {
    color: var(--text-tertiary);
    font-size: var(--font-size-caption);
    font-weight: var(--font-weight-semibold);
    text-transform: uppercase;
    letter-spacing: 0.4px;
  }

  .preview {
    display: grid;
    place-items: center;
    width: 64px;
    height: 64px;
    margin: var(--space-xs) 0;
  }

  .meta {
    color: var(--text-secondary);
    font-size: var(--font-size-caption);
    font-variant-numeric: tabular-nums;
  }

  .buttons {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-sm);
    margin-top: var(--space-lg);
  }

  .buttons button {
    padding: var(--space-xs) var(--space-md);
    border-radius: var(--radius-md);
    background: var(--bg-control);
    color: var(--text-primary);
    font-weight: var(--font-weight-medium);
  }
  .buttons button:hover:not(:disabled) {
    background: var(--bg-hover);
  }
  .buttons button:disabled {
    opacity: 0.4;
  }
  .buttons .primary {
    background: var(--accent-default);
    color: var(--text-on-accent);
  }
  .buttons .primary:hover {
    background: var(--accent-hover);
  }
  .buttons .danger {
    color: var(--status-danger);
  }

  .note {
    text-align: right;
  }

  .foot {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: var(--space-md);
    margin-top: var(--space-lg);
    color: var(--text-secondary);
    font-size: var(--font-size-caption);
  }

  .foot label {
    display: flex;
    align-items: center;
    gap: var(--space-xs);
  }

  .link {
    color: var(--text-secondary);
    text-decoration: underline;
  }
</style>
