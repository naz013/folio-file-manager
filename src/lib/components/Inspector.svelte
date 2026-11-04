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

  /**
   * Ids for the folder counts, so a newer selection can stop the ones under way. They start
   * from the time, so a reloaded page or a second window does not reuse an id still in use.
   */
  let nextJob = Date.now();
  /** How long the open folder is shown before it is counted. */
  const OPEN_SETTLE_MS = 300;
  /** How long a folder that has changed under the inspector waits before it is counted again. */
  const RECOUNT_MS = 2000;
  /** Why a folder has no count, in the words for it. */
  const WHY: Record<string, string> = {
    network: "Not counted on a network folder",
    cloud: "Not counted: the folder is online only",
    large: "Too big to count here",
  };
</script>

<script lang="ts">
  import { untrack } from "svelte";
  import { measureCancel, measureDir, type Entry, type Measured, type Tag } from "$lib/api/fs";
  import { formatDate, formatSize } from "$lib/format";
  import { hasDetails } from "$lib/imageinfo";
  import { tagColor } from "$lib/tags";
  import FolderIcon from "./FolderIcon.svelte";
  import ImageDetails from "./ImageDetails.svelte";
  import Thumbnail from "./Thumbnail.svelte";

  let {
    folder,
    entries,
    picked,
    tags,
    have,
    ontoggle,
    clearControls = false,
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
    /** Window buttons are drawn over the top of this panel: start below them. */
    clearControls?: boolean;
  } = $props();

  const total = (items: Entry[]) => items.reduce((sum, e) => sum + (e.isDir ? 0 : e.size), 0);

  /** What each selected folder holds, as last counted. A folder's size is not in the listing:
   *  it is counted from the disk in the background each time it is selected, and the last
   *  answer stays up until the new one arrives. */
  const measured = $state<Record<string, Measured>>({});
  /** The folders counted since the selection last changed. */
  let fresh = $state<string[]>([]);
  const selectedFolders = $derived(
    picked
      .filter((e) => e.isDir)
      .map((e) => e.path)
      .join("\u0000"),
  );

  $effect(() => {
    const paths = selectedFolders ? selectedFolders.split("\u0000") : [];
    untrack(() => (fresh = []));
    let current = true;
    let active = 0;
    // One folder at a time, so a big selection doesn't read the whole disk at once.
    (async () => {
      for (const path of paths) {
        if (!current) return;
        const job = ++nextJob;
        active = job;
        const m = await measureDir(job, path).catch(() => null);
        // Finished: there is nothing left to stop.
        active = 0;
        if (!current) return;
        if (m) {
          measured[path] = m;
          fresh = [...fresh, path];
        }
      }
    })();
    return () => {
      current = false;
      if (active) measureCancel(active).catch(() => {});
    };
  });

  /** What the open folder holds in all, as last counted, for the folder it was counted for. */
  let opened = $state<{ path: string; m: Measured } | null>(null);
  /** Whether the open folder has been counted since it was shown or last changed. */
  let openedFresh = $state(false);

  // With nothing selected, the open folder is counted in the background like a selected one.
  // It waits a moment first, so passing through folders doesn't start a count in each, and
  // it stops when the folder changes, something is selected, or the window shows another folder.
  $effect(() => {
    const path = picked.length ? null : folder;
    // A change in what the folder holds counts it again.
    void entries.length;
    if (!path) return;
    // A folder already counted that merely changed waits longer: one that keeps changing
    // would otherwise be counted over and over.
    const delay = untrack(() => {
      openedFresh = false;
      return opened?.path === path ? RECOUNT_MS : OPEN_SETTLE_MS;
    });
    let current = true;
    let started = false;
    let finished = false;
    const job = ++nextJob;
    const timer = setTimeout(async () => {
      started = true;
      const m = await measureDir(job, path, true).catch(() => null);
      finished = true;
      if (!current || !m) return;
      opened = { path, m };
      openedFresh = true;
    }, delay);
    return () => {
      current = false;
      clearTimeout(timer);
      // Only a count that is running needs stopping; one that never began or has ended has
      // nothing to stop, and a stop for it would be left behind.
      if (started && !finished) measureCancel(job).catch(() => {});
    };
  });

  const pickedFolders = $derived(picked.filter((e) => e.isDir).map((e) => e.path));
  const counting = (paths: string[]) => paths.some((p) => !fresh.includes(p));
  /** The size of the selection: files as listed, folders as counted. Null until every folder
   *  has been counted at least once; "unknown" if one was too big to count. */
  const selectionBytes = (items: Entry[]): number | null | "unknown" => {
    let sum = 0;
    let unknown = false;
    for (const e of items) {
      if (!e.isDir) sum += e.size;
      else if (measured[e.path]?.skipped) unknown = true;
      else if (measured[e.path]) sum += measured[e.path].bytes;
      else return null;
    }
    return unknown ? "unknown" : sum;
  };
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

  const pickedBytes = $derived(selectionBytes(picked));

  const folders = (items: Entry[]) => items.filter((e) => e.isDir).length;
</script>

<aside class="inspector" class:clear={clearControls} aria-label="Inspector">
  {#if clearControls}<div class="grab" data-tauri-drag-region></div>{/if}
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
      <dt>Size</dt>
      {#if !item.isDir}
        <dd>{formatSize(item.size)}</dd>
      {:else}
        {@const m = measured[item.path]}
        <dd class:busy={counting([item.path])} aria-busy={counting([item.path])}>
          {#if m?.skipped}Too big to count{:else if m}{formatSize(m.bytes)} · {plural(
              m.files,
              "file",
            )}{:else}Calculating…{/if}
        </dd>
      {/if}
      <dt>Modified</dt>
      <dd>{formatDate(item.modified)}</dd>
      <dt>Where</dt>
      <dd class="path">{item.path}</dd>
    </dl>
    {#if !item.isDir && hasDetails(item.name)}
      {#key item.path}<ImageDetails path={item.path} layout="list" />{/key}
    {/if}
  {:else if picked.length > 1}
    <h2 class="name">{plural(picked.length, "item")} selected</h2>
    <dl>
      <dt>Files</dt>
      <dd>{picked.length - folders(picked)}</dd>
      <dt>Folders</dt>
      <dd>{folders(picked)}</dd>
      <dt>Total size</dt>
      <dd class:busy={counting(pickedFolders)} aria-busy={counting(pickedFolders)}>
        {pickedBytes === null
          ? "Calculating…"
          : pickedBytes === "unknown"
            ? "Too big to count"
            : formatSize(pickedBytes)}
      </dd>
    </dl>
  {:else}
    <div class="preview"><FolderIcon size={96} /></div>
    <h2 class="name">
      {folder ? (folder.split(/[\\/]/).filter(Boolean).pop() ?? folder) : "Tagged items"}
    </h2>
    <dl>
      <dt>Items</dt>
      <dd>{entries.length}</dd>
      {#if folder}
        {@const here = opened?.path === folder ? opened.m : null}
        <dt>Size</dt>
        <dd class:busy={!openedFresh && !here?.skipped} aria-busy={!openedFresh && !here?.skipped}>
          {#if here?.skipped}
            {WHY[here.skipped]} · {plural(entries.length - folders(entries), "file")} here, {formatSize(
              total(entries),
            )}
          {:else if here}
            {formatSize(here.bytes)} · {plural(here.files, "file")}
          {:else}
            Calculating…
          {/if}
        </dd>
      {:else}
        <dt>Files</dt>
        <dd>{entries.length - folders(entries)} · {formatSize(total(entries))}</dd>
      {/if}
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
      <!-- The kinds come from the listing, which has only the folder's own files. -->
      <h3>
        {!folder
          ? "Space in these items"
          : folders(entries)
            ? "Space in this folder's own files"
            : "Space in this folder"}
      </h3>
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
  .busy {
    opacity: 0.6;
  }

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

  .inspector.clear {
    padding-top: 0;
  }

  /* The strip under the window buttons: still drags the window. */
  .grab {
    flex-shrink: 0;
    height: var(--size-titlebar);
    margin: 0 calc(var(--space-lg) * -1);
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
