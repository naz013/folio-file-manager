<script lang="ts" module>
  /** How long a tile rests on a file before its picture is asked for: tiles that scroll past don't cost a request. */
  const SETTLE_MS = 80;
  /** The wait before a failed picture is asked for once more. */
  const RETRY_MS = 400;
</script>

<script lang="ts">
  import { thumbUrl } from "$lib/api/fs";
  import { thumbnailArt } from "$lib/plugins.svelte";
  import FileIcon from "./FileIcon.svelte";

  /**
   * Shows the OS thumbnail for a file, falling back to the built-in file icon. `size` is how
   * big it is drawn; `ask` is the size to ask the core for, when that should hold still while
   * `size` changes (a pinch), the picture then being scaled in the meantime.
   */
  let { path, name, size, ask }: { path: string; name: string; size: number; ask?: number } =
    $props();

  // A plugin's picture for this kind of file comes first: for text, JSON and the like Windows
  // answers with its file-type icon, which is no preview, and that would hide the plugin's art.
  let fileName = $derived(name || path.split(/[\\/]/).pop() || "");
  let pluginArt = $derived(
    fileName.includes(".") ? thumbnailArt(fileName.split(".").pop()!.toLowerCase()) : null,
  );

  // Thumbnails are requested at 2x for crisp rendering on HiDPI screens.
  let base = $derived(thumbUrl(path, (ask ?? size) * 2));
  // The newest request is served first, so an older one for a tile still in view can run out
  // its time while the page scrolls. A failed picture is asked for once more, as the newest.
  let retrying = $state<string | null>(null);
  let src = $derived(retrying === base ? `${base}&r=1` : base);
  let retryTimer: ReturnType<typeof setTimeout> | undefined;
  // A retry belongs to the file it was scheduled for: not to a recycled tile, nor a gone one.
  $effect(() => {
    void base;
    return () => clearTimeout(retryTimer);
  });

  // Remember which src failed, so a recycled row showing a new file starts fresh. A picture
  // that has shown stays until a sharper one for the same file has loaded over it.
  let loadedPath = $state<string | null>(null);
  let failedSrc = $state<string | null>(null);
  let loaded = $derived(loadedPath === path);
  let failed = $derived(failedSrc === src);

  /** The picture being asked for: it follows `src` once that has held still. */
  let asked = $state<{ path: string; src: string } | null>(null);
  $effect(() => {
    const next = { path, src };
    const timer = setTimeout(() => (asked = next), SETTLE_MS);
    return () => clearTimeout(timer);
  });
  /** What the tile shows: what was asked for this file, which may be an earlier size of it. */
  let current = $derived(asked && asked.path === path ? asked.src : null);

  /** The last picture that loaded, kept on show if a sharper one for the same file fails. */
  let good = $state<string | null>(null);
  let refused = $state<string | null>(null);
  let shown = $derived(refused === current && good ? good : current);
</script>

{#if pluginArt}
  <FileIcon name={fileName} {size} />
{:else if failed}
  <FileIcon {name} {size} />
{:else if shown}
  <img
    src={shown}
    alt=""
    width={size}
    height={size}
    class:loaded
    decoding="async"
    draggable="false"
    onload={() => {
      loadedPath = path;
      good = shown;
    }}
    onerror={() => {
      if (retrying === base) {
        // A picture is already showing: a failed refresh leaves it as it is.
        if (loaded && good) refused = current;
        else failedSrc = src;
      } else {
        const again = base;
        retryTimer = setTimeout(() => (retrying = again), RETRY_MS);
      }
    }}
  />
{:else}
  <span class="waiting" style:width="{size}px" style:height="{size}px"></span>
{/if}

<style>
  img {
    object-fit: contain;
    opacity: 0;
    transition: opacity var(--duration-fast) var(--ease-standard);
  }

  img.loaded {
    opacity: 1;
  }

  .waiting {
    display: inline-block;
  }
</style>
