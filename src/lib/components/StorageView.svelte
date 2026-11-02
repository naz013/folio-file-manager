<script lang="ts">
  import { onMount } from "svelte";
  import {
    SCAN_CATEGORIES,
    listDrives,
    onScanDone,
    onScanProgress,
    scanCancel,
    scanCategories,
    scanChildren,
    scanRoots,
    scanSize,
    scanStart,
    type Drive,
    type SizedDir,
  } from "$lib/api/fs";
  import { formatSize } from "$lib/format";
  import { squarify } from "$lib/treemap";

  let {
    start,
    onopen,
  }: {
    /** The folder the browser last showed; its drive is picked first. */
    start: string;
    /** Show this folder in the browser. */
    onopen: (path: string) => void;
  } = $props();

  /** One colour per category, from the tag colours. */
  const COLORS = [
    "var(--tag-blue)",
    "var(--tag-green)",
    "var(--tag-purple)",
    "var(--tag-orange)",
    "var(--tag-yellow)",
    "var(--tag-red)",
    "var(--tag-gray)",
  ];

  let dialog: HTMLElement;

  let drives = $state<Drive[]>([]);
  /** The drive (or scanned folder) the breakdown is for. */
  let root = $state("");
  /** The folder drilled into, at or below `root`. */
  let dir = $state("");
  let scanned = $state<string[]>([]);
  let children = $state<SizedDir[]>([]);
  let cats = $state<number[]>([]);
  let total = $state<number | null>(null);
  let scanning = $state(false);
  let seen = $state(0);
  let error = $state("");

  const norm = (p: string) => p.toLowerCase().replace(/\//g, "\\").replace(/\\+$/, "");
  /** Scanned folders that are not a whole drive, so they can be looked at too. */
  const folders = $derived(scanned.filter((r) => !drives.some((d) => norm(d.path) === norm(r))));
  const isScanned = $derived(scanned.some((r) => norm(r) === norm(root)));

  /** What the folder's own files weigh: its total less its subfolders. */
  const ownBytes = $derived(
    Math.max(0, (total ?? 0) - children.reduce((sum, c) => sum + c.bytes, 0)),
  );

  /** How many folders get a block of their own; the rest are folded into one. */
  const MAX_BLOCKS = 60;

  type Block = { name: string; path: string | null; bytes: number };
  const blocks = $derived.by(() => {
    const folders: Block[] = children
      .filter((c) => c.bytes > 0)
      .map((c) => ({ name: c.name, path: c.path, bytes: c.bytes }))
      .sort((a, b) => b.bytes - a.bytes);
    const list = folders.slice(0, MAX_BLOCKS);
    const rest = folders.slice(MAX_BLOCKS);
    if (rest.length) {
      const bytes = rest.reduce((sum, b) => sum + b.bytes, 0);
      list.push({ name: `${rest.length} smaller folders`, path: null, bytes });
    }
    if (ownBytes > 0) list.push({ name: "Files in this folder", path: null, bytes: ownBytes });
    return list.sort((a, b) => b.bytes - a.bytes);
  });

  let mapWidth = $state(0);
  let mapHeight = $state(0);
  const rects = $derived(
    squarify(
      blocks.map((b) => b.bytes),
      mapWidth,
      mapHeight,
    ),
  );

  const catTotal = $derived(cats.reduce((a, b) => a + b, 0));
  const RADIUS = 52;
  const CIRCUMFERENCE = 2 * Math.PI * RADIUS;
  /** The donut's arcs: each category's share of the circle, laid end to end. */
  const arcs = $derived.by(() => {
    let offset = 0;
    return cats.map((bytes, i) => {
      const length = catTotal ? (bytes / catTotal) * CIRCUMFERENCE : 0;
      const arc = { i, length, offset };
      offset += length;
      return arc;
    });
  });

  /** The folders from the root down to the one shown, for the breadcrumb. */
  const trail = $derived.by(() => {
    const parts: { name: string; path: string }[] = [{ name: root, path: root }];
    if (!root || norm(dir) === norm(root)) return parts;
    const rest = dir.slice(root.length).split(/[\\/]/).filter(Boolean);
    let at = root.replace(/[\\/]+$/, "");
    for (const name of rest) {
      at += "\\" + name;
      parts.push({ name, path: at });
    }
    return parts;
  });

  /** Each load takes a number, so a slow one for a folder left behind can't land on top. */
  let loadId = 0;

  async function load(target: string) {
    const id = ++loadId;
    error = "";
    try {
      const [kids, c, size] = await Promise.all([
        scanChildren(target),
        scanCategories(target),
        scanSize(target),
      ]);
      if (id !== loadId) return;
      children = kids;
      cats = c;
      total = size ? size[0] : null;
    } catch (e) {
      if (id === loadId) error = String(e);
    }
  }

  function pick(path: string) {
    root = path;
    dir = path;
    children = [];
    cats = [];
    total = null;
    load(path);
  }

  function drill(path: string) {
    dir = path;
    load(path);
  }

  async function refreshScanned() {
    scanned = (await scanRoots().catch(() => [])).map((r) => r.root);
  }

  async function rescan() {
    scanning = true;
    seen = 0;
    try {
      await scanStart(root);
    } catch (e) {
      scanning = false;
      error = String(e);
    }
  }

  const sharePct = (bytes: number, of: number) => (of ? Math.round((bytes / of) * 100) : 0);

  onMount(() => {
    dialog.focus();
    const unlistenProgress = onScanProgress((p) => (seen = p.files));
    const unlistenDone = onScanDone(async (d) => {
      scanning = false;
      if (d.error) error = d.error;
      await refreshScanned();
      if (norm(d.root) === norm(root)) load(dir);
    });
    (async () => {
      [drives] = await Promise.all([listDrives().catch(() => []), refreshScanned()]);
      // A scanned folder holding the current one is the closest; else the drive it is on.
      const inside = scanned.find((r) => norm(start).startsWith(norm(r)));
      const home = drives.find((d) => norm(start).startsWith(norm(d.path)));
      pick(inside ?? home?.path ?? drives[0]?.path ?? start);
    })();
    return () => {
      unlistenProgress.then((u) => u());
      unlistenDone.then((u) => u());
    };
  });
</script>

<section bind:this={dialog} class="storage" aria-label="Storage" tabindex="-1">
  <header>
    <select
      aria-label="Drive"
      value={root}
      onchange={(e) => pick(e.currentTarget.value)}
      disabled={!drives.length}
    >
      {#each drives as d (d.path)}
        <option value={d.path}>
          {d.path} · {formatSize(d.total - d.free)} used of {formatSize(d.total)}
        </option>
      {/each}
      {#each folders as f (f)}
        <option value={f}>{f}</option>
      {/each}
    </select>
    <span class="grow"></span>
    {#if scanning}
      <span class="note" role="status">Scanning… {seen.toLocaleString()} files</span>
      <button onclick={() => scanCancel()}>Cancel</button>
    {:else}
      <button class="primary" onclick={rescan} disabled={!root}>
        {isScanned ? "Rescan" : "Scan this drive"}
      </button>
    {/if}
  </header>

  {#if error}
    <p class="error" role="alert">{error}</p>
  {/if}

  {#if !isScanned && !scanning}
    <p class="empty">
      This drive hasn't been scanned yet. Scanning reads file names and sizes only, and runs in the
      background.
    </p>
  {:else}
    <nav class="crumbs" aria-label="Folder">
      {#each trail as part, i (part.path)}
        {#if i}<span aria-hidden="true">›</span>{/if}
        <button onclick={() => drill(part.path)} aria-current={i === trail.length - 1}>
          {part.name}
        </button>
      {/each}
      <span class="grow"></span>
      <button onclick={() => onopen(dir)}>Open in Folio</button>
    </nav>

    <div class="body">
      <section class="donut" aria-label="By category">
        <svg viewBox="0 0 140 140" role="img" aria-label="Space by category">
          <g transform="rotate(-90 70 70)">
            <circle class="track" cx="70" cy="70" r={RADIUS} />
            {#each arcs as a (a.i)}
              {#if a.length > 0}
                <circle
                  cx="70"
                  cy="70"
                  r={RADIUS}
                  stroke={COLORS[a.i]}
                  stroke-dasharray="{a.length} {CIRCUMFERENCE - a.length}"
                  stroke-dashoffset={-a.offset}
                />
              {/if}
            {/each}
          </g>
          <text x="70" y="68" text-anchor="middle" class="big">{formatSize(total ?? 0)}</text>
          <text x="70" y="84" text-anchor="middle" class="small">in this folder</text>
        </svg>
        <ul>
          {#each cats as bytes, i (i)}
            <li>
              <i style:background={COLORS[i]}></i>
              <span class="label">{SCAN_CATEGORIES[i]}</span>
              <span class="muted">{formatSize(bytes)} · {sharePct(bytes, catTotal)}%</span>
            </li>
          {/each}
        </ul>
      </section>

      <section class="map" aria-label="Largest folders">
        <div class="canvas" bind:clientWidth={mapWidth} bind:clientHeight={mapHeight}>
          {#each blocks as b, i (b.name + i)}
            {@const r = rects[i]}
            {#if r && r.w >= 4 && r.h >= 4}
              <button
                class="block"
                class:files={b.path === null}
                style:left="{r.x}px"
                style:top="{r.y}px"
                style:width="{r.w}px"
                style:height="{r.h}px"
                title="{b.name}: {formatSize(b.bytes)}"
                disabled={b.path === null}
                onclick={() => b.path && drill(b.path)}
              >
                {#if r.w > 70 && r.h > 34}
                  <span class="bname">{b.name}</span>
                  <span class="bsize">{formatSize(b.bytes)}</span>
                {/if}
              </button>
            {/if}
          {:else}
            <p class="empty">Nothing here.</p>
          {/each}
        </div>
      </section>
    </div>
  {/if}
</section>

<style>
  .storage {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-md);
    padding: var(--space-lg);
    background: var(--bg-window);
    outline: none;
  }

  header,
  .crumbs {
    display: flex;
    align-items: center;
    gap: var(--space-sm);
  }

  .grow {
    flex: 1;
  }

  header button,
  .crumbs button,
  select {
    padding: var(--space-xs) var(--space-md);
    border: 1px solid var(--border-default);
    border-radius: var(--radius-md);
    background: var(--bg-control);
    color: var(--text-primary);
    font: inherit;
  }

  .primary {
    border-color: transparent;
    background: var(--accent-default);
    color: var(--text-on-accent);
    font-weight: var(--font-weight-medium);
  }

  button:disabled {
    color: var(--text-tertiary);
  }

  .note,
  .muted {
    color: var(--text-secondary);
    font-size: var(--font-size-caption);
  }

  .error {
    margin: 0;
    color: var(--status-danger);
  }

  .empty {
    margin: var(--space-xl) 0;
    color: var(--text-secondary);
  }

  .crumbs button {
    padding: var(--space-2xs) var(--space-sm);
    border-color: transparent;
    background: none;
  }

  .crumbs button[aria-current="true"] {
    font-weight: var(--font-weight-semibold);
  }

  .body {
    display: grid;
    grid-template-columns: 260px 1fr;
    gap: var(--space-xl);
    flex: 1;
    min-height: 0;
  }

  .donut svg {
    width: 100%;
    max-width: 220px;
  }

  /* SVG units: the ring is 16 of the 140-unit viewBox, and the text sizes are in the same units. */
  circle {
    fill: none;
    stroke-width: 16;
  }

  circle.track {
    stroke: var(--bg-control);
  }

  .big {
    fill: var(--text-primary);
    font-size: 14px;
    font-weight: var(--font-weight-semibold);
  }

  .small {
    fill: var(--text-secondary);
    font-size: 8px;
  }

  ul {
    display: grid;
    gap: var(--space-xs);
    margin: var(--space-md) 0 0;
    padding: 0;
    list-style: none;
  }

  li {
    display: flex;
    align-items: center;
    gap: var(--space-sm);
  }

  li i {
    width: 10px;
    height: 10px;
    border-radius: var(--radius-xs);
  }

  .label {
    flex: 1;
  }

  .map {
    min-width: 0;
    min-height: 0;
  }

  .canvas {
    position: relative;
    height: 100%;
    min-height: 200px;
  }

  .block {
    position: absolute;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    justify-content: flex-start;
    padding: var(--space-sm);
    overflow: hidden;
    border: 2px solid var(--bg-window);
    border-radius: var(--radius-md);
    background: var(--accent-subtle);
    color: var(--text-primary);
    text-align: left;
  }

  .block:focus-visible {
    outline: 2px solid var(--accent-default);
  }

  .block:hover:not(:disabled) {
    background: var(--bg-selected);
    outline: 2px solid var(--accent-default);
  }

  .block.files {
    background: var(--bg-control);
  }

  .bname {
    max-width: 100%;
    overflow: hidden;
    font-weight: var(--font-weight-medium);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .bsize {
    color: var(--text-secondary);
    font-size: var(--font-size-caption);
  }
</style>
