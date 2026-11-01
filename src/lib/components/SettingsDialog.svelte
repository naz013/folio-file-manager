<script lang="ts">
  import { onMount, tick } from "svelte";
  import { listDir } from "$lib/api/fs";
  import { SPRING_DELAYS_MS, settings, updateSettings } from "$lib/settings.svelte";
  import { themes, type Theme } from "$lib/theme";

  let {
    theme,
    ontheme,
    onclose,
  }: {
    theme: Theme;
    ontheme: (theme: Theme) => void;
    onclose: () => void;
  } = $props();

  let dialog: HTMLElement;
  /** Where focus was, so closing the dialog gives it back. */
  const opener = document.activeElement as HTMLElement | null;
  onMount(() => {
    dialog.focus();
    if (isCustom) custom = settings.startLocation;
    return () => opener?.focus?.();
  });

  const places = [
    ["home", "Home"],
    ["desktop", "Desktop"],
    ["documents", "Documents"],
    ["downloads", "Downloads"],
  ] as const;

  /** "Other folder…" was picked, before a path has been saved. */
  let pickingCustom = $state(false);
  const isCustom = $derived(!places.some(([value]) => value === settings.startLocation));
  const showCustom = $derived(isCustom || pickingCustom);
  // What is typed in the custom field; saved when it loses focus or on Enter.
  let custom = $state("");
  let customError = $state("");
  let customInput = $state<HTMLInputElement>();

  async function pickCustom() {
    pickingCustom = true;
    await tick();
    customInput?.focus();
  }

  /** Saves the typed folder if it can be opened; otherwise says so and keeps the old one. */
  async function commitCustom() {
    const path = custom.trim();
    if (!path) return;
    try {
      await listDir(path);
    } catch {
      customError = "This folder can't be opened.";
      return;
    }
    customError = "";
    updateSettings({ startLocation: path });
  }

  function onkeydown(e: KeyboardEvent) {
    // Nothing behind the dialog may act on keys while it is open.
    e.stopPropagation();
    if (e.key === "Tab") {
      // Keep focus inside the dialog.
      const items = [
        ...dialog.querySelectorAll<HTMLElement>(
          "button, select, input, [tabindex]:not([tabindex='-1'])",
        ),
      ].filter((el) => !el.hasAttribute("disabled"));
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
    if (e.key === "Escape") {
      e.preventDefault();
      onclose();
    }
  }

  const label: Record<Theme, string> = { system: "System", light: "Light", dark: "Dark" };
</script>

<div class="backdrop" role="presentation" onpointerdown={onclose}>
  <div
    bind:this={dialog}
    class="dialog"
    role="dialog"
    aria-modal="true"
    aria-labelledby="settings-title"
    tabindex="-1"
    onpointerdown={(e) => e.stopPropagation()}
    {onkeydown}
  >
    <header>
      <h2 id="settings-title">Settings</h2>
      <button class="done" onclick={onclose}>Done</button>
    </header>

    <section>
      <h3>General</h3>

      <div class="row">
        <label for="start">Start location</label>
        <div class="field">
          <select
            id="start"
            value={showCustom ? "custom" : settings.startLocation}
            onchange={(e) => {
              const value = e.currentTarget.value;
              if (value === "custom") pickCustom();
              else {
                pickingCustom = false;
                customError = "";
                updateSettings({ startLocation: value });
              }
            }}
          >
            {#each places as [value, name] (value)}
              <option {value}>{name}</option>
            {/each}
            <option value="custom">Other folder…</option>
          </select>
          {#if showCustom}
            <input
              bind:this={customInput}
              aria-label="Start folder path"
              aria-invalid={!!customError}
              placeholder="Path of the folder"
              spellcheck="false"
              bind:value={custom}
              onblur={commitCustom}
              onkeydown={(e) => e.key === "Enter" && commitCustom()}
            />
            {#if customError}<span class="error" role="alert">{customError}</span>{/if}
          {/if}
        </div>
      </div>

      <div class="row">
        <span id="hidden-label">Show hidden files</span>
        <input
          type="checkbox"
          class="switch"
          role="switch"
          aria-labelledby="hidden-label"
          checked={settings.showHidden}
          onchange={(e) => updateSettings({ showHidden: e.currentTarget.checked })}
        />
      </div>

      <div class="row">
        <span id="view-label">Default view</span>
        <div class="segments" role="group" aria-labelledby="view-label">
          {#each ["grid", "list"] as const as mode (mode)}
            <button
              class:active={settings.defaultView === mode}
              aria-pressed={settings.defaultView === mode}
              onclick={() => updateSettings({ defaultView: mode })}
            >
              {mode === "grid" ? "Grid" : "List"}
            </button>
          {/each}
        </div>
      </div>

      <div class="row">
        <span id="theme-label">Theme</span>
        <div class="segments" role="group" aria-labelledby="theme-label">
          {#each themes as t (t)}
            <button
              class:active={theme === t}
              aria-pressed={theme === t}
              onclick={() => ontheme(t)}
            >
              {label[t]}
            </button>
          {/each}
        </div>
      </div>
    </section>

    <section>
      <h3>Dragging</h3>

      <div class="row">
        <label for="spring">Open folders and tabs while dragging after</label>
        <select
          id="spring"
          value={settings.springDelayMs === null ? "off" : String(settings.springDelayMs)}
          onchange={(e) => {
            const v = e.currentTarget.value;
            updateSettings({ springDelayMs: v === "off" ? null : Number(v) });
          }}
        >
          <option value="off">Never</option>
          {#each SPRING_DELAYS_MS as ms (ms)}
            <option value={String(ms)}>{ms / 1000} s</option>
          {/each}
        </select>
      </div>

      <div class="row">
        <span id="return-label">Return to the start folder after dropping</span>
        <input
          type="checkbox"
          class="switch"
          role="switch"
          aria-labelledby="return-label"
          checked={settings.returnAfterDrop}
          onchange={(e) => updateSettings({ returnAfterDrop: e.currentTarget.checked })}
        />
      </div>
    </section>
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
    width: min(520px, calc(100vw - 2 * var(--space-xl)));
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
    justify-content: space-between;
    align-items: center;
  }

  h2 {
    margin: 0;
    font-size: var(--font-size-title);
    font-weight: var(--font-weight-semibold);
  }

  h3 {
    margin: var(--space-lg) 0 var(--space-xs);
    color: var(--text-tertiary);
    font-size: var(--font-size-caption);
    font-weight: var(--font-weight-semibold);
    letter-spacing: 0.4px;
    text-transform: uppercase;
  }

  .done {
    padding: var(--space-xs) var(--space-md);
    border-radius: var(--radius-md);
    background: var(--accent-default);
    color: var(--text-on-accent);
    font-weight: var(--font-weight-medium);
  }

  .row {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: var(--space-lg);
    padding: var(--space-sm) 0;
    border-bottom: 1px solid var(--border-default);
  }

  .error {
    color: var(--status-danger);
    font-size: var(--font-size-caption);
  }

  .field {
    display: grid;
    gap: var(--space-xs);
    justify-items: end;
  }

  select,
  input:not(.switch) {
    padding: var(--space-xs) var(--space-sm);
    border: 1px solid var(--border-default);
    border-radius: var(--radius-md);
    background: var(--bg-control);
    color: var(--text-primary);
    font: inherit;
  }

  input:not(.switch) {
    width: 260px;
  }

  .segments {
    display: flex;
    gap: var(--space-2xs);
    padding: var(--space-2xs);
    border-radius: var(--radius-md);
    background: var(--bg-control);
  }

  .segments button {
    padding: var(--space-2xs) var(--space-md);
    border-radius: var(--radius-sm);
    color: var(--text-secondary);
  }

  .segments button.active {
    background: var(--bg-window);
    color: var(--text-primary);
    box-shadow: var(--shadow-raised);
  }

  .switch {
    appearance: none;
    position: relative;
    width: 36px;
    height: 20px;
    margin: 0;
    border-radius: 10px;
    background: var(--border-strong);
    transition: background 120ms ease-out;
  }

  .switch::after {
    content: "";
    position: absolute;
    top: 2px;
    left: 2px;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: var(--bg-window);
    transition: translate 120ms ease-out;
  }

  .switch:checked {
    background: var(--accent-default);
  }

  .switch:checked::after {
    translate: 16px 0;
  }
</style>
