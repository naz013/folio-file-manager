<script lang="ts">
  import { onMount } from "svelte";
  import type { SmartCriteria, SmartFolder, Tag } from "$lib/api/fs";
  import { tagColor } from "$lib/tags";

  let {
    tags,
    folder = null,
    here,
    onsave,
    ondelete,
    onclose,
  }: {
    tags: Tag[];
    /** The smart folder being changed; null for a new one. */
    folder?: SmartFolder | null;
    /** The folder the window shows, offered as the place to look. */
    here: string;
    /** Resolves to why it can't be saved, or null once it is. */
    onsave: (id: number | null, name: string, criteria: SmartCriteria) => Promise<string | null>;
    ondelete?: (folder: SmartFolder) => void;
    onclose: () => void;
  } = $props();

  /** The usual "changed in the last" choices; a saved folder's own number is added if it is not one. */
  const WINDOWS: { days: number | null; label: string }[] = [
    { days: null, label: "Any time" },
    { days: 1, label: "Day" },
    { days: 7, label: "Week" },
    { days: 30, label: "Month" },
    { days: 90, label: "3 months" },
    { days: 365, label: "Year" },
  ];

  let dialog: HTMLElement;
  /** Where focus was, so closing the dialog gives it back. */
  const opener = document.activeElement as HTMLElement | null;
  onMount(() => {
    dialog.focus();
    return () => opener?.focus?.();
  });

  // The fields start from the folder being changed and are the person's own from then on.
  // svelte-ignore state_referenced_locally
  const start = folder;
  let name = $state(start?.name ?? "");
  let chosen = $state<number[]>(start?.criteria.tags ?? []);
  let text = $state(start?.criteria.text ?? "");
  let days = $state<number | null>(start?.criteria.modifiedDays ?? null);
  let place = $state(start?.criteria.folder ?? "");
  let error = $state("");
  let saving = $state(false);

  const windows = $derived(
    days === null || WINDOWS.some((w) => w.days === days)
      ? WINDOWS
      : [...WINDOWS, { days, label: `${days} days` }],
  );
  /** Tags that were deleted since the folder was saved stay out of the choices. */
  const known = $derived(new Set(tags.map((t) => t.id)));
  const lostTag = $derived(chosen.some((id) => !known.has(id)));

  function toggle(id: number) {
    chosen = chosen.includes(id) ? chosen.filter((t) => t !== id) : [...chosen, id];
    error = "";
  }

  async function save() {
    if (saving) return;
    saving = true;
    error =
      (await onsave(folder?.id ?? null, name, {
        // A tag that was deleted can't be seen or unticked here: it is dropped on saving.
        tags: chosen.filter((id) => known.has(id)),
        text,
        modifiedDays: days,
        folder: place.trim() || null,
      })) ?? "";
    saving = false;
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
    role="dialog"
    aria-modal="true"
    aria-labelledby="smart-title"
    tabindex="-1"
    onpointerdown={(e) => e.stopPropagation()}
    {onkeydown}
  >
    <h2 id="smart-title">{folder ? "Edit smart folder" : "New smart folder"}</h2>
    <p class="lead">
      A saved search. It is worked out again each time you open it, so it is always up to date.
    </p>

    <form
      onsubmit={(e) => {
        e.preventDefault();
        save();
      }}
    >
      <label class="field">
        <span>Name</span>
        <input
          maxlength="60"
          spellcheck="false"
          placeholder="Work this week"
          bind:value={name}
          oninput={() => (error = "")}
        />
      </label>

      <fieldset class="field">
        <legend>Has all of these tags</legend>
        <div class="chips">
          {#each tags as tag (tag.id)}
            <button
              type="button"
              class="chip"
              class:on={chosen.includes(tag.id)}
              aria-pressed={chosen.includes(tag.id)}
              onclick={() => toggle(tag.id)}
            >
              <span class="dot" style:background={tagColor(tag.color)}></span>{tag.name}
            </button>
          {/each}
        </div>
        {#if lostTag}
          <p class="warn">A tag this folder used has been deleted. Choose another and save.</p>
        {/if}
      </fieldset>

      <label class="field">
        <span>Name contains</span>
        <input
          spellcheck="false"
          placeholder="report, or *.psd"
          bind:value={text}
          oninput={() => (error = "")}
        />
      </label>

      <fieldset class="field">
        <legend>Changed in the last</legend>
        <div class="chips">
          {#each windows as w (w.days)}
            <button
              type="button"
              class="chip"
              class:on={days === w.days}
              aria-pressed={days === w.days}
              onclick={() => {
                days = w.days;
                error = "";
              }}
            >
              {w.label}
            </button>
          {/each}
        </div>
      </fieldset>

      <label class="field">
        <span>Look in {chosen.length ? "(optional)" : "(needed without a tag)"}</span>
        <span class="where">
          <input
            spellcheck="false"
            placeholder="Anywhere"
            bind:value={place}
            oninput={() => (error = "")}
          />
          <button type="button" class="plain" onclick={() => (place = here)} disabled={!here}>
            This folder
          </button>
        </span>
      </label>

      {#if error}<p class="error" role="alert">{error}</p>{/if}

      <footer>
        {#if folder && ondelete}
          <button type="button" class="danger" onclick={() => ondelete(folder)}>Delete</button>
        {/if}
        <span class="spacer"></span>
        <button type="button" class="plain" onclick={onclose}>Cancel</button>
        <button type="submit" class="primary" disabled={saving || !name.trim()}>Save</button>
      </footer>
    </form>
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
    width: min(460px, calc(100vw - 2 * var(--space-xl)));
    max-height: calc(100vh - 2 * var(--space-xl));
    padding: var(--space-xl);
    overflow-y: auto;
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

  .lead {
    margin: var(--space-xs) 0 var(--space-md);
    color: var(--text-secondary);
    font-size: var(--font-size-caption);
  }

  form {
    display: grid;
    gap: var(--space-md);
  }

  .field {
    display: grid;
    gap: var(--space-xs);
    min-width: 0;
    margin: 0;
    padding: 0;
    border: 0;
  }

  .field > span,
  legend {
    padding: 0;
    color: var(--text-secondary);
    font-size: var(--font-size-caption);
  }

  input {
    flex: 1;
    min-width: 0;
    padding: var(--space-xs) var(--space-sm);
    border: 1px solid var(--border-default);
    border-radius: var(--radius-md);
    background: var(--bg-control);
    color: var(--text-primary);
    font: inherit;
  }

  .where {
    display: flex;
    gap: var(--space-sm);
  }

  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-xs);
  }

  .chip {
    display: inline-flex;
    align-items: center;
    gap: var(--space-xs);
    padding: var(--space-2xs) var(--space-sm);
    border: 1px solid var(--border-default);
    border-radius: var(--radius-md);
    font-size: var(--font-size-caption);
  }

  .chip:hover {
    background: var(--bg-hover);
  }

  .chip.on {
    border-color: var(--accent-default);
    background: var(--bg-selected);
  }

  .dot {
    width: 10px;
    height: 10px;
    border-radius: 50%;
  }

  footer {
    display: flex;
    align-items: center;
    gap: var(--space-sm);
    padding-top: var(--space-md);
    border-top: 1px solid var(--border-default);
  }

  .spacer {
    flex: 1;
  }

  .primary,
  .plain,
  .danger {
    padding: var(--space-xs) var(--space-md);
    border-radius: var(--radius-md);
    font-weight: var(--font-weight-medium);
  }

  .primary {
    background: var(--accent-default);
    color: var(--text-on-accent);
  }

  .primary:disabled {
    background: var(--bg-control);
    color: var(--text-tertiary);
  }

  .plain {
    background: var(--bg-control);
  }

  .plain:disabled {
    color: var(--text-tertiary);
  }

  .danger {
    color: var(--status-danger);
  }

  .error,
  .warn {
    margin: 0;
    font-size: var(--font-size-caption);
  }

  .error {
    color: var(--status-danger);
  }

  .warn {
    color: var(--text-secondary);
  }
</style>
