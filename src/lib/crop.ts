/** A rectangle in picture pixels. */
export interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export type Corner = "nw" | "ne" | "sw" | "se";

export const RATIOS = ["free", "original", "1:1", "4:3", "3:2", "16:9"] as const;
export type Ratio = (typeof RATIOS)[number];

/** Width over height for a choice of ratio, or null for a free crop. */
export function ratioValue(ratio: Ratio, width: number, height: number): number | null {
  if (ratio === "free") return null;
  if (ratio === "original") return width / height;
  const [w, h] = ratio.split(":").map(Number);
  return w / h;
}

/** `rect` moved by (dx, dy) and kept inside a picture of `width` by `height`. */
export function moveRect(rect: Rect, dx: number, dy: number, width: number, height: number): Rect {
  return {
    ...rect,
    x: Math.min(width - rect.width, Math.max(0, rect.x + dx)),
    y: Math.min(height - rect.height, Math.max(0, rect.y + dy)),
  };
}

/**
 * `start` with `corner` dragged to (px, py). The opposite corner stays where it is. With a
 * `ratio` (width over height) the rectangle keeps it, whichever way the pointer strays.
 */
export function resizeCorner(
  start: Rect,
  corner: Corner,
  px: number,
  py: number,
  ratio: number | null,
  width: number,
  height: number,
  min = 8,
): Rect {
  const ax = corner.includes("w") ? start.x + start.width : start.x;
  const ay = corner.includes("n") ? start.y + start.height : start.y;
  const tx = Math.min(width, Math.max(0, px));
  const ty = Math.min(height, Math.max(0, py));
  // The side the pointer is on; at the edge of the picture, the side that has room.
  const sx = ax >= width ? -1 : ax <= 0 ? 1 : tx >= ax ? 1 : -1;
  const sy = ay >= height ? -1 : ay <= 0 ? 1 : ty >= ay ? 1 : -1;
  // The room there is in the direction the pointer went.
  const roomX = sx > 0 ? width - ax : ax;
  const roomY = sy > 0 ? height - ay : ay;
  let w = Math.abs(tx - ax);
  let h = Math.abs(ty - ay);
  if (ratio) {
    if (w / Math.max(h, 1e-6) > ratio) h = w / ratio;
    else w = h * ratio;
    if (w > roomX) {
      w = roomX;
      h = w / ratio;
    }
    if (h > roomY) {
      h = roomY;
      w = h * ratio;
    }
  }
  w = Math.max(Math.min(w, roomX), Math.min(min, roomX));
  h = Math.max(Math.min(h, roomY), Math.min(min, roomY));
  if (ratio) {
    // The minimum for one side must not break the shape of the other.
    if (w / h > ratio) w = h * ratio;
    else h = w / ratio;
  }
  return { x: sx > 0 ? ax : ax - w, y: sy > 0 ? ay : ay - h, width: w, height: h };
}

/** The largest rectangle of `ratio` that is at most as wide as `rect`, around its centre. */
export function fitRatio(rect: Rect, ratio: number, width: number, height: number): Rect {
  let w = Math.min(rect.width, width);
  let h = w / ratio;
  if (h > height) {
    h = height;
    w = h * ratio;
  }
  const cx = rect.x + rect.width / 2;
  const cy = rect.y + rect.height / 2;
  return {
    x: Math.min(width - w, Math.max(0, cx - w / 2)),
    y: Math.min(height - h, Math.max(0, cy - h / 2)),
    width: w,
    height: h,
  };
}

/** `rect` rounded to whole pixels and kept inside the picture, at least one pixel big. */
export function wholePixels(rect: Rect, width: number, height: number): Rect {
  const x = Math.min(width - 1, Math.max(0, Math.round(rect.x)));
  const y = Math.min(height - 1, Math.max(0, Math.round(rect.y)));
  return {
    x,
    y,
    width: Math.min(width - x, Math.max(1, Math.round(rect.width))),
    height: Math.min(height - y, Math.max(1, Math.round(rect.height))),
  };
}
