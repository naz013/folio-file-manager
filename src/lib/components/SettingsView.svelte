<script lang="ts">
  import { getVersion } from "@tauri-apps/api/app";
  import { onMount, tick } from "svelte";
  import {
    crashClear,
    crashSet,
    crashState,
    listDir,
    openPath,
    pluginsFolder,
    tagsExport,
    type CrashState,
  } from "$lib/api/fs";
  import { SCALE_STOPS, nearestStop } from "$lib/gridScale";
  import type { Motion } from "$lib/motion";
  import { PERMISSIONS } from "$lib/plugins";
  import {
    disablePlugin,
    enablePlugin,
    plugins,
    reloaded,
    reloadPlugins,
  } from "$lib/plugins.svelte";
  import { SPRING_DELAYS_MS, settings, updateSettings } from "$lib/settings.svelte";
  import { themes, type Theme } from "$lib/theme";
  import { checkForUpdate, updateStatusText, updates } from "$lib/updates.svelte";

  /** Fast scan reads the NTFS file table, so only Windows offers it. */
  const onWindows = navigator.userAgent.includes("Windows");

  /** The plugin whose permissions are being read before it is turned on. */
  let reviewing = $state<string | null>(null);
  let pluginsNote = $state("");

  // What a plugin asks may have changed on a reload: the person reads it again.
  $effect(() => {
    void reloaded.n;
    reviewing = null;
  });

  async function openPluginsFolder() {
    try {
      await openPath(await pluginsFolder());
    } catch (e) {
      pluginsNote = String(e);
    }
  }

  let {
    theme,
    ontheme,
    onabout,
    onshortcuts,
    onhelp,
  }: {
    theme: Theme;
    ontheme: (theme: Theme) => void;
    /** Open the About dialog. */
    onabout: () => void;
    /** Open the list of keyboard shortcuts. */
    onshortcuts: () => void;
    /** Open the help topics. */
    onhelp: () => void;
  } = $props();

  let dialog: HTMLElement;
  onMount(() => {
    dialog.focus();
    if (isCustom) custom = settings.startLocation;
  });

  /** What the last tags export did, in words. */
  let tagsNote = $state(
    "Saves a Folio tags.json file in Documents. It lists the paths of your tagged files, so keep it private. On the other computer, open it in Folio and choose Import tags from its right-click menu.",
  );

  let exporting = $state(false);

  async function exportTags() {
    if (exporting) return;
    exporting = true;
    try {
      const done = await tagsExport();
      const left = done.leftOut ? ` ${done.leftOut} could not be found and were left out.` : "";
      tagsNote = `Saved ${done.path}: ${done.tags} tags, ${done.files} tagged files.${left}`;
    } catch (e) {
      tagsNote = String(e);
    } finally {
      exporting = false;
    }
  }

  let version = $state("");
  onMount(() => {
    getVersion().then(
      (v) => (version = v),
      () => {},
    );
  });

  /** What the manual check found, in words. */
  const updateStatus = $derived(updateStatusText());

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

  const label: Record<Theme, string> = { system: "System", light: "Light", dark: "Dark" };
</script>

<section bind:this={dialog} class="settings" aria-labelledby="settings-title" tabindex="-1">
  <div class="page">
    <header>
      <h2 id="settings-title">Settings</h2>
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
        <span id="tiles-label">Size of the tiles in the grid</span>
        <input
          type="range"
          class="size"
          min="0"
          max={SCALE_STOPS.length - 1}
          step="1"
          aria-labelledby="tiles-label"
          value={nearestStop(settings.gridScale)}
          oninput={(e) => updateSettings({ gridScale: SCALE_STOPS[Number(e.currentTarget.value)] })}
        />
      </div>

      <div class="row">
        <span id="view-label">Default view</span>
        <div class="segments" role="group" aria-labelledby="view-label">
          {#each ["grid", "list", "columns", "gallery"] as const as mode (mode)}
            <button
              class:active={settings.defaultView === mode}
              aria-pressed={settings.defaultView === mode}
              onclick={() => updateSettings({ defaultView: mode })}
            >
              {mode === "grid"
                ? "Grid"
                : mode === "list"
                  ? "List"
                  : mode === "columns"
                    ? "Columns"
                    : "Gallery"}
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

    {#if onWindows}
      <section>
        <h3>Disk scans</h3>

        <div class="row">
          <span id="fast-label">Fast scan (administrator)</span>
          <input
            type="checkbox"
            class="switch"
            role="switch"
            aria-labelledby="fast-label"
            checked={settings.fastScan}
            onchange={(e) => updateSettings({ fastScan: e.currentTarget.checked })}
          />
        </div>
        <p class="note">
          Reads an NTFS drive's file table, so a whole drive is scanned in seconds. Windows asks for
          administrator permission each time a scan starts, and only the scan runs with it: Folio
          itself does not. If you say no, or the drive is not NTFS, the folders are walked as usual.
        </p>
      </section>
    {/if}

    <section>
      <h3>Plugins</h3>
      <p class="note">
        Plugins add actions to the right-click menu and previews to Quick Look. Each runs in a
        sandbox: it can't read or change your files, or use the network, except for what you allow
        below. A plugin is a folder with a plugin.json inside the plugins folder.
      </p>
      <div class="row">
        <span>Plugins folder</span>
        <div class="buttons">
          <button onclick={openPluginsFolder}>Open folder</button>
          <button onclick={reloadPlugins}>Reload</button>
        </div>
      </div>
      {#if pluginsNote}<p class="error" role="alert">{pluginsNote}</p>{/if}
      {#each plugins.list as p (p.id)}
        <div class="plugin">
          <div class="row">
            <div>
              <strong>{p.manifest?.name ?? p.id}</strong>
              {#if p.manifest}<span class="note">v{p.manifest.version}</span>{/if}
              {#if p.manifest?.description}<p class="note">{p.manifest.description}</p>{/if}
            </div>
            {#if p.manifest}
              <input
                type="checkbox"
                class="switch"
                role="switch"
                aria-label="Turn on {p.manifest.name}"
                checked={p.enabled}
                onchange={(e) => {
                  if (!e.currentTarget.checked) {
                    disablePlugin(p.id);
                    reviewing = null;
                  } else {
                    // Turned on only once the person has read what it can do.
                    e.currentTarget.checked = false;
                    reviewing = p.id;
                  }
                }}
              />
            {/if}
          </div>
          {#if p.error}<p class="error" role="alert">{p.error}</p>{/if}
          {#if p.changed}
            <p class="note" role="status">
              Its version, permissions or files have changed since you turned it on, so it is off.
              Turn it on again to review it.
            </p>
          {/if}
          {#if p.manifest && (p.enabled || reviewing === p.id)}
            <ul class="can">
              {#each p.manifest.permissions as perm (perm)}<li>{PERMISSIONS[perm]}</li>{/each}
              {#if !p.manifest.permissions.length}<li>Nothing beyond drawing its own page</li>{/if}
            </ul>
            <p class="note">
              {#each p.manifest.actions as a (a.id)}Action: {a.title}.
              {/each}
              {#each p.manifest.previews as v (v.id)}Preview: {v.title} ({v.extensions.join(", ")}).
              {/each}
            </p>
          {/if}
          {#if p.manifest && reviewing === p.id}
            <div class="buttons">
              <button
                onclick={() => {
                  if (p.manifest) enablePlugin(p.manifest, p.digest);
                  reviewing = null;
                }}>Allow and turn on</button
              >
              <button onclick={() => (reviewing = null)}>Cancel</button>
            </div>
          {/if}
        </div>
      {/each}
      {#if plugins.loaded && !plugins.list.length}
        <p class="note">No plugins yet. Put a plugin folder in the plugins folder, then Reload.</p>
      {/if}
    </section>

    <section>
      <h3>Motion</h3>

      <div class="row">
        <label for="motion">Animations</label>
        <select
          id="motion"
          value={settings.motion}
          onchange={(e) => updateSettings({ motion: e.currentTarget.value as Motion })}
        >
          <option value="system">Follow the system</option>
          <option value="full">Always on</option>
          <option value="reduced">Off</option>
        </select>
      </div>
      <p class="note">
        Menus, dialogs and tabs ease in. Off shows every change at once; the ring that counts down
        before a dragged-over folder opens still runs.
      </p>
    </section>

    <section>
      <h3>Hints</h3>

      <div class="row">
        <span id="hints-label">Show hints in empty areas</span>
        <input
          type="checkbox"
          class="switch"
          role="switch"
          aria-labelledby="hints-label"
          checked={settings.showHints}
          onchange={(e) => updateSettings({ showHints: e.currentTarget.checked })}
        />
      </div>
      <p class="note">
        An empty folder, tag list or clipboard shelf says what to do there. Turn this off once you
        know. Messages about a search, the Recycle Bin or a cleanup that found nothing always show.
      </p>
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
      <h3>Tags</h3>

      <div class="row">
        <span>Back up tags, or take them to another computer</span>
        <button disabled={exporting} onclick={exportTags}>Export</button>
      </div>
      <p class="note" role="status">{tagsNote}</p>
    </section>

    <section>
      <h3>Help</h3>

      <div class="row">
        <span>Help</span>
        <button onclick={onhelp}>Open</button>
      </div>

      <div class="row">
        <span>Keyboard shortcuts</span>
        <button onclick={onshortcuts}>View</button>
      </div>

      <div class="row">
        <span>Welcome tour</span>
        <button onclick={() => updateSettings({ onboarded: false })}> Show again </button>
      </div>

      <div class="row">
        <span>Version, license and links</span>
        <button onclick={onabout}>About Folio</button>
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

  .size {
    width: 160px;
    accent-color: var(--accent-default);
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

  .plugin {
    padding-bottom: var(--space-sm);
    border-bottom: 1px solid var(--border-default);
  }

  .plugin .row {
    border-bottom: 0;
    padding-bottom: 0;
  }

  .can {
    margin: var(--space-xs) 0 0;
    padding-left: var(--space-lg);
    color: var(--text-secondary);
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
    transition: background var(--duration-fast) var(--ease-standard);
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
    transition: translate var(--duration-fast) var(--ease-standard);
  }

  .switch:checked {
    background: var(--accent-default);
  }

  .switch:checked::after {
    translate: 16px 0;
  }
</style>
