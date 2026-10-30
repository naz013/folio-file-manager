<script lang="ts">
  import { thumbUrl } from "$lib/api/fs";
  import FileIcon from "./FileIcon.svelte";

  /** Shows the OS thumbnail for a file, falling back to the built-in file icon. */
  let { path, name, size }: { path: string; name: string; size: number } = $props();

  let failed = $state(false);
  let loaded = $state(false);

  // Thumbnails are requested at 2x for crisp rendering on HiDPI screens.
  let src = $derived(thumbUrl(path, size * 2));

  $effect(() => {
    src;
    failed = false;
    loaded = false;
  });
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
    onload={() => (loaded = true)}
    onerror={() => (failed = true)} />
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
