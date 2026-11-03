<script lang="ts" module>
  /** How long a tile rests on a file before its picture is asked for: tiles that scroll past don't cost a request. */
  const SETTLE_MS = 80;
</script>

<script lang="ts">
  import { thumbUrl } from "$lib/api/fs";
  import FileIcon from "./FileIcon.svelte";

  /** Shows the OS thumbnail for a file, falling back to the built-in file icon. */
  let { path, name, size }: { path: string; name: string; size: number } = $props();

  // Thumbnails are requested at 2x for crisp rendering on HiDPI screens.
  let src = $derived(thumbUrl(path, size * 2));

  // Remember which src loaded or failed, so a recycled row showing a new file starts fresh.
  let loadedSrc = $state<string | null>(null);
  let failedSrc = $state<string | null>(null);
  let loaded = $derived(loadedSrc === src);
  let failed = $derived(failedSrc === src);

  /** The src whose picture is being asked for: it follows `src` once that has held still. */
  let asked = $state<string | null>(null);
  $effect(() => {
    const next = src;
    const timer = setTimeout(() => (asked = next), SETTLE_MS);
    return () => clearTimeout(timer);
  });
</script>

{#if failed}
  <FileIcon {name} {size} />
{:else if asked === src}
  <img
    {src}
    alt=""
    width={size}
    height={size}
    class:loaded
    decoding="async"
    draggable="false"
    onload={() => (loadedSrc = src)}
    onerror={() => (failedSrc = src)}
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
