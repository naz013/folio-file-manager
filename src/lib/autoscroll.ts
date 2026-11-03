/** Scrolling a list while a drag is held near its edge. Pure, so it can be tested. */

/**
 * How far to scroll along one axis for a pointer at `pos` over a box spanning `lo`..`hi`: negative
 * near `lo`, positive near `hi`, faster the closer to the edge, at most `max`; 0 in the middle.
 */
export function edgeStep(pos: number, lo: number, hi: number, edge: number, max: number): number {
  const toLo = lo + edge - pos;
  const toHi = pos - (hi - edge);
  if (toLo > 0) return -Math.min(max, (toLo / edge) * max);
  if (toHi > 0) return Math.min(max, (toHi / edge) * max);
  return 0;
}

/**
 * The axes a scroll element can move along: where it overflows by more than a rounding pixel,
 * and, sideways, where the person can scroll it. A list hides its sideways overflow, which
 * `scrollBy` would still move: a drag must not.
 */
export function scrollableAxes(
  el: { scrollWidth: number; clientWidth: number; scrollHeight: number; clientHeight: number },
  overflowX = "auto",
): { x: boolean; y: boolean } {
  return {
    x: overflowX !== "hidden" && el.scrollWidth - el.clientWidth > 1,
    y: el.scrollHeight - el.clientHeight > 1,
  };
}

/**
 * The scroll to apply for a pointer at (`x`, `y`) over `box`: the vertical zones count while the
 * pointer is between the box's sides, the horizontal ones while it is between its top and bottom.
 */
export function dragScroll(
  x: number,
  y: number,
  box: { left: number; right: number; top: number; bottom: number },
  axes: { x: boolean; y: boolean },
  edge: number,
  max: number,
): { dx: number; dy: number } {
  const insideX = x >= box.left && x <= box.right;
  const insideY = y >= box.top && y <= box.bottom;
  return {
    dx: axes.x && insideY ? edgeStep(x, box.left, box.right, edge, max) : 0,
    dy: axes.y && insideX ? edgeStep(y, box.top, box.bottom, edge, max) : 0,
  };
}
