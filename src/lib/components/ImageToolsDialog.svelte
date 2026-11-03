<script lang="ts">
  import { onMount } from "svelte";
  import {
    imageEdit,
    imageEstimate,
    previewUrl,
    type Entry,
    type ImageEdit,
    type ImagePreset,
  } from "$lib/api/fs";
  import {
    fitRatio,
    moveRect,
    RATIOS,
    ratioValue,
    resizeCorner,
    wholePixels,
    type Corner,
    type Ratio,
    type Rect,
  } from "$lib/crop";
  import { formatSize } from "$lib/format";
  import Icon from "./Icon.svelte";

  let {
    entry,
    onclose,
  }: {
    entry: Entry;
    /** Closed, whether or not a job was started. */
    onclose: () => void;
  } = $props();

  type Tab = "rotate" | "compress" | "crop";
  let tab = $state<Tab>("rotate");
  let url = $state<string | null>(null);
  /** The picture's size as it is shown (the browser applies its EXIF orientation). */
  let natural = $state({ width: 0, height: 0 });
  let save = $state<"copy" | "replace">("copy");
  let error = $state("");
  let busy = $state(false);

  // ---- Rotate
  /** Quarter turns clockwise waiting to be saved. */
  let turns = $state(0);
  const odd = $derived(Math.abs(turns) % 2 === 1);
  const STAGE_W = 440;
  const STAGE_H = 280;

  // ---- Compress
  const presets: { id: ImagePreset; label: string; text: string }[] = [
    { id: "high", label: "High quality", text: "Almost no visible change." },
    { id: "balanced", label: "Balanced", text: "Good for sharing." },
    { id: "small", label: "Small", text: "Three quarters of the size, lower quality." },
    { id: "smallest", label: "Smallest", text: "Half the size, low quality." },
  ];
  let preset = $state<ImagePreset>("balanced");
  /** What each preset would give: bytes, a reason it can't, or still being worked out. */
  const estimates = $state<Record<string, number | string | undefined>>({});

  // ---- Crop
  let ratio = $state<Ratio>("free");
  let rect = $state<Rect>({ x: 0, y: 0, width: 0, height: 0 });
  let frame = $state<HTMLElement | null>(null);
  let shownWidth = $state(0);
  const scale = $derived(natural.width ? shownWidth / natural.width : 1);
  const ratioNumber = $derived(ratioValue(ratio, natural.width, natural.height));
  const whole = $derived(wholePixels(rect, natural.width || 1, natural.height || 1));
  const cropped = $derived(
    natural.width > 0 &&
      (whole.width !== natural.width ||
        whole.height !== natural.height ||
        whole.x !== 0 ||
        whole.y !== 0),
  );

  onMount(() => {
    dialog.focus();
    previewUrl(entry.path).then(
      (u) => (url = u),
      (e) => (error = String(e)),
    );
    return () => {
      alive = false;
      opener?.focus?.();
    };
  });

  function loaded(e: Event) {
    const img = e.currentTarget as HTMLImageElement;
    natural = { width: img.naturalWidth, height: img.naturalHeight };
    rect = { x: 0, y: 0, width: img.naturalWidth, height: img.naturalHeight };
  }

  // The estimates are worked out when Compress is first shown, one after another: each is a
  // real encode, which is the only honest size.
  let estimating = false;
  let alive = true;
  $effect(() => {
    if (tab !== "compress" || estimating) return;
    estimating = true;
    (async () => {
      for (const p of presets) {
        if (!alive) return;
        try {
          estimates[p.id] = await imageEstimate(entry.path, p.id);
        } catch (e) {
          estimates[p.id] = String(e);
        }
      }
    })();
  });
  const chosenEstimate = $derived(estimates[preset]);
  const compressOk = $derived(typeof chosenEstimate === "number");

  const ready = $derived(
    !!url &&
      natural.width > 0 &&
      (tab === "rotate" ? turns % 4 !== 0 : tab === "compress" ? compressOk : cropped),
  );

  // ---- Crop interaction
  let drag: { mode: "move" | Corner; start: Rect; x0: number; y0: number } | null = null;

  function down(e: PointerEvent, mode: "move" | Corner) {
    if (e.button !== 0) return;
    e.preventDefault();
    e.stopPropagation();
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    drag = { mode, start: { ...rect }, x0: e.clientX, y0: e.clientY };
  }

  function move(e: PointerEvent) {
    if (!drag || !frame) return;
    if (drag.mode === "move") {
      rect = moveRect(
        drag.start,
        (e.clientX - drag.x0) / scale,
        (e.clientY - drag.y0) / scale,
        natural.width,
        natural.height,
      );
    } else {
      const box = frame.getBoundingClientRect();
      rect = resizeCorner(
        drag.start,
        drag.mode,
        (e.clientX - box.left) / scale,
        (e.clientY - box.top) / scale,
        ratioNumber,
        natural.width,
        natural.height,
      );
    }
  }

  function up() {
    drag = null;
  }

  function chooseRatio(next: Ratio) {
    ratio = next;
    const r = ratioValue(next, natural.width, natural.height);
    if (r) rect = fitRatio(rect, r, natural.width, natural.height);
  }

  /** A number typed in a field; the other side follows when the shape is fixed. */
  function typed(field: HTMLInputElement, key: keyof Rect) {
    const n = Number(field.value);
    if (field.value.trim() === "" || !Number.isFinite(n)) {
      field.value = String(whole[key]);
      return;
    }
    let next = { ...whole, [key]: n };
    if (ratioNumber && key === "width") next.height = n / ratioNumber;
    if (ratioNumber && key === "height") next.width = n * ratioNumber;
    next = wholePixels(next, natural.width, natural.height);
    // A fixed shape stays fixed: a box moved or sized past the edge is fitted back.
    if (ratioNumber)
      next = wholePixels(
        fitRatio(next, ratioNumber, natural.width, natural.height),
        natural.width,
        natural.height,
      );
    rect = next;
    // What was typed may have been clamped; show what applies.
    field.value = String(wholePixels(next, natural.width, natural.height)[key]);
  }

  async function start() {
    error = "";
    busy = true;
    const edit: ImageEdit =
      tab === "rotate"
        ? { kind: "rotate", turns: ((turns % 4) + 4) % 4 }
        : tab === "compress"
          ? { kind: "compress", preset }
          : { kind: "crop", ...whole };
    try {
      await imageEdit(entry.path, edit, save);
      onclose();
    } catch (e) {
      error = String(e);
      busy = false;
    }
  }

  let dialog: HTMLElement;
  /** Where focus was, so closing the dialog gives it back. */
  const opener = document.activeElement as HTMLElement | null;

  function onkeydown(e: KeyboardEvent) {
    // Nothing behind the dialog may act on keys while it is open.
    e.stopPropagation();
    if (e.key === "Escape") {
      e.preventDefault();
      onclose();
    }
    if (e.key === "Tab") {
      const items = [
        ...dialog.querySelectorAll<HTMLElement>(
          "button:not(:disabled), input:not(:disabled), select, [tabindex='0']",
        ),
      ];
      const at = document.activeElement;
      if (e.shiftKey && (at === items[0] || at === dialog)) {
        e.preventDefault();
        items[items.length - 1]?.focus();
      } else if (!e.shiftKey && at === items[items.length - 1]) {
        e.preventDefault();
        items[0]?.focus();
      }
    }
  }

  const corners: Corner[] = ["nw", "ne", "sw", "se"];
  // Arrow keys nudge the crop for people who do not drag.
  function nudge(e: KeyboardEvent) {
    const step = e.shiftKey ? 10 : 1;
    const by: Record<string, [number, number]> = {
      ArrowLeft: [-step, 0],
      ArrowRight: [step, 0],
      ArrowUp: [0, -step],
      ArrowDown: [0, step],
    };
    const d = by[e.key];
    if (!d) return;
    e.preventDefault();
    rect = moveRect(rect, d[0], d[1], natural.width, natural.height);
  }
</script>

<div class="backdrop" role="presentation">
  <div
    bind:this={dialog}
    class="dialog"
    role="dialog"
    aria-modal="true"
    aria-labelledby="image-title"
    tabindex="-1"
    {onkeydown}
  >
    <h2 id="image-title">{entry.name}</h2>

    <div class="tabs" role="group" aria-label="Tool">
      {#each ["rotate", "compress", "crop"] as const as t (t)}
        <button class:chosen={tab === t} aria-pressed={tab === t} onclick={() => (tab = t)}>
          {t === "rotate" ? "Rotate" : t === "compress" ? "Compress" : "Crop"}
        </button>
      {/each}
    </div>

    <div class="stage" style:width="{STAGE_W}px" style:height="{STAGE_H}px">
      {#if url}
        <!-- Cropping is drawn over the picture; the other tools show the whole of it. -->
        <div
          class="frame"
          class:crop={tab === "crop"}
          bind:this={frame}
          bind:clientWidth={shownWidth}
        >
          <img
            src={url}
            alt={entry.name}
            draggable="false"
            onload={loaded}
            style:max-width="{tab === 'rotate' && odd ? STAGE_H : STAGE_W}px"
            style:max-height="{tab === 'rotate' && odd ? STAGE_W : STAGE_H}px"
            style:transform={tab === "rotate" ? `rotate(${turns * 90}deg)` : undefined}
          />
          {#if tab === "crop" && natural.width}
            <!-- A draggable overlay: keyboard users move it with the arrow keys and size it in the fields. -->
            <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
            <div
              class="crop-box"
              role="group"
              aria-label="Crop area. Arrow keys move it; use the fields below to resize."
              tabindex="0"
              style:left="{rect.x * scale}px"
              style:top="{rect.y * scale}px"
              style:width="{rect.width * scale}px"
              style:height="{rect.height * scale}px"
              onpointerdown={(e) => down(e, "move")}
              onpointermove={move}
              onpointerup={up}
              onpointercancel={up}
              onkeydown={nudge}
            >
              {#each corners as corner (corner)}
                <span
                  class="handle {corner}"
                  role="presentation"
                  onpointerdown={(e) => down(e, corner)}
                  onpointermove={move}
                  onpointerup={up}
                ></span>
              {/each}
            </div>
          {/if}
        </div>
      {:else}
        <span class="note">Loading…</span>
      {/if}
    </div>

    {#if tab === "rotate"}
      <div class="row">
        <button onclick={() => (turns -= 1)}>
          <Icon name="chevronLeft" size={14} /> Rotate left
        </button>
        <button onclick={() => (turns += 1)}>
          Rotate right <Icon name="chevronRight" size={14} />
        </button>
      </div>
      <p class="note">
        A JPEG is turned without being re-saved when its details allow it. If they don't, it is
        saved again at high quality and its photo details are not kept.
      </p>
    {:else if tab === "compress"}
      <p class="note">
        Now {formatSize(entry.size)}.
        {#if !/\.jpe?g$/i.test(entry.name)}
          This kind of picture has no quality setting: only Small and Smallest make it smaller, by
          shrinking it.
        {/if}
      </p>
      <fieldset>
        <legend>Quality</legend>
        {#each presets as p (p.id)}
          {@const size = estimates[p.id]}
          <label class="preset">
            <input type="radio" name="preset" value={p.id} bind:group={preset} />
            <span>
              <strong>{p.label}</strong>
              {p.text}
              <span class="size" aria-live="polite">
                {#if size === undefined}
                  …
                {:else if typeof size === "number"}
                  about {formatSize(size)}
                {:else}
                  can't be made smaller
                {/if}
              </span>
            </span>
          </label>
        {/each}
      </fieldset>
      {#if typeof chosenEstimate === "string"}<p class="note">{chosenEstimate}</p>{/if}
    {:else}
      <div class="row">
        <label class="inline">
          Shape
          <select value={ratio} onchange={(e) => chooseRatio(e.currentTarget.value as Ratio)}>
            {#each RATIOS as r (r)}
              <option value={r}>{r === "free" ? "Free" : r === "original" ? "Original" : r}</option>
            {/each}
          </select>
        </label>
        {#each [["x", "Left"], ["y", "Top"], ["width", "Width"], ["height", "Height"]] as const as [key, label] (key)}
          <label class="inline">
            {label}
            <input
              class="number"
              type="number"
              min="0"
              value={whole[key]}
              onchange={(e) => typed(e.currentTarget, key)}
            />
          </label>
        {/each}
      </div>
      <p class="note">
        Drag the box or its corners. {whole.width} × {whole.height} px of {natural.width} ×
        {natural.height}.
      </p>
    {/if}

    <fieldset>
      <legend>Save</legend>
      <label class="preset">
        <input type="radio" name="save" value="copy" bind:group={save} />
        <span>As a copy next to the original</span>
      </label>
      <label class="preset">
        <input type="radio" name="save" value="replace" bind:group={save} />
        <span>
          Replace the original. It goes to the Recycle Bin and Undo brings it back. Photo details
          such as the date and camera may not be kept.
        </span>
      </label>
    </fieldset>

    {#if error}<p class="error" role="alert">{error}</p>{/if}

    <div class="actions">
      <button onclick={onclose}>Cancel</button>
      <button class="primary" disabled={busy || !ready} onclick={start}>Save</button>
    </div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 60;
    display: grid;
    place-items: center;
    background: var(--scrim);
  }

  .dialog {
    display: flex;
    flex-direction: column;
    gap: var(--space-md);
    width: min(520px, calc(100vw - 2 * var(--space-xl)));
    max-height: calc(100vh - 2 * var(--space-xl));
    overflow-y: auto;
    padding: var(--space-xl);
    border: 1px solid var(--border-default);
    border-radius: var(--radius-xl);
    background: var(--bg-window);
    box-shadow: var(--shadow-popover);
    outline: none;
  }

  h2 {
    margin: 0;
    font-size: var(--font-size-title);
    font-weight: var(--font-weight-semibold);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .tabs,
  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-end;
    gap: var(--space-sm);
  }

  .tabs button.chosen {
    background: var(--accent-subtle);
    color: var(--accent-default);
    font-weight: var(--font-weight-medium);
  }

  .stage {
    max-width: 100%;
    display: grid;
    grid-template: minmax(0, 1fr) / minmax(0, 1fr);
    place-items: center;
    align-self: center;
    overflow: hidden;
    border-radius: var(--radius-md);
    background: var(--bg-subtle);
  }

  .frame {
    position: relative;
    display: inline-block;
    line-height: 0;
  }

  img {
    display: block;
    object-fit: contain;
    transition: transform var(--duration-fast) var(--ease-standard);
  }

  .crop-box {
    position: absolute;
    box-sizing: border-box;
    border: 2px solid var(--accent-default);
    /* Dims everything outside the chosen area. */
    box-shadow: 0 0 0 9999px var(--scrim);
    cursor: move;
    touch-action: none;
    outline-offset: 2px;
  }

  .handle {
    position: absolute;
    width: 14px;
    height: 14px;
    border-radius: var(--radius-xs);
    background: var(--accent-default);
    touch-action: none;
  }

  .handle.nw {
    top: -8px;
    left: -8px;
    cursor: nwse-resize;
  }

  .handle.ne {
    top: -8px;
    right: -8px;
    cursor: nesw-resize;
  }

  .handle.sw {
    bottom: -8px;
    left: -8px;
    cursor: nesw-resize;
  }

  .handle.se {
    right: -8px;
    bottom: -8px;
    cursor: nwse-resize;
  }

  .note {
    margin: 0;
    color: var(--text-secondary);
  }

  .size {
    color: var(--text-secondary);
  }

  .inline {
    display: flex;
    flex-direction: column;
    gap: var(--space-2xs);
    color: var(--text-secondary);
  }

  select,
  .number {
    padding: var(--space-xs) var(--space-sm);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-md);
    background: var(--bg-window);
    color: var(--text-primary);
    font: inherit;
  }

  .number {
    width: 76px;
    user-select: text;
  }

  fieldset {
    margin: 0;
    padding: 0;
    border: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-xs);
  }

  legend {
    padding: 0;
    margin-bottom: var(--space-xs);
    color: var(--text-secondary);
  }

  .preset {
    display: flex;
    align-items: baseline;
    gap: var(--space-sm);
  }

  .error {
    margin: 0;
    padding-left: var(--space-sm);
    border-left: 3px solid var(--bg-danger);
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-sm);
  }

  button {
    display: inline-flex;
    align-items: center;
    gap: var(--space-xs);
    padding: var(--space-xs) var(--space-md);
    border-radius: var(--radius-md);
    background: var(--bg-control);
  }

  button:disabled {
    color: var(--text-tertiary);
  }

  .primary {
    background: var(--accent-default);
    color: var(--text-on-accent);
    font-weight: var(--font-weight-medium);
  }

  .primary:disabled {
    color: var(--text-on-accent);
    opacity: 0.6;
  }
</style>
