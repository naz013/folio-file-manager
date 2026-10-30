// Opt-in benchmark helpers driven by scripts/perf/run.ps1 (see docs/PERFORMANCE.md).

import { invoke } from "@tauri-apps/api/core";
import { tick } from "svelte";

export interface PerfConfig {
  enabled: boolean;
  benchDirs: string[];
}

export const perfConfig = () => invoke<PerfConfig>("perf_config");
export const perfReport = (event: string) => invoke<void>("perf_report", { event });

/** Resolves after Svelte has flushed and the browser has painted the result. */
export async function nextPaint(): Promise<void> {
  await tick();
  await new Promise<void>((r) => requestAnimationFrame(() => requestAnimationFrame(() => r())));
}

export interface ScrollStats {
  frames: number;
  avgFps: number;
  p95Ms: number;
  maxMs: number;
  /** Frames longer than 25 ms (visibly janky at 60 Hz). */
  slow: number;
}

function stats(deltas: number[]): ScrollStats {
  const sorted = [...deltas].sort((a, b) => a - b);
  const mean = deltas.reduce((a, b) => a + b, 0) / deltas.length;
  return {
    frames: deltas.length,
    avgFps: Math.round(1000 / mean),
    p95Ms: +sorted[Math.floor(sorted.length * 0.95)].toFixed(1),
    maxMs: +sorted[sorted.length - 1].toFixed(1),
    slow: deltas.filter((d) => d > 25).length,
  };
}

/** Scrolls `el` by `px` every frame for `frames` frames and records frame times. */
export async function measureScroll(el: HTMLElement, frames = 600, px = 60): Promise<ScrollStats> {
  el.scrollTop = 0;
  await nextPaint();
  const deltas: number[] = [];
  let last = performance.now();
  await new Promise<void>((done) => {
    const step = (now: number) => {
      deltas.push(now - last);
      last = now;
      if (deltas.length >= frames) return done();
      el.scrollTop = el.scrollTop + el.clientHeight >= el.scrollHeight - 1 ? 0 : el.scrollTop + px;
      requestAnimationFrame(step);
    };
    requestAnimationFrame(step);
  });
  return stats(deltas.slice(1));
}

/** Jumps to several scroll positions (like dragging the scrollbar) and returns the slowest paint. */
export async function measureJumps(el: HTMLElement, positions = [0.5, 1, 0.25, 0.75, 0]): Promise<number> {
  let worst = 0;
  for (const p of positions) {
    const t = performance.now();
    el.scrollTop = (el.scrollHeight - el.clientHeight) * p;
    await nextPaint();
    worst = Math.max(worst, performance.now() - t);
  }
  return +worst.toFixed(1);
}
