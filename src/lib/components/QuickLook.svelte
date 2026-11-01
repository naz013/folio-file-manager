<script lang="ts">
  import { onMount } from "svelte";
  import { previewUrl, readTextPreview, releasePreview, type Entry } from "$lib/api/fs";
  import { formatDate, formatSize } from "$lib/format";
  import { previewKind } from "$lib/preview";
  import FileIcon from "./FileIcon.svelte";
  import FolderIcon from "./FolderIcon.svelte";

  let {
    entry,
    position,
    total,
    onopen,
    ontools,
    onclose,
  }: {
    entry: Entry;
    /** 1-based place of the file among those in the folder. */
    position: number;
    total: number;
    /** "Open": hand the file to its default app. */
    onopen: () => void;
    /** "PDF tools": merge or compress; given only for a PDF. */
    ontools?: () => void;
    onclose: () => void;
  } = $props();

  type Shown =
    | { kind: "loading" }
    | { kind: "url"; media: "image" | "pdf" | "audio" | "video"; url: string }
    | { kind: "text"; text: string; truncated: boolean }
    | { kind: "none"; why: string };

  let shown = $state<Shown>({ kind: "loading" });
  let dialog: HTMLElement;

  // The keys belong to this window, not to whatever was focused behind it.
  onMount(() => {
    dialog.focus();
    return () => {
      releasePreview().catch(() => {});
    };
  });

  // Only what identifies the file reloads the preview: a refresh of the folder hands over a
  // new object for the same file, and must not restart playing audio.
  const path = $derived(entry.path);
  const name = $derived(entry.name);
  const isDir = $derived(entry.isDir);

  // Load what the current file needs; a newer file drops the answer to an older one.
  $effect(() => {
    const kind = previewKind(name, isDir);
    let current = true;
    shown = { kind: "loading" };
    if (kind === "other") {
      shown = { kind: "none", why: isDir ? "Folder" : "No preview for this kind of file" };
    } else if (kind === "text") {
      readTextPreview(path)
        .then((t) => current && (shown = { kind: "text", ...t }))
        .catch(
          () => current && (shown = { kind: "none", why: "No preview for this kind of file" }),
        );
    } else {
      previewUrl(path)
        .then((url) => current && (shown = { kind: "url", media: kind, url }))
        .catch((e) => current && (shown = { kind: "none", why: String(e) }));
    }
    return () => (current = false);
  });
</script>

<div class="backdrop" role="presentation" onpointerdown={onclose}>
  <div
    bind:this={dialog}
    class="window"
    role="dialog"
    aria-modal="true"
    aria-label="Quick Look: {entry.name}"
    tabindex="-1"
    onpointerdown={(e) => e.stopPropagation()}
  >
    <header>
      <div class="title">
        <span class="name">{entry.name}</span>
        <span class="meta">
          {entry.isDir ? "Folder" : formatSize(entry.size)} · {formatDate(entry.modified)}
        </span>
      </div>
      <span class="count">{position} of {total}</span>
      {#if ontools}<button class="open" onclick={ontools}>PDF tools…</button>{/if}
      <button class="open" onclick={onopen}>Open</button>
      <button class="open" onclick={onclose}>Close</button>
    </header>

    <div class="stage">
      {#if shown.kind === "loading"}
        <span class="note">Loading…</span>
      {:else if shown.kind === "url" && shown.media === "image"}
        <img src={shown.url} alt={entry.name} draggable="false" />
      {:else if shown.kind === "url" && shown.media === "pdf"}
        <iframe src={shown.url} title={entry.name}></iframe>
      {:else if shown.kind === "url" && shown.media === "audio"}
        <div class="audio">
          <FileIcon name={entry.name} size={96} />
          <audio src={shown.url} controls autoplay></audio>
        </div>
      {:else if shown.kind === "url"}
        <!-- Files in a folder come without captions. -->
        <!-- svelte-ignore a11y_media_has_caption -->
        <video src={shown.url} controls autoplay></video>
      {:else if shown.kind === "text"}
        <pre>{shown.text}{#if shown.truncated}
            … the rest of the file is not shown{/if}</pre>
      {:else if shown.kind === "none"}
        <div class="card">
          {#if entry.isDir}<FolderIcon size={96} />{:else}<FileIcon
              name={entry.name}
              size={96}
            />{/if}
          <span class="note">{shown.why}</span>
        </div>
      {/if}
    </div>

    <footer>← → to move · Space or Esc to close</footer>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 55;
    display: grid;
    place-items: center;
    background: var(--scrim-strong);
  }

  .window {
    display: grid;
    grid-template-rows: auto 1fr auto;
    width: min(960px, calc(100vw - 2 * var(--space-xl)));
    height: min(680px, calc(100vh - 2 * var(--space-xl)));
    border: 1px solid var(--border-default);
    border-radius: var(--radius-xl);
    background: var(--bg-window);
    box-shadow: var(--shadow-popover);
    overflow: hidden;
    outline: none;
  }

  header {
    display: flex;
    align-items: center;
    gap: var(--space-md);
    padding: var(--space-sm) var(--space-lg);
    border-bottom: 1px solid var(--border-default);
  }

  .title {
    display: grid;
    flex: 1;
    min-width: 0;
  }

  .name {
    overflow: hidden;
    font-weight: var(--font-weight-semibold);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .meta,
  .count,
  footer,
  .note {
    color: var(--text-secondary);
    font-size: var(--font-size-caption);
  }

  .open {
    padding: var(--space-xs) var(--space-md);
    border-radius: var(--radius-md);
    background: var(--bg-control);
    font-weight: var(--font-weight-medium);
  }

  .open:hover {
    background: var(--bg-hover);
  }

  .stage {
    display: grid;
    place-items: center;
    min-height: 0;
    overflow: auto;
    background: var(--bg-subtle);
  }

  img,
  video {
    max-width: 100%;
    max-height: 100%;
    object-fit: contain;
  }

  iframe {
    width: 100%;
    height: 100%;
    border: 0;
  }

  .audio,
  .card {
    display: grid;
    justify-items: center;
    gap: var(--space-lg);
  }

  pre {
    align-self: stretch;
    justify-self: stretch;
    margin: 0;
    padding: var(--space-lg);
    overflow: auto;
    font-family: var(--font-mono);
    font-size: var(--font-size-caption);
    line-height: 1.5;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    user-select: text;
  }

  footer {
    padding: var(--space-xs) var(--space-lg);
    border-top: 1px solid var(--border-default);
    text-align: center;
  }
</style>
