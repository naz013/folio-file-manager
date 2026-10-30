<script lang="ts">
  import Icon, { type IconName } from "./Icon.svelte";
  import { joinPath } from "$lib/api/fs";
  import { DRAG_MIME } from "./FileView.svelte";

  let {
    home,
    current,
    onnavigate,
    ondrop,
  }: {
    home: string;
    current: string;
    onnavigate: (path: string) => void;
    ondrop: (paths: string[], dest: string, copy: boolean) => void;
  } = $props();

  let dropTarget = $state<string | null>(null);

  function dragOver(e: DragEvent, path: string) {
    if (!e.dataTransfer?.types.includes(DRAG_MIME)) return;
    e.preventDefault();
    e.dataTransfer.dropEffect = e.ctrlKey ? "copy" : "move";
    dropTarget = path;
  }

  function drop(e: DragEvent, path: string) {
    dropTarget = null;
    const raw = e.dataTransfer?.getData(DRAG_MIME);
    if (!raw) return;
    e.preventDefault();
    ondrop(JSON.parse(raw), path, e.ctrlKey);
  }

  const favorites: { label: string; icon: IconName; sub: string | null }[] = [
    { label: "Home", icon: "home", sub: null },
    { label: "Desktop", icon: "monitor", sub: "Desktop" },
    { label: "Documents", icon: "fileText", sub: "Documents" },
    { label: "Downloads", icon: "download", sub: "Downloads" },
    { label: "Pictures", icon: "image", sub: "Pictures" },
  ];

  const tags = [
    ["Work", "--tag-red"],
    ["Urgent", "--tag-orange"],
    ["Ideas", "--tag-yellow"],
    ["Done", "--tag-green"],
    ["Personal", "--tag-blue"],
    ["Design", "--tag-purple"],
  ];
</script>

<aside class="sidebar">
  <div class="brand">
    <span class="logo"><Icon name="folder" size={14} stroke={2.2} /></span>
    <span class="name">Folio</span>
  </div>

  <div class="label">Favorites</div>
  {#each favorites as fav}
    {@const path = fav.sub ? joinPath(home, fav.sub) : home}
    <button
      class="item"
      class:selected={path === current}
      class:drop={dropTarget === path}
      onclick={() => onnavigate(path)}
      ondragover={(e) => dragOver(e, path)}
      ondragleave={() => (dropTarget = null)}
      ondrop={(e) => drop(e, path)}>
      <Icon name={fav.icon} />
      <span>{fav.label}</span>
    </button>
  {/each}

  <div class="label">Tags</div>
  {#each tags as [name, color]}
    <div class="item tag">
      <span class="dot" style:background="var({color})"></span>
      <span>{name}</span>
    </div>
  {/each}
</aside>

<style>
  .sidebar {
    width: 236px;
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 14px 10px 12px;
    background: var(--bg-sidebar);
    overflow-y: auto;
  }

  .brand {
    display: flex;
    align-items: center;
    gap: 10px;
    height: 32px;
    padding: 0 8px;
    margin-bottom: 10px;
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
