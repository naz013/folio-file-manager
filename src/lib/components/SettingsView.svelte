<script lang="ts">
  import { getVersion } from "@tauri-apps/api/app";
  import { onMount, tick } from "svelte";
  import {
    crashClear,
    crashSet,
    crashState,
    listDir,
    openPath,
    type CrashState,
  } from "$lib/api/fs";
  import { shortcutGroups } from "$lib/shortcuts";
  import { SPRING_DELAYS_MS, settings, updateSettings } from "$lib/settings.svelte";
  import { themes, type Theme } from "$lib/theme";
  import { checkForUpdate, updates } from "$lib/updates.svelte";

  let {
    theme,
    ontheme,
  }: {
    theme: Theme;
    ontheme: (theme: Theme) => void;
  } = $props();

  let dialog: HTMLElement;
  onMount(() => {
    dialog.focus();
    if (isCustom) custom = settings.startLocation;
  });

  let version = $state("");
  onMount(() => {
    getVersion().then(
      (v) => (version = v),
      () => {},
    );
  });

  /** What the manual check found, in words. */
  const updateStatus = $derived(
    updates.phase === "checking"
      ? "Checking…"
      : updates.phase === "current"
        ? "Folio is up to date."
        : updates.phase === "available"
          ? `Folio ${updates.version} is available.`
          : updates.phase === "failed"
            ? updates.error
            : "",
  );

  let crash = $state<CrashState | null>(null);
  onMount(() => {
    crashState().then(
      (s) => (crash = s),
      () => {},
    );
  });

  /** "12 of 13 sessions ended cleanly (92%)", or a note while there is nothing to count. */
  const crashFree = $derived.by(() => {
    if (!crash?.sessions) return "No sessions counted yet.";
    const clean = crash.sessions - crash.crashed;
    const pct = Math.floor((clean / crash.sessions) * 100);
    return `${clean} of ${crash.sessions} sessions ended cleanly (${pct}%).`;
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

  /** "main": the settings; "shortcuts": the list of keyboard shortcuts. */
  let screen = $state<"main" | "shortcuts">("main");

  async function show(next: "main" | "shortcuts") {
    screen = next;
    await tick();
    dialog.scrollTop = 0;
    dialog.focus();
  }

  /** Escape leaves the shortcuts list for the settings; the tab's own keys stay global. */
  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape" && screen === "shortcuts") {
      e.preventDefault();
      show("main");
    }
  }

  const label: Record<Theme, string> = { system: "System", light: "Light", dark: "Dark" };
</script>

<!-- Only answers Escape inside the shortcuts list. -->
<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<section
  bind:this={dialog}
  class="settings"
  aria-labelledby="settings-title"
  tabindex="-1"
  {onkeydown}
>
  <div class="page">
    <header>
      {#if screen === "shortcuts"}
        <span class="heading">
          <button class="back" onclick={() => show("main")}>← Settings</button>
          <h2 id="settings-title">Keyboard shortcuts</h2>
        </span>
      {:else}
        <h2 id="settings-title">Settings</h2>
      {/if}
    </header>

    {#if screen === "shortcuts"}
      {#each shortcutGroups as group (group.title)}
        <section>
          <h3>{group.title}</h3>
          {#each group.items as item (item.keys + item.does)}
            <div class="row shortcut">
              <span>{item.does}</span>
              <kbd>{item.keys}</kbd>
            </div>
          {/each}
        </section>
      {/each}
    {:else}
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
          <span id="frame-label">Use the system window buttons</span>
          <input
            type="checkbox"
            class="switch"
            role="switch"
            aria-labelledby="frame-label"
            checked={settings.nativeFrame}
            onchange={(e) => updateSettings({ nativeFrame: e.currentTarget.checked })}
          />
        </div>

        <div class="row">
          <span id="view-label">Default view</span>
          <div class="segments" role="group" aria-labelledby="view-label">
            {#each ["grid", "list", "columns"] as const as mode (mode)}
              <button
                class:active={settings.defaultView === mode}
                aria-pressed={settings.defaultView === mode}
                onclick={() => updateSettings({ defaultView: mode })}
              >
                {mode === "grid" ? "Grid" : mode === "list" ? "List" : "Columns"}
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

      <section>
        <h3>Updates</h3>

        <div class="row">
          <span id="update-label">Check for updates automatically</span>
          <input
            type="checkbox"
            class="switch"
            role="switch"
            aria-labelledby="update-label"
            checked={settings.updateCheck}
            onchange={(e) => updateSettings({ updateCheck: e.currentTarget.checked })}
          />
        </div>
        <p class="note">
          Once a day Folio asks its release page whether a new version exists. Nothing else is sent.
          An update is only installed when you say so, and never while files are being copied or
          moved.
        </p>

        <div class="row">
          <span>Folio {version}{updateStatus ? ` · ${updateStatus}` : ""}</span>
          <button
            onclick={() => checkForUpdate(true)}
            disabled={updates.phase === "checking" || updates.phase === "downloading"}
          >
            Check now
          </button>
        </div>
      </section>

      <section>
        <h3>Help</h3>

        <div class="row">
          <span>Keyboard shortcuts</span>
          <button onclick={() => show("shortcuts")}>View</button>
        </div>

        <div class="row">
          <span>Welcome tour</span>
          <button onclick={() => updateSettings({ onboarded: false })}> Show again </button>
        </div>
      </section>

      <section>
        <h3>Privacy</h3>

        <div class="row">
          <span id="crash-label">Keep crash reports on this computer</span>
          <input
            type="checkbox"
            class="switch"
            role="switch"
            aria-labelledby="crash-label"
            checked={crash?.consent === true}
            onchange={async (e) => {
              const box = e.currentTarget;
              try {
                crash = await crashSet(box.checked);
              } catch {
                box.checked = crash?.consent === true;
              }
            }}
          />
        </div>
        <p class="note">Nothing is sent anywhere. Reports stay in a folder you can open.</p>

        {#if crash?.consent}
          <div class="row">
            <span>{crashFree}</span>
            <span class="buttons">
              <button onclick={() => crash && openPath(crash.dir)} disabled={!crash?.reports}>
                Show {crash?.reports ?? 0}
                {crash?.reports === 1 ? "report" : "reports"}
              </button>
              <button
                onclick={async () => (crash = await crashClear().catch(() => crash))}
                disabled={!crash?.reports && !crash?.crashed}
              >
                Delete
              </button>
            </span>
          </div>
        {/if}
      </section>
    {/if}
  </div>
</section>

<style>
  .settings {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    background: var(--bg-window);
    outline: none;
  }

  .page {
    width: min(560px, 100%);
    margin: 0 auto;
    padding: var(--space-xl) var(--space-lg) var(--space-3xl);
    box-sizing: border-box;
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

  .heading {
    display: grid;
    gap: var(--space-2xs);
    justify-items: start;
  }

  .back {
    color: var(--accent-default);
    font-size: var(--font-size-caption);
  }

  .shortcut kbd {
    flex-shrink: 0;
    padding: var(--space-2xs) var(--space-sm);
    border: 1px solid var(--border-default);
    border-radius: var(--radius-sm);
    background: var(--bg-control);
    color: var(--text-secondary);
    font: inherit;
    font-size: var(--font-size-caption);
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

  .note {
    margin: var(--space-xs) 0 0;
    color: var(--text-tertiary);
    font-size: var(--font-size-caption);
  }

  .buttons {
    display: flex;
    gap: var(--space-xs);
  }

  .buttons button {
    padding: var(--space-2xs) var(--space-sm);
    border-radius: var(--radius-sm);
    background: var(--bg-control);
  }

  .buttons button:disabled {
    color: var(--text-tertiary);
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
