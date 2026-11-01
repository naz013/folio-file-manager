<script lang="ts" module>
  /** What the space breakdown groups files by, judged by extension. */
  const KINDS: { name: string; color: string; exts: string[] }[] = [
    {
      name: "Images",
      color: "--tag-green",
      exts: [
        "png",
        "jpg",
        "jpeg",
        "gif",
        "webp",
        "bmp",
        "svg",
        "ico",
        "avif",
        "heic",
        "tif",
        "tiff",
      ],
    },
    {
      name: "Video",
      color: "--tag-purple",
      exts: ["mp4", "mkv", "mov", "avi", "webm", "m4v", "wmv"],
    },
    {
      name: "Audio",
      color: "--tag-orange",
      exts: ["mp3", "wav", "flac", "ogg", "m4a", "aac", "opus"],
    },
    {
      name: "Documents",
      color: "--tag-blue",
      exts: ["pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "txt", "md", "rtf", "odt", "csv"],
    },
    {
      name: "Archives",
      color: "--tag-yellow",
      exts: ["zip", "7z", "rar", "tar", "gz", "bz2", "xz", "iso"],
    },
  ];

  export function kindOf(name: string): string {
    const dot = name.lastIndexOf(".");
    const ext = dot > 0 ? name.slice(dot + 1).toLowerCase() : "";
    return KINDS.find((k) => k.exts.includes(ext))?.name ?? "Other";
  }

  const colorOf = (kind: string) => KINDS.find((k) => k.name === kind)?.color ?? "--tag-gray";
</script>

<script lang="ts">
  import type { Entry, Tag } from "$lib/api/fs";
  import { formatDate, formatSize } from "$lib/format";
  import { tagColor } from "$lib/tags";
  import FolderIcon from "./FolderIcon.svelte";
  import Thumbnail from "./Thumbnail.svelte";

  let {
    folder,
    entries,
    picked,
    tags,
    have,
    ontoggle,
  }: {
    /** The folder shown, or null in a tag view. */
    folder: string | null;
    /** Everything shown in the list. */
    entries: Entry[];
    /** The selected items. */
    picked: Entry[];
    tags: Tag[];
    /** For each tag id: whether all, some or none of the selected items have it. */
    have: Record<number, "all" | "some" | "none">;
    ontoggle: (id: number) => void;
  } = $props();

  const total = (items: Entry[]) => items.reduce((sum, e) => sum + (e.isDir ? 0 : e.size), 0);
  const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;

  /** Where the space goes: bytes by kind of file, largest first. */
  let breakdown = $derived.by(() => {
    const by: Record<string, number> = {};
    for (const e of entries) {
      if (e.isDir) continue;
      const kind = kindOf(e.name);
      by[kind] = (by[kind] ?? 0) + e.size;
    }
    return Object.entries(by).sort((a, b) => b[1] - a[1]);
  });
  let breakdownTotal = $derived(breakdown.reduce((sum, [, bytes]) => sum + bytes, 0));

  const folders = (items: Entry[]) => items.filter((e) => e.isDir).length;
</script>

<aside class="inspector" aria-label="Inspector">
  {#if picked.length === 1}
    {@const item = picked[0]}
    <div class="preview">
      {#if item.isDir}<FolderIcon size={96} />{:else}<Thumbnail
          path={item.path}
          name={item.name}
          size={128}
        />{/if}
    </div>
    <h2 class="name">{item.name}</h2>
    <dl>
      <dt>Kind</dt>
      <dd>{item.isDir ? "Folder" : kindOf(item.name)}</dd>
      {#if !item.isDir}<dt>Size</dt>
        <dd>{formatSize(item.size)}</dd>{/if}
      <dt>Modified</dt>
      <dd>{formatDate(item.modified)}</dd>
      <dt>Where</dt>
      <dd class="path">{item.path}</dd>
    </dl>
  {:else if picked.length > 1}
    <h2 class="name">{plural(picked.length, "item")} selected</h2>
    <dl>
      <dt>Files</dt>
      <dd>{picked.length - folders(picked)}</dd>
      <dt>Folders</dt>
      <dd>{folders(picked)}</dd>
      <dt>Total size</dt>
      <dd>{formatSize(total(picked))}</dd>
    </dl>
  {:else}
    <div class="preview"><FolderIcon size={96} /></div>
    <h2 class="name">
      {folder ? (folder.split(/[\\/]/).filter(Boolean).pop() ?? folder) : "Tagged items"}
    </h2>
    <dl>
      <dt>Items</dt>
      <dd>{entries.length}</dd>
      <dt>Files</dt>
      <dd>{entries.length - folders(entries)} · {formatSize(total(entries))}</dd>
      {#if folder}<dt>Where</dt>
        <dd class="path">{folder}</dd>{/if}
    </dl>
  {/if}

  {#if picked.length}
    <section>
      <h3>Tags</h3>
      <div class="tags">
        {#if !tags.length}<span class="empty">No tags yet</span>{/if}
        {#each tags as tag (tag.id)}
          <button
            class="tag"
            class:on={have[tag.id] === "all"}
            class:some={have[tag.id] === "some"}
            aria-pressed={have[tag.id] === "all" ? true : have[tag.id] === "some" ? "mixed" : false}
            title={have[tag.id] === "some" ? "Some of the selected items have this tag" : undefined}
            onclick={() => ontoggle(tag.id)}
          >
            <span class="dot" style:background={tagColor(tag.color)}></span>
            {tag.name}
          </button>
        {/each}
      </div>
    </section>
  {:else if breakdownTotal > 0}
    <section>
      <h3>{folder ? "Space in this folder" : "Space in these items"}</h3>
      <div class="bar" role="img" aria-label="Space by kind of file">
        {#each breakdown as [kind, bytes] (kind)}
          <span
            style:flex={bytes}
            style:background="var({colorOf(kind)})"
            title="{kind}: {formatSize(bytes)}"
          ></span>
        {/each}
      </div>
      <ul class="legend">
        {#each breakdown as [kind, bytes] (kind)}
          <li>
            <span class="dot" style:background="var({colorOf(kind)})"></span>
            <span class="kind">{kind}</span>
            <span class="bytes">{formatSize(bytes)}</span>
          </li>
        {/each}
      </ul>
    </section>
  {/if}
</aside>

<style>
  .inspector {
    display: flex;
    flex-direction: column;
    gap: var(--space-md);
    width: var(--size-inspector);
    flex-shrink: 0;
    padding: var(--space-lg);
    overflow-y: auto;
    border-left: 1px solid var(--border-default);
    background: var(--bg-sidebar);
  }

  .preview {
    display: grid;
    place-items: center;
    min-height: 140px;
    padding: var(--space-md);
    border-radius: var(--radius-lg);
    background: var(--bg-subtle);
  }

  .name {
    margin: 0;
    font-size: var(--font-size-title);
    font-weight: var(--font-weight-semibold);
    overflow-wrap: anywhere;
  }

  h3 {
    margin: 0 0 var(--space-xs);
    color: var(--text-tertiary);
    font-size: var(--font-size-caption);
    font-weight: var(--font-weight-semibold);
    letter-spacing: 0.4px;
    text-transform: uppercase;
  }

  dl {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: var(--space-xs) var(--space-md);
    margin: 0;
    font-size: var(--font-size-caption);
  }

  dt {
    color: var(--text-secondary);
  }

  dd.path {
    text-align: left;
    grid-column: 1 / -1;
  }

  .empty {
    color: var(--text-tertiary);
    font-size: var(--font-size-caption);
  }

  dd {
    margin: 0;
    text-align: right;
    overflow-wrap: anywhere;
    user-select: text;
  }

  .tags {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-xs);
  }

  .tag {
    display: inline-flex;
    align-items: center;
    gap: var(--space-xs);
    padding: var(--space-2xs) var(--space-sm);
    border: 1px solid var(--border-default);
    border-radius: 999px;
    color: var(--text-secondary);
    font-size: var(--font-size-caption);
  }

  .tag:hover {
    background: var(--bg-hover);
  }

  .tag.on {
    border-color: var(--accent-default);
    background: var(--accent-subtle);
    color: var(--text-primary);
  }

  .tag.some {
    border-style: dashed;
    border-color: var(--accent-default);
  }

  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
  }

  .bar {
    display: flex;
    gap: 1px;
    height: 10px;
    overflow: hidden;
    border-radius: var(--radius-sm);
    background: var(--bg-control);
  }

  .legend {
    margin: var(--space-sm) 0 0;
    padding: 0;
    list-style: none;
    font-size: var(--font-size-caption);
  }

  .legend li {
    display: flex;
    align-items: center;
    gap: var(--space-xs);
    padding: var(--space-2xs) 0;
  }

  .kind {
    flex: 1;
  }

  .bytes {
    color: var(--text-secondary);
    font-variant-numeric: tabular-nums;
  }
</style>
