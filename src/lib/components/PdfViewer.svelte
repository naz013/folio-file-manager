<script lang="ts">
  import { flushSync, tick, untrack } from "svelte";
  import { SvelteSet } from "svelte/reactivity";
  import * as pdfjs from "pdfjs-dist";
  import type { PDFDocumentProxy, RenderTask } from "pdfjs-dist";
  import workerUrl from "pdfjs-dist/build/pdf.worker.min.mjs?url";
  import { createZoomEase, wheelFactor } from "$lib/zoom";

  pdfjs.GlobalWorkerOptions.workerSrc = workerUrl;

  let { url, name }: { url: string; name: string } = $props();

  const PAD = 16;
  const MIN_ZOOM = 0.25;
  const MAX_ZOOM = 8;
  const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

  let scroller = $state<HTMLElement>();
  let viewW = $state(0);
  let viewH = $state(0);
  let doc = $state.raw<PDFDocumentProxy | null>(null);
  /** Each page's size at scale 1, in CSS pixels. */
  let sizes = $state.raw<[number, number][]>([]);
  let error = $state("");
  /** 1 is "fit width"; the live value follows the gesture. */
  let zoom = $state(1);
  /** The zoom the pages were last drawn for: sharp pages wait for the gesture to settle. */
  let settled = $state(1);
  const visible = new SvelteSet<number>();
  const canvases: HTMLCanvasElement[] = [];
  const pageEls: HTMLElement[] = [];

  const widest = $derived(sizes.reduce((w, [pw]) => Math.max(w, pw), 0));
  const fit = $derived(widest && viewW ? Math.max(viewW - 2 * PAD, 1) / widest : 1);
  const drawScale = $derived(fit * settled);

  // Plain bookkeeping of what is drawn and drawing, never rendered.
  // eslint-disable-next-line svelte/prefer-svelte-reactivity
  const drawn = new Map<number, number>();
  // eslint-disable-next-line svelte/prefer-svelte-reactivity
  const drawing = new Map<number, RenderTask>();

  // Open the file; a newer file drops the answer to an older one.
  $effect(() => {
    const source = url;
    let current = true;
    untrack(() => {
      doc = null;
      sizes = [];
      error = "";
      ease.stop();
      zoom = settled = 1;
      visible.clear();
      drawn.clear();
    });
    // The whole file is read at once: the preview protocol serves one file to one viewer.
    const task = pdfjs.getDocument({ url: source, disableRange: true, disableStream: true });
    task.promise
      .then(async (d) => {
        if (!current) return;
        const first = await d.getPage(1);
        const v = first.getViewport({ scale: 1 });
        if (!current) return;
        sizes = Array.from({ length: d.numPages }, () => [v.width, v.height]);
        doc = d;
        const all: [number, number][] = [[v.width, v.height]];
        for (let n = 2; n <= d.numPages && current; n++) {
          const size = (await d.getPage(n)).getViewport({ scale: 1 });
          all.push([size.width, size.height]);
        }
        if (current) sizes = all;
      })
      .catch((e) => current && (error = e instanceof Error ? e.message : String(e)));
    return () => {
      current = false;
      for (const t of drawing.values()) t.cancel();
      drawing.clear();
      task.destroy();
    };
  });

  // Draw what is near the window, again when the zoom has settled.
  $effect(() => {
    const d = doc;
    const scale = drawScale;
    if (!d) return;
    for (const i of visible) {
      if (drawn.get(i) !== scale) void draw(d, i, scale);
    }
  });

  async function draw(d: PDFDocumentProxy, i: number, scale: number) {
    drawing.get(i)?.cancel();
    drawn.set(i, scale);
    const page = await d.getPage(i + 1);
    const dpr = window.devicePixelRatio || 1;
    const viewport = page.getViewport({ scale: scale * dpr });
    // Drawn off screen and then copied over, so the page never flashes blank.
    const off = document.createElement("canvas");
    off.width = Math.ceil(viewport.width);
    off.height = Math.ceil(viewport.height);
    const task = page.render({ canvas: off, viewport });
    drawing.set(i, task);
    try {
      await task.promise;
    } catch {
      // Cancelled by a newer draw, or the file went away.
      if (drawn.get(i) === scale) drawn.delete(i);
      return;
    }
    const canvas = canvases[i];
    if (!canvas || d !== doc) return;
    canvas.width = off.width;
    canvas.height = off.height;
    canvas.getContext("2d")?.drawImage(off, 0, 0);
  }

  // The zoom is drawn sharp once it stops changing.
  $effect(() => {
    const z = zoom;
    const timer = setTimeout(() => (settled = z), 150);
    return () => clearTimeout(timer);
  });

  /** The pages near the window are drawn; the rest are left empty. */
  function updateVisible() {
    const el = scroller;
    if (!el) return;
    const box = el.getBoundingClientRect();
    const near = 800;
    pageEls.forEach((page, i) => {
      const r = page.getBoundingClientRect();
      if (r.bottom > box.top - near && r.top < box.bottom + near) visible.add(i);
      else visible.delete(i);
    });
  }

  // New pages, a new size or another zoom move the pages under the window.
  $effect(() => {
    void [sizes, fit, zoom, viewH];
    tick().then(updateVisible);
  });

  /** Zooms to `next`, keeping the point under (px, py), measured from the window's corner. */
  function zoomTo(next: number, px: number, py: number) {
    const el = scroller;
    if (!el) return;
    const z = clamp(next, MIN_ZOOM, MAX_ZOOM);
    const ratio = z / zoom;
    const sx = el.scrollLeft + px;
    const sy = el.scrollTop + py;
    flushSync(() => (zoom = z));
    el.scrollLeft = sx * ratio - px;
    el.scrollTop = sy * ratio - py;
  }

  /** Wheel and pinch ask for a zoom; it eases there over a few frames. */
  const ease = createZoomEase({
    get: () => zoom,
    min: MIN_ZOOM,
    max: MAX_ZOOM,
    apply: zoomTo,
  });

  const at = (el: HTMLElement, x: number, y: number): [number, number] => {
    const box = el.getBoundingClientRect();
    return [x - box.left, y - box.top];
  };

  /** Pinch on a trackpad arrives as Ctrl+wheel; plain scrolling is left to the page. A touch
   *  screen pinches with two fingers. */
  function gestures(el: HTMLElement) {
    let pinch = 0;

    const onwheel = (e: WheelEvent) => {
      if (!e.ctrlKey) return;
      e.preventDefault();
      const [px, py] = at(el, e.clientX, e.clientY);
      if (!pinch) ease.by(wheelFactor(e, viewH), px, py);
    };

    const spread = (e: TouchEvent) =>
      Math.hypot(
        e.touches[0].clientX - e.touches[1].clientX,
        e.touches[0].clientY - e.touches[1].clientY,
      );

    const ontouchstart = (e: TouchEvent) => {
      pinch = e.touches.length === 2 ? spread(e) : 0;
      if (pinch) ease.stop();
    };

    const ontouchmove = (e: TouchEvent) => {
      if (e.touches.length !== 2 || !pinch) return;
      e.preventDefault();
      const d = spread(e);
      const [px, py] = at(
        el,
        (e.touches[0].clientX + e.touches[1].clientX) / 2,
        (e.touches[0].clientY + e.touches[1].clientY) / 2,
      );
      zoomTo(zoom * (d / pinch), px, py);
      pinch = d;
    };

    const onend = () => (pinch = 0);

    el.addEventListener("wheel", onwheel, { passive: false });
    el.addEventListener("touchstart", ontouchstart, { passive: true });
    el.addEventListener("touchmove", ontouchmove, { passive: false });
    el.addEventListener("touchend", onend);
    el.addEventListener("touchcancel", onend);
    return {
      destroy() {
        ease.stop();
        el.removeEventListener("wheel", onwheel);
        el.removeEventListener("touchstart", ontouchstart);
        el.removeEventListener("touchmove", ontouchmove);
        el.removeEventListener("touchend", onend);
        el.removeEventListener("touchcancel", onend);
      },
    };
  }

  function toggleZoom(e: MouseEvent) {
    const [px, py] = at(e.currentTarget as HTMLElement, e.clientX, e.clientY);
    ease.stop();
    zoomTo(zoom > 1 ? 1 : 2, px, py);
  }
</script>

<div
  bind:this={scroller}
  bind:clientWidth={viewW}
  bind:clientHeight={viewH}
  onscroll={updateVisible}
  class="scroller"
  role="document"
  aria-label={name}
  use:gestures
  ondblclick={toggleZoom}
>
  {#if error}
    <span class="note">Can’t show this PDF: {error}</span>
  {:else if !doc}
    <span class="note">Loading…</span>
  {:else}
    <div class="pages">
      {#each sizes as [w, h], i (i)}
        <div
          class="page"
          style:width="{w * fit * zoom}px"
          style:height="{h * fit * zoom}px"
          bind:this={pageEls[i]}
        >
          <canvas bind:this={canvases[i]} aria-label="Page {i + 1}"></canvas>
        </div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .scroller {
    width: 100%;
    height: 100%;
    overflow: auto;
    /* One finger scrolls; the pinch is ours. */
    touch-action: pan-x pan-y;
  }

  .note {
    display: grid;
    place-items: center;
    height: 100%;
    color: var(--text-secondary);
    font-size: var(--font-size-caption);
  }

  .pages {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--space-md);
    /* Wider than the window once zoomed in: the page can then be scrolled to its edges. */
    width: max-content;
    min-width: 100%;
    padding: var(--space-lg);
    box-sizing: border-box;
  }

  .page {
    flex-shrink: 0;
    background: #fff;
    box-shadow: var(--shadow-raised);
  }

  canvas {
    display: block;
    width: 100%;
    height: 100%;
  }
</style>
