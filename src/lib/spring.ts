/** Spring-loaded navigation: hovering a folder mid-drag opens it after a delay. */

const KEY = "folio.spring";

export interface SpringSettings {
  /** How long to hover before the folder opens; null turns spring-loading off. */
  delayMs: number | null;
  /** After a drop or cancel, go back to the folder the drag started in. */
  returnAfterDrop: boolean;
}

const MIN_DELAY = 500;
const MAX_DELAY = 3000;
const DEFAULTS: SpringSettings = { delayMs: 2000, returnAfterDrop: false };

/** The saved settings (exposed in Settings, #25); out-of-range delays are clamped. */
export function loadSpring(): SpringSettings {
  try {
    const saved = JSON.parse(localStorage.getItem(KEY) ?? "null") as Partial<SpringSettings> | null;
    if (saved) {
      const delay = saved.delayMs === null ? null : Number(saved.delayMs ?? DEFAULTS.delayMs);
      return {
        delayMs: delay === null ? null : Math.min(MAX_DELAY, Math.max(MIN_DELAY, delay || 0)),
        returnAfterDrop: saved.returnAfterDrop === true,
      };
    }
  } catch {
    // Unreadable settings: use the defaults.
  }
  return { ...DEFAULTS };
}

/** Counts down while the drag hovers one target, then asks for it to be opened. */
export function createSpring(hooks: {
  delayMs: () => number | null;
  /** Whether `target` may open (not the current folder, not what is being dragged). */
  eligible: (target: string) => boolean;
  open: (target: string) => void;
  /** The target whose countdown is running, with its length; null when none is. */
  ring: (target: string | null, ms: number) => void;
}) {
  let target: string | null = null;
  let timer: ReturnType<typeof setTimeout> | undefined;

  function clear() {
    clearTimeout(timer);
    timer = undefined;
  }

  function fire() {
    clear();
    const t = target;
    target = null;
    hooks.ring(null, 0);
    if (t) hooks.open(t);
  }

  return {
    /** The drag is over `next` (null: over no folder). */
    hover(next: string | null) {
      if (next === target) return;
      clear();
      target = next;
      const ms = hooks.delayMs();
      if (next && ms !== null && hooks.eligible(next)) {
        hooks.ring(next, ms);
        timer = setTimeout(fire, ms);
      } else {
        hooks.ring(null, 0);
      }
    },
    /** Space: open the hovered folder without waiting. */
    openNow() {
      if (target && hooks.eligible(target)) fire();
    },
    /** The drag ended. */
    stop() {
      clear();
      target = null;
      hooks.ring(null, 0);
    },
  };
}
