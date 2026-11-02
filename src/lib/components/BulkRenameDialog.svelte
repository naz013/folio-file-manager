<script lang="ts">
  import { onMount } from "svelte";
  import { checkRenames, type Entry, type RenamePlan } from "$lib/api/fs";
  import {
    DEFAULT_RULE,
    newName,
    ruleError,
    type RenameMode,
    type RenameRule,
  } from "$lib/bulkRename";

  let {
    items,
    onapply,
    onclose,
  }: {
    /** What is renamed, in the order the numbers are handed out. */
    items: Entry[];
    /** Resolves to an error message, or null when the batch was renamed. */
    onapply: (plan: RenamePlan[]) => Promise<string | null>;
    onclose: () => void;
  } = $props();

  /** The preview shows this many rows; every row is still checked and renamed. */
  const SHOWN = 200;

  const MODES: { id: RenameMode; label: string }[] = [
    { id: "replace", label: "Find & replace" },
    { id: "number", label: "Number" },
    { id: "date", label: "Date" },
    { id: "case", label: "Case" },
    { id: "affix", label: "Prefix & suffix" },
  ];

  let dialog: HTMLElement;
  /** Where focus was, so closing the dialog gives it back. */
  const opener = document.activeElement as HTMLElement | null;
  onMount(() => {
    dialog.focus();
    return () => opener?.focus?.();
  });

  const rule = $state<RenameRule>({ ...DEFAULT_RULE });
  let busy = $state(false);
  let failure = $state("");

  const plan = $derived(
    items.map((e, i) => ({ path: e.path, newName: newName(e.name, e.isDir, i, rule, e.modified) })),
  );
  const rows = $derived(items.map((e, i) => ({ old: e.name, now: plan[i].newName })));
  const changed = $derived(rows.filter((r) => r.old !== r.now).length);
  const brokenRule = $derived(ruleError(rule));

  /** The core's verdict on each row; empty until it answers. */
  let problems = $state.raw<(string | null)[]>([]);
  /** The plan `problems` answers; a different one means the verdict is out of date. */
  let checked = $state.raw<typeof plan | null>(null);
  let asked = 0;

  // Ask the core after the typing pauses; only the newest answer counts.
  $effect(() => {
    const sent = plan;
    const id = ++asked;
    const timer = setTimeout(async () => {
      const answer = await checkRenames(sent).catch(() => null);
      if (id !== asked) return;
      if (answer) {
        problems = answer;
        checked = sent;
        failure = "";
      } else {
        failure = "Couldn't check the names.";
      }
    }, 120);
    return () => clearTimeout(timer);
  });

  const upToDate = $derived(checked === plan);
  const problemCount = $derived(upToDate ? problems.filter(Boolean).length : 0);
  const canApply = $derived(!busy && !brokenRule && changed > 0 && upToDate && problemCount === 0);

  /** Rows with a problem come first, so the cap never hides one. */
  const shown = $derived(
    upToDate
      ? [...rows.keys()]
          .sort((a, b) => Number(!!problems[b]) - Number(!!problems[a]))
          .slice(0, SHOWN)
      : [...rows.keys()].slice(0, SHOWN),
  );

  async function apply() {
    if (!canApply) return;
    busy = true;
    failure = (await onapply(plan)) ?? "";
    busy = false;
    if (!failure) onclose();
  }

  function onkeydown(e: KeyboardEvent) {
    // Nothing behind the dialog may act on keys while it is open.
    e.stopPropagation();
    if (e.key === "Escape") {
      e.preventDefault();
      onclose();
    } else if (e.key === "Enter" && !(e.target as HTMLElement).closest("button, select")) {
      e.preventDefault();
      apply();
    }
  }
</script>

<div
  class="backdrop"
  role="presentation"
  onpointerdown={(e) => e.target === e.currentTarget && onclose()}
>
  <div
    bind:this={dialog}
    class="dialog"
    role="dialog"
    aria-modal="true"
    aria-labelledby="bulk-title"
    tabindex="-1"
    onpointerdown={(e) => e.stopPropagation()}
    {onkeydown}
  >
    <header>
      <h2 id="bulk-title">Rename {items.length} items</h2>
    </header>

    <div class="modes" role="tablist" aria-label="How to rename">
      {#each MODES as m (m.id)}
        <button
          role="tab"
          class:chosen={rule.mode === m.id}
          aria-selected={rule.mode === m.id}
          onclick={() => (rule.mode = m.id)}>{m.label}</button
        >
      {/each}
    </div>

    <p class="hint">Only the name changes; the extension stays.</p>

    <form onsubmit={(e) => e.preventDefault()}>
      {#if rule.mode === "replace"}
        <label>Find <input bind:value={rule.find} spellcheck="false" /></label>
        <label>Replace with <input bind:value={rule.replace} spellcheck="false" /></label>
        <div class="checks">
          <label class="check"
            ><input type="checkbox" bind:checked={rule.caseSensitive} /> Match case</label
          >
          <label class="check"
            ><input type="checkbox" bind:checked={rule.regex} /> Regular expression</label
          >
        </div>
        {#if brokenRule}<p class="error" role="alert">{brokenRule}</p>{/if}
      {:else if rule.mode === "number"}
        <label>
          Name pattern
          <input bind:value={rule.pattern} spellcheck="false" />
        </label>
        <p class="hint">
          <code>{"{name}"}</code> is the old name, <code>{"{n}"}</code> the number.
        </p>
        <div class="checks">
          <label>Start at <input type="number" min="0" bind:value={rule.start} /></label>
          <label>Digits <input type="number" min="1" max="9" bind:value={rule.digits} /></label>
        </div>
      {:else if rule.mode === "date"}
        <label>
          Name pattern
          <input bind:value={rule.datePattern} spellcheck="false" />
        </label>
        <p class="hint">
          <code>{"{name}"}</code> is the old name, <code>{"{date}"}</code> the modified date.
        </p>
        <label>
          Date format
          <input bind:value={rule.dateFormat} spellcheck="false" />
        </label>
        <p class="hint">
          <code>YYYY</code> <code>MM</code> <code>DD</code> <code>HH</code> <code>mm</code>
          <code>ss</code>, in local time. Any other letter of these kinds is read as a part too.
          Characters a name can't hold are refused in the preview.
        </p>
      {:else if rule.mode === "case"}
        <label>
          Change to
          <select bind:value={rule.textCase}>
            <option value="lower">lower case</option>
            <option value="upper">UPPER CASE</option>
            <option value="title">Title Case</option>
            <option value="sentence">Sentence case</option>
          </select>
        </label>
      {:else}
        <label>Prefix <input bind:value={rule.prefix} spellcheck="false" /></label>
        <label
          >Suffix, before the extension <input bind:value={rule.suffix} spellcheck="false" /></label
        >
      {/if}
    </form>

    <ul class="preview" aria-label="Preview">
      {#each shown as i (i)}
        {@const row = rows[i]}
        {@const why = upToDate ? problems[i] : null}
        <li class:same={row.old === row.now} class:bad={why}>
          <span class="old">{row.old}</span>
          <span class="arrow" aria-hidden="true">→</span>
          <span class="new">{row.now}</span>
          {#if why}<span class="why">{why}</span>{/if}
        </li>
      {/each}
      {#if rows.length > SHOWN}
        <li class="more">and {rows.length - SHOWN} more</li>
      {/if}
    </ul>

    {#if problemCount}
      <p class="error" role="alert">
        {problemCount === 1 ? "1 name can't be used" : `${problemCount} names can't be used`}.
      </p>
    {/if}
    {#if failure}<p class="error" role="alert">{failure}</p>{/if}

    <footer>
      <button class="cancel" onclick={onclose}>Cancel</button>
      <button class="apply" disabled={!canApply} onclick={apply}>
        {changed === 1 ? "Rename 1 item" : `Rename ${changed} items`}
      </button>
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
    width: min(560px, calc(100vw - 2 * var(--space-xl)));
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

  .modes {
    display: flex;
    gap: var(--space-xs);
    padding: var(--space-2xs);
    border-radius: var(--radius-md);
    background: var(--bg-control);
  }

  .modes button {
    flex: 1;
    padding: var(--space-xs) var(--space-sm);
    border-radius: var(--radius-sm);
    color: var(--text-secondary);
  }

  .modes button.chosen {
    background: var(--bg-window);
    color: var(--text-primary);
    font-weight: var(--font-weight-medium);
  }

  form {
    display: flex;
    flex-direction: column;
    gap: var(--space-sm);
  }

  label {
    display: flex;
    flex-direction: column;
    gap: var(--space-2xs);
    color: var(--text-secondary);
    font-size: var(--font-size-caption);
  }

  input:not([type="checkbox"]),
  select {
    padding: var(--space-xs) var(--space-sm);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-sm);
    background: var(--bg-window);
    color: var(--text-primary);
    font-size: var(--font-size-body);
  }

  .checks {
    display: flex;
    gap: var(--space-lg);
  }

  .checks label.check {
    flex-direction: row;
    align-items: center;
    gap: var(--space-xs);
    font-size: var(--font-size-body);
  }

  .hint {
    margin: 0;
    color: var(--text-secondary);
    font-size: var(--font-size-caption);
  }

  .preview {
    flex: 1;
    min-height: 80px;
    max-height: 260px;
    margin: 0;
    padding: var(--space-xs);
    overflow-y: auto;
    list-style: none;
    border: 1px solid var(--border-default);
    border-radius: var(--radius-md);
    background: var(--bg-subtle);
  }

  .preview li {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: var(--space-sm);
    padding: var(--space-2xs) var(--space-sm);
    font-size: var(--font-size-body);
  }

  .old {
    color: var(--text-secondary);
  }

  .same .new {
    color: var(--text-tertiary);
  }

  .arrow {
    color: var(--text-tertiary);
  }

  .bad .new,
  .why {
    color: var(--status-danger);
  }

  .why {
    flex-basis: 100%;
    font-size: var(--font-size-caption);
  }

  .more {
    color: var(--text-tertiary);
  }

  .error {
    margin: 0;
    color: var(--status-danger);
    font-size: var(--font-size-caption);
  }

  footer {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-sm);
  }

  footer button {
    padding: var(--space-xs) var(--space-md);
    border-radius: var(--radius-md);
    font-weight: var(--font-weight-medium);
  }

  .cancel {
    background: var(--bg-control);
  }

  .apply {
    background: var(--accent-default);
    color: var(--text-on-accent);
  }

  .apply:disabled {
    background: var(--bg-control);
    color: var(--text-tertiary);
  }
</style>
