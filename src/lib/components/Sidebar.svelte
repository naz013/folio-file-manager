<script lang="ts">
  import Icon, { type IconName } from "./Icon.svelte";
  import { joinPath, type Tag } from "$lib/api/fs";
  import { tagColor } from "$lib/tags";

  let {
    home,
    current,
    onnavigate,
    onopentab,
    dropTarget,
    springPath,
    springMs,
    tags,
    activeTag,
    ontag,
    onbin,
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
    tags: Tag[];
    /** The tag whose items the list shows, if any. */
    activeTag: number | null;
    /** Show every item with this tag. */
    ontag: (tag: Tag) => void;
    /** Open the Recycle Bin. */
    onbin: () => void;
  } = $props();

  const favorites: { label: string; icon: IconName; sub: string | null }[] = [
    { label: "Home", icon: "home", sub: null },
    { label: "Desktop", icon: "monitor", sub: "Desktop" },
    { label: "Documents", icon: "fileText", sub: "Documents" },
    { label: "Downloads", icon: "download", sub: "Downloads" },
    { label: "Pictures", icon: "image", sub: "Pictures" },
  ];
</script>

<aside class="sidebar">
  <!-- The window has no native title bar; the brand row is part of the drag area. -->
  <div class="brand" data-tauri-drag-region>
    <span class="logo"><Icon name="folder" size={14} stroke={2.2} /></span>
    <span class="name">Folio</span>
  </div>

  <div class="label">Favorites</div>
  {#each favorites as fav (fav.label)}
    {@const path = fav.sub ? joinPath(home, fav.sub) : home}
    <button
      class="item"
      class:selected={path === current}
      class:drop={dropTarget === path}
      class:springing={springPath === path}
      style:--spring-ms="{springMs}ms"
      data-drop-path={path}
      onclick={() => onnavigate(path)}
      onmousedown={(e) => e.button === 1 && e.preventDefault()}
      onauxclick={(e) => e.button === 1 && onopentab(path)}
    >
      <Icon name={fav.icon} />
      <span>{fav.label}</span>
    </button>
  {/each}

  <button class="item" onclick={onbin}>
    <Icon name="trash" />
    <span>Recycle Bin</span>
  </button>

  <div class="label">Tags</div>
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
  {/each}
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
    overflow-y: auto;
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
    padding: 12px 10px 4px;
    font-size: var(--font-size-caption);
    font-weight: var(--font-weight-semibold);
    letter-spacing: 0.4px;
    color: var(--text-tertiary);
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
