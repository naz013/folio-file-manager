/** Smooth zooming for pinch and Ctrl+wheel, shared by the picture and PDF viewers. A
 *  trackpad pinch can reach the page as a few large wheel events rather than many small ones;
 *  applying each at once shows as steps. Here each event only moves a target, and the zoom
 *  approaches it a little every frame. */

/** The most one wheel event may zoom by, in pixels of `deltaY`: a notch of a mouse wheel is
 *  100 and a fast pinch can report more. */
export const WHEEL_STEP_MAX = 40;

/** How quickly the zoom closes on its target (ms for about two thirds of the way). */
const EASE_MS = 90;

/** Close enough to the target to stop (a fraction of the zoom, in log terms). */
const SNAP = 0.002;

/** What a wheel event's deltas are multiplied by to get pixels: lines and pages (a mouse
 *  wheel) are scaled up. */
export function pixelsPer(e: { deltaMode: number }, pageHeight: number): number {
  return e.deltaMode === 1 ? 16 : e.deltaMode === 2 ? pageHeight : 1;
}

/** The zoom factor one wheel event asks for; positive `deltaY` zooms out. */
export function wheelFactor(
  e: { deltaY: number; deltaMode: number },
  pageHeight: number,
  clampTo = WHEEL_STEP_MAX,
): number {
  const d = Math.max(-clampTo, Math.min(clampTo, e.deltaY * pixelsPer(e, pageHeight)));
  return Math.exp(-d * 0.01);
}

/** One frame of easing from `current` towards `target`. Zoom is a ratio, so it moves by a
 *  share of the ratio (the same speed when zooming in as out); `dt` is the frame's ms. */
export function easeStep(current: number, target: number, dt: number): number {
  if (current <= 0 || target <= 0) return target;
  const rest = Math.log(target / current);
  if (Math.abs(rest) < SNAP) return target;
  const share = 1 - Math.exp(-Math.max(0, dt) / EASE_MS);
  const next = current * Math.exp(rest * share);
  return Math.abs(Math.log(target / next)) < SNAP ? target : next;
}

export type ZoomEase = {
  /** Asks for the zoom to be multiplied by `factor`, keeping the point (x, y) fixed. Asks
   *  that come before the last has finished add up. */
  by(factor: number, x: number, y: number): void;
  /** Asks for the zoom to become `value`, at once when `instant` (reduced motion). */
  to(value: number, x: number, y: number, instant?: boolean): void;
  /** Drops what is under way; something else (a touch pinch, a new picture) took over. */
  stop(): void;
};

export function createZoomEase(opts: {
  get: () => number;
  min: number;
  max: number;
  /** Sets the zoom to `z` around (x, y); the viewer keeps the point fixed. */
  apply: (z: number, x: number, y: number) => void;
  /** For tests; the page's own frame loop by default. */
  request?: (cb: (now: number) => void) => number;
  cancel?: (id: number) => void;
}): ZoomEase {
  const request = opts.request ?? ((cb) => requestAnimationFrame(cb));
  const cancel = opts.cancel ?? ((id) => cancelAnimationFrame(id));
  let target: number | null = null;
  let at: [number, number] = [0, 0];
  let frame = 0;
  let last = 0;

  const clamp = (v: number) => Math.min(opts.max, Math.max(opts.min, v));

  function tick(now: number) {
    frame = 0;
    if (target === null) return;
    const next = easeStep(opts.get(), target, Math.min(now - last, 50));
    last = now;
    opts.apply(next, at[0], at[1]);
    if (next === target) target = null;
    else frame = request(tick);
  }

  function stop() {
    if (frame) cancel(frame);
    frame = 0;
    target = null;
  }

  function to(value: number, x: number, y: number, instant = false) {
    at = [x, y];
    if (instant) {
      stop();
      opts.apply(clamp(value), x, y);
      return;
    }
    target = clamp(value);
    if (!frame) {
      frame = request((now) => {
        // The first frame has no time behind it yet; it takes a typical one.
        last = now - 16;
        tick(now);
      });
    }
  }

  return {
    by: (factor, x, y) => to((target ?? opts.get()) * factor, x, y),
    to,
    stop,
  };
}
