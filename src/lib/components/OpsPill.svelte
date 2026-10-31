<script lang="ts">
  import Icon from "./Icon.svelte";
  import type { Job } from "$lib/api/fs";

  let {
    jobs,
    onpause,
    onresume,
    oncancel,
    ondismiss,
  }: {
    jobs: Job[];
    onpause: (id: number) => void;
    onresume: (id: number) => void;
    oncancel: (id: number) => void;
    ondismiss: (id: number) => void;
  } = $props();

  let open = $state(false);

  const active = (j: Job) => j.state === "queued" || j.state === "running" || j.state === "paused";
  let working = $derived(jobs.filter(active));
  let current = $derived(working.find((j) => j.state !== "queued") ?? working[0]);

  const percent = (j: Job) =>
    j.bytesTotal > 0
      ? Math.min(100, Math.round((j.bytesDone / j.bytesTotal) * 100))
      : j.itemsTotal > 0
        ? Math.min(100, Math.round((j.itemsDone / j.itemsTotal) * 100))
        : 0;

  function size(bytes: number): string {
    const units = ["B", "KB", "MB", "GB", "TB"];
    let n = bytes;
    let i = 0;
    while (n >= 1024 && i < units.length - 1) {
      n /= 1024;
      i++;
    }
    return `${n >= 10 || i === 0 ? Math.round(n) : n.toFixed(1)} ${units[i]}`;
  }

  function time(secs: number): string {
    if (secs < 60) return `${Math.max(1, Math.round(secs))} s`;
    const m = Math.floor(secs / 60);
    return m < 60 ? `${m} min ${Math.round(secs % 60)} s` : `${Math.floor(m / 60)} h ${m % 60} min`;
  }

  /** "42% · 12 MB/s · 8 s left" for a job that moves bytes; plain count otherwise. */
  function detail(j: Job): string {
    if (j.state === "queued") return "Waiting";
    if (j.state === "paused") return `Paused · ${percent(j)}%`;
    if (j.bytesTotal === 0) return `${j.itemsDone} of ${j.itemsTotal}`;
    const parts = [`${percent(j)}%`];
    if (j.speed > 0) parts.push(`${size(j.speed)}/s`);
    if (j.etaSecs != null) parts.push(`${time(j.etaSecs)} left`);
    return parts.join(" · ");
  }

  function outcome(j: Job): string {
    if (j.state === "done") return "Done";
    if (j.state === "cancelled") return "Cancelled";
    return j.error ?? "Failed";
  }

  // The panel closes by itself once there is nothing to show.
  $effect(() => {
    if (!jobs.length) open = false;
  });
</script>

{#if jobs.length}
  <div class="ops">
    <button
      class="pill"
      class:busy={!!current}
      onclick={() => (open = !open)}
      aria-expanded={open}
      aria-label="Operations"
    >
      {#if current}
        <span class="meter" style:--p="{percent(current)}%"></span>
        <span class="text">{current.label}</span>
        <span class="num">{detail(current)}</span>
        {#if working.length > 1}<span class="more">+{working.length - 1}</span>{/if}
      {:else}
        <span class="text">Operations</span>
        <span class="num">{jobs.length}</span>
      {/if}
    </button>

    {#if open}
      <div class="panel" role="dialog" aria-label="Operations">
        {#each jobs as job (job.id)}
          <div class="row" class:failed={job.state === "failed"}>
            <div class="main">
              <span class="label">{job.label}</span>
              {#if active(job)}
                <span class="detail">{detail(job)}</span>
              {:else}
                <span class="detail">{outcome(job)}</span>
              {/if}
              {#if active(job)}
                <div class="bar"><div class="fill" style:width="{percent(job)}%"></div></div>
              {/if}
            </div>
            <div class="actions">
              {#if job.state === "running" && job.kind !== "trash"}
                <button title="Pause" aria-label="Pause" onclick={() => onpause(job.id)}>
                  <Icon name="pause" size={14} stroke={2} />
                </button>
              {:else if job.state === "paused"}
                <button title="Resume" aria-label="Resume" onclick={() => onresume(job.id)}>
                  <Icon name="play" size={14} stroke={2} />
                </button>
              {/if}
              {#if active(job) && (job.kind !== "trash" || job.state === "queued")}
                <button title="Cancel" aria-label="Cancel" onclick={() => oncancel(job.id)}>
                  <Icon name="x" size={14} stroke={2} />
                </button>
              {:else if !active(job)}
                <button title="Dismiss" aria-label="Dismiss" onclick={() => ondismiss(job.id)}>
                  <Icon name="x" size={14} stroke={2} />
                </button>
              {/if}
            </div>
          </div>
        {/each}
      </div>
    {/if}
  </div>
{/if}

<style>
  .ops {
    position: relative;
    margin-left: auto;
  }

  .pill {
    display: flex;
    align-items: center;
    gap: var(--space-sm);
    max-width: 420px;
    padding: var(--space-2xs) var(--space-sm);
    border-radius: var(--radius-full);
    background: var(--bg-control);
    color: var(--text-secondary);
    font-size: var(--font-size-caption);
  }
  .pill:hover {
    background: var(--bg-hover);
  }

  .meter {
    flex: none;
    width: 28px;
    height: 4px;
    border-radius: var(--radius-full);
    background: linear-gradient(
      to right,
      var(--accent-default) var(--p),
      var(--border-strong) var(--p)
    );
  }

  .text {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--text-primary);
  }

  .num,
  .more {
    flex: none;
    font-variant-numeric: tabular-nums;
  }

  .panel {
    position: absolute;
    right: 0;
    bottom: calc(100% + var(--space-sm));
    width: 360px;
    max-height: 320px;
    overflow-y: auto;
    padding: var(--space-xs);
    border: 1px solid var(--border-default);
    border-radius: var(--radius-lg);
    background: var(--bg-window);
    box-shadow: var(--shadow-popover);
  }

  .row {
    display: flex;
    align-items: center;
    gap: var(--space-sm);
    padding: var(--space-sm);
    border-radius: var(--radius-md);
  }
  .row:hover {
    background: var(--bg-hover);
  }

  .main {
    flex: 1;
    min-width: 0;
    display: grid;
    gap: var(--space-2xs);
  }

  .label {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--text-primary);
    font-size: var(--font-size-body);
    font-weight: var(--font-weight-medium);
  }

  .detail {
    color: var(--text-secondary);
    font-size: var(--font-size-caption);
    font-variant-numeric: tabular-nums;
  }
  .failed .detail {
    color: var(--status-danger);
  }

  .bar {
    height: 4px;
    border-radius: var(--radius-full);
    background: var(--bg-control);
    overflow: hidden;
  }
  .fill {
    height: 100%;
    background: var(--accent-default);
  }

  .actions {
    display: flex;
    gap: var(--space-2xs);
  }
  .actions button {
    display: grid;
    place-items: center;
    width: 24px;
    height: 24px;
    border-radius: var(--radius-sm);
    color: var(--text-secondary);
  }
  .actions button:hover {
    background: var(--bg-control);
    color: var(--text-primary);
  }
</style>
