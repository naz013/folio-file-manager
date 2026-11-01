/** Spring-loaded navigation: hovering a folder mid-drag opens it after a delay. */

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
