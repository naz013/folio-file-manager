<script lang="ts">
  import { tick } from "svelte";
  import Icon from "./Icon.svelte";
  import { joinPath, listDir, pathSegments, separatorOf } from "$lib/api/fs";
  import type { Theme } from "$lib/theme";

  export type View = "grid" | "list";

  let {
    path,
    view = $bindable(),
    theme,
    dropTarget,
    springPath,
    springMs,
    canBack,
    canForward,
    onback,
    onforward,
    onnavigate,
    ontheme,
    onsettings,
  }: {
    path: string;
    view: View;
    theme: Theme;
    /** Folder under a native drag, and the one whose spring-loading countdown is running. */
    dropTarget: string | null;
    springPath: string | null;
    springMs: number;
    canBack: boolean;
    canForward: boolean;
    onback: () => void;
    onforward: () => void;
    /** Resolves to whether the folder was opened (null if superseded); `quiet` leaves the error to the caller. */
    onnavigate: (path: string, quiet?: boolean) => Promise<boolean | null>;
    ontheme: () => void;
    onsettings: () => void;
  } = $props();

  let segments = $derived(pathSegments(path));

  // ---- Sibling menus ----

  const MENU_LIMIT = 300;
  /** The subfolders of `dir`, listed under the chevron that follows it in the path. */
  let menu = $state<{ dir: string; x: number; y: number; names: string[]; more: number } | null>(
    null,
  );
  let menuEl = $state<HTMLElement>();

  async function subfolders(dir: string): Promise<{ name: string; hidden: boolean }[]> {
    const entries = await listDir(dir);
    return entries
      .filter((e) => e.isDir)
      .map((e) => ({ name: e.name, hidden: e.hidden }))
      .sort((a, b) =>
        a.name.localeCompare(b.name, undefined, { numeric: true, sensitivity: "base" }),
      );
  }

  let menuGen = 0;

  // The menu belongs to the path it was opened on.
  $effect(() => {
    void path;
    menu = null;
  });

  async function openMenu(dir: string, anchor: HTMLElement) {
    if (menu?.dir === dir) {
      menu = null;
      return;
    }
    const gen = ++menuGen;
    const box = anchor.getBoundingClientRect();
    let names: string[] = [];
    try {
      names = (await subfolders(dir)).filter((f) => !f.hidden).map((f) => f.name);
    } catch {
      // Unreadable folder: the menu just shows as empty.
    }
    // Another menu, or the path editor, took over while this one was listing.
    if (gen !== menuGen) return;
    menu = {
      dir,
      x: box.left,
      y: box.bottom + 4,
      names: names.slice(0, MENU_LIMIT),
      more: Math.max(0, names.length - MENU_LIMIT),
    };
    await tick();
    // Land on the folder you are in, else the first; an empty menu takes focus itself so
    // Escape still reaches it.
    const first =
      menuEl?.querySelector<HTMLElement>("[aria-current=true]") ??
      menuEl?.querySelector<HTMLElement>("button") ??
      menuEl;
    first?.focus();
    first?.scrollIntoView?.({ block: "nearest" });
  }

  function pick(name: string) {
    if (!menu) return;
    const target = joinPath(menu.dir, name);
    menu = null;
    onnavigate(target);
  }

  function menuKeys(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      menu = null;
      return;
    }
    const step = e.key === "ArrowDown" ? 1 : e.key === "ArrowUp" ? -1 : 0;
    if (!step || !menuEl) return;
    e.preventDefault();
    const items = [...menuEl.querySelectorAll<HTMLElement>("button")];
    const at = items.indexOf(document.activeElement as HTMLElement);
    items[(at + step + items.length) % items.length]?.focus();
  }

  // ---- Editable path ----

  let editing = $state(false);
  let draft = $state("");
  let error = $state("");
  let input = $state<HTMLInputElement>();
  let suggestions = $state<string[]>([]);
  let chosen = $state(-1);
  let suggestGen = 0;
  /** The folders of the directory being completed in, listed once while it stays the same. */
  let listed: { dir: string; folders: { name: string; hidden: boolean }[] } | null = null;

  /** Starts typing a path (Ctrl+L, or double-click on the path). */
  export async function edit() {
    menuGen++;
    menu = null;
    listed = null;
    draft = path;
    error = "";
    suggestions = [];
    editing = true;
    await tick();
    input?.focus();
    input?.select();
  }

  /** Folders that continue what has been typed: "C:\Us" gives "C:\Users". */
  async function suggest() {
    const gen = ++suggestGen;
    const cut = draft.lastIndexOf(separatorOf(draft));
    if (cut < 0) {
      suggestions = [];
      return;
    }
    const dir = draft.slice(0, cut + 1);
    const prefix = draft.slice(cut + 1).toLowerCase();
    if (listed?.dir !== dir) {
      let folders: { name: string; hidden: boolean }[] = [];
      try {
        folders = await subfolders(dir);
      } catch {
        // Not a folder (yet): nothing to suggest.
      }
      if (gen !== suggestGen) return;
      listed = { dir, folders };
    }
    // Hidden folders only once a leading dot says they are wanted.
    suggestions = listed.folders
      .filter(
        (f) => (prefix.startsWith(".") || !f.hidden) && f.name.toLowerCase().startsWith(prefix),
      )
      .slice(0, 8)
      .map((f) => dir + f.name);
    chosen = -1;
  }

  function stopEditing() {
    editing = false;
    error = "";
    suggestions = [];
    suggestGen++;
  }

  async function commit() {
    const target = (chosen >= 0 ? suggestions[chosen] : draft).trim();
    if (!target) return;
    const opened = await onnavigate(target, true);
    // null: a newer navigation took over, so there is nothing to report.
    if (opened) stopEditing();
    else if (opened === false) error = `"${target}" isn't a folder that can be opened.`;
  }

  function inputKeys(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      stopEditing();
    } else if (e.key === "Enter") {
      e.preventDefault();
      commit();
    } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      if (!suggestions.length) return;
      e.preventDefault();
      const n = suggestions.length;
      chosen = e.key === "ArrowDown" ? (chosen + 1) % n : (chosen - 1 + n) % n;
    } else if (e.key === "Tab" && !e.shiftKey && suggestions.length) {
      e.preventDefault();
      draft = suggestions[Math.max(chosen, 0)] + separatorOf(draft);
      error = "";
      chosen = -1;
      suggestions = [];
      suggest();
    }
  }

  const themeLabel = { system: "Theme: follow system", light: "Theme: light", dark: "Theme: dark" };
</script>

<svelte:window onpointerdown={() => (menu = null)} />

<header class="toolbar">
  <button class="icon-btn" disabled={!canBack} onclick={onback} aria-label="Back">
    <Icon name="chevronLeft" />
  </button>
  <button class="icon-btn" disabled={!canForward} onclick={onforward} aria-label="Forward">
    <Icon name="chevronRight" />
  </button>

  {#if editing}
    <div class="editor">
      <input
        bind:this={input}
        bind:value={draft}
        class:invalid={!!error}
        spellcheck="false"
        autocomplete="off"
        aria-label="Path"
        aria-invalid={!!error}
        oninput={() => {
          error = "";
          // What was highlighted belongs to the old text.
          chosen = -1;
          suggestions = [];
          suggest();
        }}
        onkeydown={inputKeys}
        onblur={stopEditing}
      />
      {#if error}
        <p class="error" role="alert">{error}</p>
      {:else if suggestions.length}
        <ul class="suggest" role="listbox">
          {#each suggestions as s, i (s)}
            <li role="option" aria-selected={i === chosen} class:chosen={i === chosen}>
              <!-- mousedown, not click: the input's blur would close the list first -->
              <button
                tabindex="-1"
                onmousedown={(e) => {
                  e.preventDefault();
                  chosen = i;
                  commit();
                }}
              >
                {s}
              </button>
            </li>
          {/each}
        </ul>
      {/if}
    </div>
  {:else}
    <nav
      class="crumbs"
      ondblclick={(e) => {
        // Only the empty space: a crumb or chevron has already acted on its clicks.
        if (!(e.target as HTMLElement).closest("button")) edit();
      }}
      aria-label="Path"
    >
      {#each segments as seg, i (seg.path)}
        <button
          class="crumb"
          class:last={i === segments.length - 1}
          class:drop={dropTarget === seg.path}
          class:springing={springPath === seg.path}
          style:--spring-ms="{springMs}ms"
          data-drop-path={seg.path}
          onclick={() => onnavigate(seg.path)}
        >
          {seg.name}
        </button>
        <button
          class="sep"
          class:open={menu?.dir === seg.path}
          aria-label="Folders in {seg.name}"
          aria-haspopup="menu"
          aria-expanded={menu?.dir === seg.path}
          onpointerdown={(e) => e.stopPropagation()}
          onclick={(e) => openMenu(seg.path, e.currentTarget)}
        >
          <Icon name="chevronRight" size={12} stroke={2} />
        </button>
      {/each}
    </nav>
  {/if}

  <div class="switcher" role="group" aria-label="View">
    <button class:active={view === "grid"} onclick={() => (view = "grid")} aria-label="Grid view">
      <Icon name="grid" size={15} />
    </button>
    <button class:active={view === "list"} onclick={() => (view = "list")} aria-label="List view">
      <Icon name="list" size={15} />
    </button>
  </div>

  <button
    class="icon-btn"
    onclick={ontheme}
    aria-label={themeLabel[theme]}
    title={themeLabel[theme]}
  >
    <Icon name={theme === "system" ? "monitor" : theme === "light" ? "sun" : "moon"} size={16} />
  </button>

  <button
    class="icon-btn"
    onclick={onsettings}
    aria-label="Settings (Ctrl+,)"
    title="Settings (Ctrl+,)"
  >
    <Icon name="settings" size={16} />
  </button>
</header>

{#if menu}
  <div
    class="menu"
    role="menu"
    tabindex="-1"
    bind:this={menuEl}
    style:left="{menu.x}px"
    style:top="{menu.y}px"
    onpointerdown={(e) => e.stopPropagation()}
    onkeydown={menuKeys}
  >
    {#each menu.names as name (name)}
      <button
        role="menuitem"
        aria-current={segments.some((s) => s.path === joinPath(menu!.dir, name))}
        onclick={() => pick(name)}
      >
        {name}
      </button>
    {:else}
      <span class="none">No folders</span>
    {/each}
    {#if menu.more}<span class="none">and {menu.more} more</span>{/if}
  </div>
{/if}

<style>
  .toolbar {
    display: flex;
    align-items: center;
    gap: 6px;
    height: 52px;
    padding: 0 12px;
    border-bottom: 1px solid var(--border-default);
  }

  .icon-btn {
    display: grid;
    place-items: center;
    width: 30px;
    height: 30px;
    border-radius: var(--radius-md);
    color: var(--text-secondary);
  }

  .icon-btn:hover:not(:disabled) {
    background: var(--bg-hover);
  }

  .icon-btn:disabled {
    color: var(--text-tertiary);
  }

  .crumbs {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 4px;
    min-width: 0;
    padding-left: 6px;
    overflow: hidden;
  }

  .sep {
    display: flex;
    align-items: center;
    padding: 4px 2px;
    border-radius: var(--radius-sm);
    color: var(--text-tertiary);
  }

  .sep:hover,
  .sep.open {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .editor {
    position: relative;
    flex: 1;
    min-width: 0;
  }

  .editor input {
    width: 100%;
    padding: 5px var(--space-sm);
    border: 1px solid var(--accent-default);
    border-radius: var(--radius-md);
    background: var(--bg-window);
    color: var(--text-primary);
    font: inherit;
    outline: none;
  }

  .editor input.invalid {
    border-color: var(--status-danger);
  }

  .error {
    position: absolute;
    top: 100%;
    left: 0;
    margin: 4px 0 0;
    color: var(--status-danger);
    font-size: var(--font-size-caption);
  }

  .suggest,
  .menu {
    z-index: 40;
    border: 1px solid var(--border-default);
    border-radius: var(--radius-lg);
    background: var(--bg-window);
    box-shadow: var(--shadow-popover);
  }

  .suggest {
    position: absolute;
    top: 100%;
    left: 0;
    right: 0;
    margin: 4px 0 0;
    padding: var(--space-2xs);
    list-style: none;
  }

  .menu {
    position: fixed;
    min-width: 200px;
    max-width: 360px;
    max-height: 320px;
    padding: var(--space-2xs);
    overflow-y: auto;
  }

  .suggest button,
  .menu button {
    display: block;
    width: 100%;
    padding: var(--space-xs) var(--space-sm);
    border-radius: var(--radius-sm);
    color: var(--text-primary);
    text-align: left;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .suggest .chosen button,
  .menu button:hover,
  .menu button:focus-visible {
    background: var(--bg-hover);
  }

  .menu button[aria-current="true"] {
    font-weight: var(--font-weight-semibold);
  }

  .none {
    display: block;
    padding: var(--space-xs) var(--space-sm);
    color: var(--text-tertiary);
    font-size: var(--font-size-caption);
  }

  .crumb {
    padding: 4px 6px;
    border-radius: var(--radius-sm);
    color: var(--text-secondary);
    white-space: nowrap;
  }

  .crumb:hover {
    background: var(--bg-hover);
  }

  .crumb.drop {
    background: var(--accent-subtle);
  }

  .crumb.last {
    color: var(--text-primary);
    font-weight: var(--font-weight-semibold);
  }

  .switcher {
    display: flex;
    gap: 2px;
    padding: 2px;
    border-radius: 9px;
    background: var(--bg-control);
  }

  .switcher button {
    display: grid;
    place-items: center;
    width: 32px;
    height: 26px;
    border-radius: 7px;
    color: var(--text-secondary);
  }

  .switcher button.active {
    background: var(--bg-window);
    color: var(--text-primary);
    box-shadow: var(--shadow-raised);
  }
</style>
