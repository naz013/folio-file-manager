/** Motion started from script, for what a stylesheet cannot trigger. Timings come from the tokens. */

import { reducedMotion } from "$lib/settings.svelte";

const token = (name: string) =>
  getComputedStyle(document.documentElement).getPropertyValue(name).trim();

/** A duration token in ms, or 0 while motion is off. */
export function motionMs(name: "--duration-fast" | "--duration-base" | "--duration-slow"): number {
  return reducedMotion() ? 0 : parseFloat(token(name)) || 0;
}

/** Fades `el` in. Cancels itself when something else starts, so it never holds anything up. */
export function fadeIn(el: HTMLElement) {
  const duration = motionMs("--duration-base");
  if (!duration) return;
  el.animate([{ opacity: 0 }, { opacity: 1 }], {
    duration,
    easing: token("--ease-standard") || "ease",
  });
}
