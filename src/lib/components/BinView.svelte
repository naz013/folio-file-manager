<script lang="ts">
  import { onMount } from "svelte";
  import { binName } from "$lib/shortcuts";
  import { binList, binPurge, binRestore, binSizes, type BinItem, type BinSize } from "$lib/api/fs";
  import { formatDate, formatSize } from "$lib/format";
  import EmptyState from "./EmptyState.svelte";

  let {
    onrestored,
    dropping = false,
  }: {
    /** Items were put back, so the folder shown may have changed. */
    onrestored: () => void;
    /** Something is being dragged over the list, to be moved to the bin. */
    dropping?: boolean;
  } = $props();

  let items = $state<BinItem[]>([]);
  /** Item id → what it weighs, which arrives after the list. */
  let sizes = $state<Record<string, BinSize>>({});
  let loading = $state(true);
  let error = $state("");
  let chosen = $state<string[]>([]);
  /** What is waiting for a confirmation: deleting the chosen items, or everything. */
  let asking = $state<"chosen" | "all" | null>(null);
  let busy = $state(false);

  const parentOf = (path: string) => path.replace(/[\\/][^\\/]*$/, "");
  /** The items the pending confirmation is about, as they were listed. */
  const asked = $derived(asking === "all" ? items : items.filter((i) => chosen.includes(i.id)));
  const refs = (list: BinItem[]) =>
    list.map(({ id, original, deleted }) => ({ id, original, deleted }));

  async function load() {
    loading = true;
    try {
      items = await binList();
      // Only items that are still there stay chosen.
      chosen = chosen.filter((id) => items.some((i) => i.id === id));
      error = "";
    } catch (e) {
      error = String(e);
    }
    loading = false;
    // The system is slow to say what things weigh: the list is shown first.
    binSizes().then(
      (all) => (sizes = Object.fromEntries(all.map((s) => [s.id, s]))),
      () => {},
    );
  }

  function toggle(id: string) {
    chosen = chosen.includes(id) ? chosen.filter((c) => c !== id) : [...chosen, id];
  }

  async function act(run: () => Promise<void>) {
    busy = true;
    error = "";
    try {
      await run();
    } catch (e) {
      error = String(e);
    }
    asking = null;
    busy = false;
    await load();
  }

  const restore = () =>
    act(async () => {
      await binRestore(refs(items.filter((i) => chosen.includes(i.id))));
      onrestored();
    });

  const purge = () => act(() => binPurge(refs(asked)));

  const sizeOf = (i: BinItem) => {
    const s = sizes[i.id];
    if (!s) return "";
    if (s.size !== null) return formatSize(s.size);
    return s.entries !== null ? `${s.entries} ${s.entries === 1 ? "item" : "items"}` : "";
  };

  onMount(() => {
    load();
  });

  function onkeydown(e: KeyboardEvent) {
    // A confirmation open: Escape answers it, and nothing else acts behind it.
    if (e.key === "Escape" && asking) {
      e.preventDefault();
      e.stopPropagation();
      asking = null;
    }
  }

  const bin = binName();
</script>

<!-- Only answers Escape for the confirmation inside it. -->
<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<section class="bin" class:dropping aria-label={bin} data-drop-bin {onkeydown}>
  <header>
    <span class="note">{items.length} {items.length === 1 ? "item" : "items"}</span>
    <span class="grow"></span>
    <button class="primary" disabled={!chosen.length || busy || asking !== null} onclick={restore}>
      Restore{chosen.length ? ` ${chosen.length}` : ""}
    </button>
    <button class="danger" disabled={!chosen.length || busy} onclick={() => (asking = "chosen")}>
      Delete permanently…
    </button>
    <button class="danger" disabled={!items.length || busy} onclick={() => (asking = "all")}>
      Empty {bin}…
    </button>
  </header>

  {#if asking}
    <div class="confirm" role="alertdialog" aria-label="Confirm deleting for good">
      <p>
        Delete {asked.length === 1 ? `"${asked[0].name}"` : `${asked.length} items`} permanently? This
        can't be undone, and nothing can be brought back from the {bin}.
      </p>
      <button onclick={() => (asking = null)}>Cancel</button>
      <button class="danger" disabled={busy} onclick={purge}>Delete permanently</button>
    </div>
  {/if}

  {#if error}
    <p class="error" role="alert">{error}</p>
  {/if}

  {#if loading && !items.length}
    <EmptyState icon="trash" text={`Reading the ${bin}…`} />
  {:else if !items.length}
    <EmptyState
      icon="trash"
      text={`The ${bin} is empty. Deleted files wait here until you empty it.`}
    />
  {:else}
    <ul class="list" aria-label={`Items in the ${bin}`}>
      {#each items as item (item.id)}
        <li class:chosen={chosen.includes(item.id)}>
          <label>
            <input
              type="checkbox"
              checked={chosen.includes(item.id)}
              disabled={asking !== null}
              onchange={() => toggle(item.id)}
            />
            <span class="name" title={item.name}>{item.name}</span>
            <span class="where" title={item.original}>{parentOf(item.original)}</span>
            <span class="muted">{formatDate(item.deleted * 1000)}</span>
            <span class="muted num">{sizeOf(item)}</span>
          </label>
        </li>
      {/each}
    </ul>
  {/if}
</section>

<style>
  .bin {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-md);
    padding: var(--space-md) var(--space-lg);
    background: var(--bg-window);
  }

  .bin.dropping {
    outline: 2px solid var(--accent-default);
    outline-offset: -2px;
  }

  header {
    display: flex;
    align-items: center;
    gap: var(--space-sm);
  }

  .grow {
    flex: 1;
  }

  button {
    padding: var(--space-xs) var(--space-md);
    border: 1px solid var(--border-default);
    border-radius: var(--radius-md);
    background: var(--bg-control);
    color: var(--text-primary);
    font: inherit;
  }

  button:disabled {
    color: var(--text-tertiary);
  }

  .primary {
    border-color: transparent;
    background: var(--accent-default);
    color: var(--text-on-accent);
    font-weight: var(--font-weight-medium);
  }

  .primary:disabled {
    background: var(--bg-control);
    color: var(--text-tertiary);
  }

  .danger:not(:disabled) {
    color: var(--status-danger);
  }

  .note,
  .muted {
    color: var(--text-secondary);
    font-size: var(--font-size-caption);
  }

  .confirm {
    display: flex;
    align-items: center;
    gap: var(--space-md);
    padding: var(--space-md);
    border: 1px solid var(--status-danger);
    border-radius: var(--radius-lg);
  }

  .confirm p {
    flex: 1;
    margin: 0;
  }

  .error {
    margin: 0;
    color: var(--status-danger);
  }

  .list {
    flex: 1;
    min-height: 0;
    margin: 0;
    padding: 0;
    overflow-y: auto;
    list-style: none;
  }

  li label {
    display: grid;
    grid-template-columns: 20px 1fr 180px 190px 90px;
    align-items: center;
    column-gap: var(--space-md);
    padding: var(--space-xs) var(--space-sm);
    border-radius: var(--radius-md);
  }

  li:hover label,
  li.chosen label {
    background: var(--bg-hover);
  }

  .name,
  .where {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .where {
    color: var(--text-secondary);
  }

  .num {
    text-align: right;
  }
</style>
