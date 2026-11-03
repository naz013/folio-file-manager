<script lang="ts">
  import { onMount } from "svelte";
  import { CLOUD_ONLY, previewUrl, readTextPreview, releasePreview, type Entry } from "$lib/api/fs";
  import { formatDate, formatSize } from "$lib/format";
  import { previewKind } from "$lib/preview";
  import { previewEvent, previewFor, type PreviewHit } from "$lib/plugins";
  import { plugins } from "$lib/plugins.svelte";
  import { createZoomEase, pixelsPer, wheelFactor } from "$lib/zoom";
  import FileIcon from "./FileIcon.svelte";
  import FolderIcon from "./FolderIcon.svelte";
  import PdfViewer from "./PdfViewer.svelte";
  import PluginFrame from "./PluginFrame.svelte";

  let {
    entry,
    position,
    total,
    onopen,
    ontools,
    toolsLabel = "PDF tools…",
    onclose,
  }: {
    entry: Entry;
    /** 1-based place of the file among those in the folder. */
    position: number;
    total: number;
    /** "Open": hand the file to its default app. */
    onopen: () => void;
    /** The file's tools (PDF tools, image tools); given only for a file that has them. */
    ontools?: () => void;
    /** What the tools button says. */
    toolsLabel?: string;
    onclose: () => void;
  } = $props();

  type Shown =
    | { kind: "loading" }
    /** A plugin the person turned on draws this file in its own frame. */
    | { kind: "plugin"; hit: PreviewHit }
    | { kind: "url"; media: "image" | "pdf" | "audio" | "video"; url: string }
    | { kind: "text"; text: string; truncated: boolean }
    /** `cloud`: the file is only in the cloud, and opening it would download it. */
    | { kind: "none"; why: string; cloud?: boolean };

  let shown = $state<Shown>({ kind: "loading" });
  /** The file whose plugin preview the person chose to leave, to see it the usual way. */
  let skipped = $state("");
  let dialog: HTMLElement;

  // ---- A picture is shown whole, then zoomed and moved by pinch, scroll and drag ----
  const MAX_ZOOM = 16;
  const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

  let stageW = $state(0);
  let stageH = $state(0);
  /** The picture's own size, known once it has loaded. */
  let natW = $state(0);
  let natH = $state(0);
  /** 1 is "fit": the whole picture in the window, never enlarged past its own size. */
  let zoom = $state(1);
  /** Where the middle of the picture sits against the middle of the window. */
  let panX = $state(0);
  let panY = $state(0);

  const fit = $derived(
    natW && natH && stageW && stageH ? Math.min(stageW / natW, stageH / natH, 1) : 1,
  );
  const imgW = $derived(natW * fit);
  const imgH = $derived(natH * fit);

  const limit = (size: number, view: number) => Math.max(0, (size - view) / 2);

  /** Zooms to `next`, keeping the point under (cx, cy), measured from the window's middle. */
  function zoomTo(next: number, cx: number, cy: number) {
    const z = clamp(next, 1, MAX_ZOOM);
    const ratio = z / zoom;
    zoom = z;
    moveTo(cx - (cx - panX) * ratio, cy - (cy - panY) * ratio);
  }

  function moveTo(x: number, y: number) {
    const mx = limit(imgW * zoom, stageW);
    const my = limit(imgH * zoom, stageH);
    panX = clamp(x, -mx, mx);
    panY = clamp(y, -my, my);
  }

  function fromMiddle(stage: HTMLElement, x: number, y: number): [number, number] {
    const box = stage.getBoundingClientRect();
    return [x - (box.left + box.width / 2), y - (box.top + box.height / 2)];
  }

  /** Wheel, pinch and double-click ask for a zoom; it eases there over a few frames. */
  const ease = createZoomEase({
    get: () => zoom,
    min: 1,
    max: MAX_ZOOM,
    apply: zoomTo,
  });

  /** Full size, or twice the fit for a picture that already fills the window at full size. */
  const actualSize = () => (fit < 1 ? 1 / fit : 2);

  /** Pinch on a trackpad arrives as Ctrl+wheel; two-finger scrolling as plain wheel. Touch
   *  screens pinch with two pointers; one pointer drags. */
  function gestures(stage: HTMLElement) {
    // Plain bookkeeping of the pointers down, never rendered.
    // eslint-disable-next-line svelte/prefer-svelte-reactivity
    const pointers = new Map<number, [number, number]>();
    let pinch = 0;

    const onwheel = (e: WheelEvent) => {
      e.preventDefault();
      if (e.ctrlKey) {
        const [cx, cy] = fromMiddle(stage, e.clientX, e.clientY);
        // Fingers on the screen are pinching; a wheel event then must not pull against them.
        if (!pinch) ease.by(wheelFactor(e, stageH), cx, cy);
      } else {
        const k = pixelsPer(e, stageH);
        moveTo(panX - e.deltaX * k, panY - e.deltaY * k);
      }
    };

    const spread = () => {
      const [a, b] = [...pointers.values()];
      return Math.hypot(a[0] - b[0], a[1] - b[1]);
    };

    const ondown = (e: PointerEvent) => {
      pointers.set(e.pointerId, [e.clientX, e.clientY]);
      stage.setPointerCapture(e.pointerId);
      if (pointers.size === 2) {
        ease.stop();
        pinch = spread();
      }
    };

    const onmove = (e: PointerEvent) => {
      const last = pointers.get(e.pointerId);
      if (!last) return;
      const now: [number, number] = [e.clientX, e.clientY];
      pointers.set(e.pointerId, now);
      if (pointers.size === 2) {
        const d = spread();
        const [a, b] = [...pointers.values()];
        const [cx, cy] = fromMiddle(stage, (a[0] + b[0]) / 2, (a[1] + b[1]) / 2);
        if (pinch) zoomTo(zoom * (d / pinch), cx, cy);
        pinch = d;
      } else if (zoom > 1) {
        moveTo(panX + now[0] - last[0], panY + now[1] - last[1]);
      }
    };

    const onup = (e: PointerEvent) => {
      pointers.delete(e.pointerId);
      pinch = 0;
    };

    stage.addEventListener("wheel", onwheel, { passive: false });
    stage.addEventListener("pointerdown", ondown);
    stage.addEventListener("pointermove", onmove);
    stage.addEventListener("pointerup", onup);
    stage.addEventListener("pointercancel", onup);
    return {
      destroy() {
        stage.removeEventListener("wheel", onwheel);
        stage.removeEventListener("pointerdown", ondown);
        stage.removeEventListener("pointermove", onmove);
        stage.removeEventListener("pointerup", onup);
        stage.removeEventListener("pointercancel", onup);
      },
    };
  }

  function toggleZoom(e: MouseEvent) {
    const [cx, cy] = fromMiddle(e.currentTarget as HTMLElement, e.clientX, e.clientY);
    const still = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    ease.to(zoom > 1 ? 1 : actualSize(), cx, cy, still);
  }

  // Each picture starts whole.
  $effect(() => {
    void shown;
    ease.stop();
    natW = natH = 0;
    zoom = 1;
    panX = panY = 0;
  });

  // The keys belong to this window, not to whatever was focused behind it.
  onMount(() => {
    dialog.focus();
    return () => {
      ease.stop();
      releasePreview().catch(() => {});
    };
  });

  // Only what identifies the file reloads the preview: a refresh of the folder hands over a
  // new object for the same file, and must not restart playing audio.
  const path = $derived(entry.path);
  const name = $derived(entry.name);
  const isDir = $derived(entry.isDir);
  // A file edited in place keeps its path; its size or time say it is another file now.
  const version = $derived(`${entry.modified}-${entry.size}`);

  // Load what the current file needs; a newer file drops the answer to an older one.
  $effect(() => {
    const kind = previewKind(name, isDir);
    void version;
    let current = true;
    shown = { kind: "loading" };
    const hit = skipped === path ? null : previewFor(plugins.list, { path, name, isDir });
    if (hit) {
      shown = { kind: "plugin", hit };
    } else if (kind === "other") {
      shown = { kind: "none", why: isDir ? "Folder" : "No preview for this kind of file" };
    } else if (kind === "text") {
      readTextPreview(path)
        .then((t) => current && (shown = { kind: "text", ...t }))
        .catch(
          (e) =>
            current &&
            (shown =
              String(e) === CLOUD_ONLY
                ? { kind: "none", why: CLOUD_ONLY, cloud: true }
                : { kind: "none", why: "No preview for this kind of file" }),
        );
    } else {
      previewUrl(path)
        .then(
          (url) => current && (shown = { kind: "url", media: kind, url: `${url}?v=${version}` }),
        )
        .catch(
          (e) =>
            current && (shown = { kind: "none", why: String(e), cloud: String(e) === CLOUD_ONLY }),
        );
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
      {#if ontools}<button class="open" onclick={ontools}>{toolsLabel}</button>{/if}
      <button class="open" onclick={onopen}>Open</button>
      <button class="open" onclick={onclose}>Close</button>
    </header>

    <div
      class="stage"
      class:picture={shown.kind === "url" && shown.media === "image"}
      bind:clientWidth={stageW}
      bind:clientHeight={stageH}
      use:gestures
      ondblclick={shown.kind === "url" && shown.media === "image" ? toggleZoom : undefined}
      role="presentation"
    >
      {#if shown.kind === "loading"}
        <span class="note">Loading…</span>
      {:else if shown.kind === "url" && shown.media === "image"}
        <img
          src={shown.url}
          alt={entry.name}
          draggable="false"
          class:grab={zoom > 1}
          style:width={natW ? `${imgW}px` : undefined}
          style:height={natW ? `${imgH}px` : undefined}
          style:opacity={natW ? 1 : 0}
          style:transform="translate(-50%, -50%) translate({panX}px, {panY}px) scale({zoom})"
          onload={(e) => {
            const img = e.currentTarget as HTMLImageElement;
            natW = img.naturalWidth || stageW;
            natH = img.naturalHeight || stageH;
          }}
        />
      {:else if shown.kind === "url" && shown.media === "pdf"}
        <PdfViewer url={shown.url} name={entry.name} />
      {:else if shown.kind === "url" && shown.media === "audio"}
        <div class="audio">
          <FileIcon name={entry.name} size={96} />
          <audio src={shown.url} controls autoplay></audio>
        </div>
      {:else if shown.kind === "url"}
        <!-- Files in a folder come without captions. -->
        <!-- svelte-ignore a11y_media_has_caption -->
        <video src={shown.url} controls autoplay></video>
      {:else if shown.kind === "plugin"}
        {#key path}
          <PluginFrame
            plugin={shown.hit.plugin}
            onskip={() => (skipped = path)}
            hello={{
              event: previewEvent(shown.hit.preview.id, { path, name, isDir, size: entry.size }),
              paths: [path],
            }}
          />
        {/key}
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
          {#if shown.cloud}
            <!-- Looking never downloads; this does, because the person asked. -->
            <button class="open" onclick={onopen}>Download and open</button>
          {/if}
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

  video {
    max-width: 100%;
    max-height: 100%;
    object-fit: contain;
  }

  /* A picture is placed by the zoom and pan state; the stage clips it. */
  .stage.picture {
    position: relative;
    overflow: hidden;
    touch-action: none;
  }

  .picture img {
    position: absolute;
    top: 50%;
    left: 50%;
    max-width: none;
    max-height: none;
    user-select: none;
  }

  .picture img.grab {
    cursor: grab;
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
