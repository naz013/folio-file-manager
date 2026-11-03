/** What a person can Tab to inside a dialog. */
const TABBABLE =
  'a[href], button:not(:disabled), input:not(:disabled), select:not(:disabled), textarea:not(:disabled), [tabindex]:not([tabindex="-1"])';

/**
 * Keeps Tab and Shift+Tab inside a dialog: past the last control it goes back to the first, and
 * before the first it goes to the last, instead of into the page behind the dialog.
 */
export function trapFocus(node: HTMLElement) {
  function onkeydown(e: KeyboardEvent) {
    if (e.key !== "Tab" || e.ctrlKey || e.metaKey || e.altKey) return;
    const stops = [...node.querySelectorAll<HTMLElement>(TABBABLE)].filter(
      (el) => el.offsetParent !== null,
    );
    if (!stops.length) {
      e.preventDefault();
      return;
    }
    const first = stops[0];
    const last = stops[stops.length - 1];
    const at = document.activeElement;
    // Focus is on the dialog itself (it takes it on opening) or outside it: start at an end.
    if (!node.contains(at) || at === node) {
      e.preventDefault();
      (e.shiftKey ? last : first).focus();
    } else if (e.shiftKey && at === first) {
      e.preventDefault();
      last.focus();
    } else if (!e.shiftKey && at === last) {
      e.preventDefault();
      first.focus();
    }
  }
  node.addEventListener("keydown", onkeydown);
  return { destroy: () => node.removeEventListener("keydown", onkeydown) };
}
