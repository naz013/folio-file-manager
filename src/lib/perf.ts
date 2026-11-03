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

/** The axis a list scrolls along: the gallery's filmstrip scrolls sideways. */
export type Axis = "x" | "y";

const at = (el: HTMLElement, axis: Axis) => (axis === "x" ? el.scrollLeft : el.scrollTop);
const moveTo = (el: HTMLElement, axis: Axis, to: number) => {
  if (axis === "x") el.scrollLeft = to;
  else el.scrollTop = to;
};
const room = (el: HTMLElement, axis: Axis) =>
  axis === "x" ? el.scrollWidth - el.clientWidth : el.scrollHeight - el.clientHeight;

/** Scrolls `el` by `px` every frame for `frames` frames and records frame times. */
export async function measureScroll(
  el: HTMLElement,
  frames = 600,
  px = 60,
  axis: Axis = "y",
): Promise<ScrollStats> {
  moveTo(el, axis, 0);
  await nextPaint();
  const deltas: number[] = [];
  let last = performance.now();
  await new Promise<void>((done) => {
    const step = (now: number) => {
      deltas.push(now - last);
      last = now;
      if (deltas.length >= frames) return done();
      moveTo(el, axis, at(el, axis) >= room(el, axis) - 1 ? 0 : at(el, axis) + px);
      requestAnimationFrame(step);
    };
    requestAnimationFrame(step);
  });
  return stats(deltas.slice(1));
}

/** Jumps to several scroll positions (like dragging the scrollbar) and returns the slowest paint. */
export async function measureJumps(
  el: HTMLElement,
  positions = [0.5, 1, 0.25, 0.75, 0],
  axis: Axis = "y",
): Promise<number> {
  let worst = 0;
  for (const p of positions) {
    const t = performance.now();
    moveTo(el, axis, room(el, axis) * p);
    await nextPaint();
    worst = Math.max(worst, performance.now() - t);
  }
  return +worst.toFixed(1);
}

/** Jumps to a place along the list (0 start, 1 end), like dragging the scrollbar, and waits for the paint. */
export async function jumpTo(el: HTMLElement, fraction: number, axis: Axis = "y"): Promise<void> {
  moveTo(el, axis, room(el, axis) * fraction);
  await nextPaint();
}

/**
 * Holds the right arrow key, as a held key repeats, for `frames` frames: the gallery's cursor
 * moves one picture a frame and the big picture and the strip follow it.
 */
export async function measureArrowHold(frames = 300): Promise<ScrollStats> {
  const deltas: number[] = [];
  let last = performance.now();
  await new Promise<void>((done) => {
    const step = (now: number) => {
      deltas.push(now - last);
      last = now;
      if (deltas.length >= frames) return done();
      window.dispatchEvent(
        new KeyboardEvent("keydown", {
          key: "ArrowRight",
          bubbles: true,
          cancelable: true,
          repeat: true,
        }),
      );
      requestAnimationFrame(step);
    };
    requestAnimationFrame(step);
  });
  return stats(deltas.slice(1));
}

/**
 * Waits, up to `timeoutMs`, until every picture tile in `root` shows its picture, and returns how
 * many still do not: those are left on the plain icon (or still waiting) once movement stopped.
 */
export async function settleTiles(
  root: HTMLElement,
  timeoutMs = 8000,
): Promise<{ tiles: number; unpainted: number; waitedMs: number }> {
  const t0 = performance.now();
  for (;;) {
    // Folders carry data-drop-path and have no picture to wait for.
    const tiles = [...root.querySelectorAll<HTMLElement>(".tile:not([data-drop-path])")];
    const unpainted = tiles.filter((t) => !t.querySelector("img.loaded")).length;
    const waitedMs = Math.round(performance.now() - t0);
    if (unpainted === 0 || waitedMs >= timeoutMs)
      return { tiles: tiles.length, unpainted, waitedMs };
    await new Promise((r) => setTimeout(r, 100));
  }
}
