<script lang="ts">
  import FolderIcon from "./FolderIcon.svelte";
  import Icon, { type IconName } from "./Icon.svelte";
  import { joinPath, listDrives, type Drive, type SmartFolder, type Tag } from "$lib/api/fs";
  import { formatSize } from "$lib/format";
  import { hintFor } from "$lib/shortcuts";
  import { tagColor } from "$lib/tags";
  import EmptyState from "./EmptyState.svelte";
  import { settings } from "$lib/settings.svelte";

  /** A press on a pinned folder that moves far enough pulls it out as a drag to reorder. */
  let press: { x: number; y: number; path: string } | null = null;

  function pressPin(e: PointerEvent, path: string) {
    if (e.button === 0) press = { x: e.clientX, y: e.clientY, path };
  }

  function movePress(e: PointerEvent) {
    if (!press) return;
    if (!(e.buttons & 1)) {
      press = null;
      return;
    }
    if (Math.hypot(e.clientX - press.x, e.clientY - press.y) < 5) return;
    const { path } = press;
    press = null;
    onpindrag(path);
  }

  const baseName = (p: string) => p.split(/[\\/]/).filter(Boolean).pop() ?? p;
  const trimEnd = (p: string) => p.replace(/[\\/]+$/, "").toLowerCase();
  /** A drive this full or more shows its bar as a warning. */
  const NEARLY_FULL = 0.9;

  let {
    home,
    current,
    onnavigate,
    onopentab,
    dropTarget,
    springPath,
    springMs,
    collapsed,
    ontogglesection,
    tags,
    activeTag,
    ontag,
    smartFolders,
    activeSmart,
    onsmart,
    onsmartnew,
    onsmartmenu,
    onbin,
    binShown = false,
    folderTint,
    pinSlot,
    onpinmenu,
    onpindrag,
  }: {
    home: string;
    current: string;
    onnavigate: (path: string) => void;
    /** Middle-click: open the folder in a new background tab. */
    onopentab: (path: string) => void;
    /** Folder path currently under a native drag. */
    dropTarget: string | null;
    springPath: string | null;
    springMs: number;
    /** The sections folded away in this tab: "favorites", "drives" or "tags". */
    collapsed: string[];
    /** Fold a section away, or open it again. */
    ontogglesection: (section: string) => void;
    tags: Tag[];
    /** The tag whose items the list shows, if any. */
    activeTag: number | null;
    /** Show every item with this tag. */
    ontag: (tag: Tag) => void;
    smartFolders: SmartFolder[];
    /** The smart folder the list shows, if any. */
    activeSmart: number | null;
    onsmart: (folder: SmartFolder) => void;
    /** Start a new smart folder. */
    onsmartnew: () => void;
    /** Right-click on a smart folder. */
    onsmartmenu: (folder: SmartFolder, e: MouseEvent) => void;
    /** Open the Recycle Bin. */
    onbin: () => void;
    /** The tab shows the Recycle Bin. */
    binShown?: boolean;
    /** The CSS colour a folder takes from its first colour tag, if it has one. */
    folderTint: (path: string) => string | null;
    /** A folder drag is over the Favorites section, where dropping pins it. */
    pinSlot: number | null;
    /** A pinned folder was pulled out: start dragging it to a new place in the list. */
    onpindrag: (path: string) => void;
    /** Right-click on a pinned folder. */
    onpinmenu: (path: string, e: MouseEvent) => void;
  } = $props();

  const favorites: { label: string; icon: IconName; sub: string | null }[] = [
    { label: "Home", icon: "home", sub: null },
    { label: "Desktop", icon: "monitor", sub: "Desktop" },
    { label: "Documents", icon: "fileText", sub: "Documents" },
    { label: "Downloads", icon: "download", sub: "Downloads" },
    { label: "Pictures", icon: "image", sub: "Pictures" },
  ];

  const sections = {
    favorites: "Favorites",
    drives: "Drives",
    tags: "Tags",
    smart: "Smart folders",
  } as const;
  const folded = (section: keyof typeof sections) => collapsed.includes(section);

  let drives = $state.raw<Drive[]>([]);

  const refreshDrives = () => {
    listDrives()
      .then((d) => (drives = d))
      .catch(() => {});
  };

  // Free space changes while the app is open, so look again when the window is back in front.
  $effect(() => {
    refreshDrives();
    window.addEventListener("focus", refreshDrives);
    return () => window.removeEventListener("focus", refreshDrives);
  });
</script>

{#snippet heading(section: keyof typeof sections)}
  <button
    class="label"
    id="{section}-label"
    aria-expanded={!folded(section)}
    onclick={() => ontogglesection(section)}
  >
    <span class="chevron" class:folded={folded(section)}><Icon name="chevronDown" size={12} /></span
    >
    {sections[section]}
  </button>
{/snippet}

<svelte:window onpointermove={movePress} onpointerup={() => (press = null)} />

<aside class="sidebar" aria-label="Places and tags">
  <!-- The window has no native title bar; the brand row is part of the drag area. -->
  <div class="brand" data-tauri-drag-region>
    <span class="logo"><Icon name="folder" size={14} stroke={2.2} /></span>
    <span class="name">Folio</span>
  </div>

  <div class="places">
    <!-- Dropping a folder anywhere in here (not on a favorite) pins it. -->
    <div class="favorites" data-pin-drop>
      {@render heading("favorites")}
      {#if !folded("favorites")}
        {#each favorites as fav, at (fav.label)}
          {@const path = fav.sub ? joinPath(home, fav.sub) : home}
          <button
            class="item"
            class:selected={path === current}
            aria-current={path === current ? "page" : undefined}
            class:drop={dropTarget === path}
            class:springing={springPath === path}
            style:--spring-ms="{springMs}ms"
            data-drop-path={path}
            data-pin-fixed
            class:line-after={pinSlot === 0 &&
              settings.pinnedFolders.length === 0 &&
              at === favorites.length - 1}
            data-pin-last={at === favorites.length - 1 ? "" : undefined}
            onclick={() => onnavigate(path)}
            onmousedown={(e) => e.button === 1 && e.preventDefault()}
            onauxclick={(e) => e.button === 1 && onopentab(path)}
          >
            <span class="icon" style:color={folderTint(path)}><Icon name={fav.icon} /></span>
            <span>{fav.label}</span>
          </button>
        {/each}
        {#each settings.pinnedFolders as pinned, index (pinned)}
          <button
            class="item"
            class:selected={pinned === current}
            aria-current={pinned === current ? "page" : undefined}
            class:drop={dropTarget === pinned}
            class:springing={springPath === pinned}
            style:--spring-ms="{springMs}ms"
            data-drop-path={pinned}
            data-pin-index={index}
            class:line-before={pinSlot === index}
            class:line-after={pinSlot === settings.pinnedFolders.length &&
              index === settings.pinnedFolders.length - 1}
            title={pinned}
            onpointerdown={(e) => pressPin(e, pinned)}
            onclick={() => onnavigate(pinned)}
            onmousedown={(e) => e.button === 1 && e.preventDefault()}
            onauxclick={(e) => e.button === 1 && onopentab(pinned)}
            oncontextmenu={(e) => {
              e.preventDefault();
              onpinmenu(pinned, e);
            }}
          >
            <span class="icon"><FolderIcon size={16} tint={folderTint(pinned)} /></span>
            <span>{baseName(pinned)}</span>
          </button>
        {/each}
      {/if}
    </div>

    {#if drives.length}
      {@render heading("drives")}
      {#if !folded("drives")}
        <div role="group" aria-labelledby="drives-label">
          {#each drives as drive (drive.path)}
            {@const used = drive.total > 0 ? (drive.total - drive.free) / drive.total : 0}
            {@const letter = drive.path.slice(0, 2)}
            <button
              class="item drive"
              class:selected={trimEnd(drive.path) === trimEnd(current)}
              aria-current={trimEnd(drive.path) === trimEnd(current) ? "page" : undefined}
              class:drop={dropTarget === drive.path}
              class:springing={springPath === drive.path}
              style:--spring-ms="{springMs}ms"
              data-drop-path={drive.path}
              title="{letter} · {formatSize(drive.free)} free of {formatSize(drive.total)}"
              onclick={() => onnavigate(drive.path)}
              onmousedown={(e) => e.button === 1 && e.preventDefault()}
              onauxclick={(e) => e.button === 1 && onopentab(drive.path)}
            >
              <span class="icon"><Icon name="hardDrive" /></span>
              <span class="drive-info">
                <span class="drive-name">{letter}</span>
                <span class="usage" class:full={used >= NEARLY_FULL}>
                  <span class="fill" style:width="{Math.round(used * 100)}%"></span>
                </span>
                <span class="free">{formatSize(drive.free)} free</span>
              </span>
            </button>
          {/each}
        </div>
      {/if}
    {/if}

    {@render heading("tags")}
    {#if !folded("tags")}
      <div role="group" aria-labelledby="tags-label">
        {#each tags as tag (tag.id)}
          <button
            class="item tag"
            class:selected={activeTag === tag.id}
            aria-pressed={activeTag === tag.id}
            onclick={() => ontag(tag)}
          >
            <span class="dot" style:background={tagColor(tag.color)}></span>
            <span>{tag.name}</span>
          </button>
        {:else}
          <EmptyState
            icon="tag"
            text="No tags yet. Select files and press {hintFor('tag')}."
            hint
            compact
          />
        {/each}
      </div>
    {/if}

    {@render heading("smart")}
    {#if !folded("smart")}
      <div role="group" aria-labelledby="smart-label">
        {#each smartFolders as smart (smart.id)}
          <button
            class="item"
            class:selected={activeSmart === smart.id}
            aria-pressed={activeSmart === smart.id}
            onclick={() => onsmart(smart)}
            oncontextmenu={(e) => {
              e.preventDefault();
              onsmartmenu(smart, e);
            }}
          >
            <span class="icon"><Icon name="search" /></span>
            <span>{smart.name}</span>
          </button>
        {/each}
        <button class="item create" onclick={onsmartnew}>
          <span class="icon"><Icon name="plus" /></span>
          <span>New smart folder…</span>
        </button>
      </div>
    {/if}
  </div>

  <!-- Apart from the places above: the bin is not a folder to pin or drop on. -->
  <div class="bin">
    <button
      class="item"
      class:selected={binShown}
      aria-current={binShown ? "page" : undefined}
      onclick={onbin}
    >
      <Icon name="trash" />
      <span>Recycle Bin</span>
    </button>
  </div>
</aside>

<style>
  .sidebar {
    width: 236px;
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 0 10px 12px;
    background: var(--bg-sidebar);
    overflow: hidden;
  }

  /* Favorites and tags scroll; the bin below stays in view. */
  .places {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
    overflow-y: auto;
  }

  .bin {
    flex-shrink: 0;
    padding-top: var(--space-sm);
    border-top: 1px solid var(--border-default);
  }

  /* Same height as the tab bar, so the whole top edge of the window drags it. */
  .brand {
    display: flex;
    align-items: center;
    gap: 10px;
    height: var(--size-titlebar);
    flex-shrink: 0;
    padding: 0 8px;
    margin-bottom: 10px;
  }

  /* Drag regions only react to presses on the element itself, not its children. */
  .brand > * {
    pointer-events: none;
  }

  .logo {
    display: grid;
    place-items: center;
    width: 24px;
    height: 24px;
    border-radius: 7px;
    color: #fff;
    background: linear-gradient(135deg, #7b93ff, #4d6bfe);
  }

  .name {
    font-size: var(--font-size-title);
    font-weight: var(--font-weight-bold);
    letter-spacing: -0.2px;
  }

  .label {
    display: flex;
    align-items: center;
    gap: var(--space-xs);
    width: 100%;
    padding: 12px 10px 4px;
    text-align: left;
    font-size: var(--font-size-caption);
    font-weight: var(--font-weight-semibold);
    letter-spacing: 0.4px;
    color: var(--text-tertiary);
  }

  .label:hover {
    color: var(--text-secondary);
  }

  .chevron {
    display: flex;
    transition: rotate 120ms ease-out;
  }

  .chevron.folded {
    rotate: -90deg;
  }

  .item {
    display: flex;
    align-items: center;
    gap: 10px;
    height: 30px;
    padding: 0 10px;
    border-radius: 7px;
    color: var(--text-secondary);
    text-align: left;
  }

  .item span {
    color: var(--text-primary);
  }

  .favorites {
    display: flex;
    flex-direction: column;
    gap: var(--space-2xs);
    border-radius: var(--radius-sm);
  }

  /* Where a dragged folder will be pinned: a line in the gap above or below an item, drawn on
     the item so it takes no room and nothing moves when it appears. */
  .line-before,
  .line-after {
    position: relative;
  }

  .line-before::before,
  .line-after::before {
    content: "";
    position: absolute;
    right: var(--space-sm);
    left: var(--space-sm);
    height: 2px;
    border-radius: 1px;
    background: var(--accent-default);
    pointer-events: none;
  }

  .line-before::before {
    top: calc(-1 * var(--space-2xs));
  }

  .line-after::before {
    bottom: calc(-1 * var(--space-2xs));
  }

  @media (forced-colors: active) {
    .line-before::before,
    .line-after::before {
      background: Highlight;
    }
  }

  .item .icon {
    display: flex;
    color: inherit;
  }

  .item:hover {
    background: var(--bg-hover);
  }

  .item.selected {
    background: var(--bg-hover);
    color: var(--accent-default);
    font-weight: var(--font-weight-medium);
  }

  .item.drop {
    outline: 2px solid var(--accent-default);
    outline-offset: -2px;
  }

  .drive {
    height: auto;
    padding-block: var(--space-sm);
  }

  .drive-info {
    flex: 1;
    min-width: 0;
    display: grid;
    grid-template-columns: 1fr auto;
    align-items: baseline;
    gap: var(--space-xs) var(--space-sm);
  }

  .usage {
    grid-column: 1 / -1;
    grid-row: 2;
    height: 4px;
    border-radius: var(--radius-full);
    background: var(--bg-control);
    overflow: hidden;
  }

  .fill {
    display: block;
    height: 100%;
    border-radius: inherit;
    background: var(--accent-default);
  }

  .usage.full .fill {
    background: var(--status-warning);
  }

  .item .free {
    grid-column: 2;
    grid-row: 1;
    font-size: var(--font-size-caption);
    color: var(--text-tertiary);
  }

  .create {
    color: var(--text-secondary);
  }

  .tag {
    gap: 12px;
    padding-left: 13px;
  }

  .dot {
    width: 10px;
    height: 10px;
    border-radius: 50%;
  }
</style>
