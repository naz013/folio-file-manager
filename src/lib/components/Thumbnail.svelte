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
</script>

{#if failed}
  <FileIcon {name} {size} />
{:else}
  <img
    {src}
    alt=""
    width={size}
    height={size}
    class:loaded
    loading="lazy"
    decoding="async"
    draggable="false"
    onload={() => (loadedSrc = src)}
    onerror={() => (failedSrc = src)}
  />
{/if}

<style>
  img {
    object-fit: contain;
    opacity: 0;
    transition: opacity 120ms ease-out;
  }

  img.loaded {
    opacity: 1;
  }
</style>
