export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

/** The worst aspect ratio among `row`'s areas when laid along a side of length `side`. */
function worst(row: number[], side: number): number {
  const sum = row.reduce((a, b) => a + b, 0);
  const max = Math.max(...row);
  const min = Math.min(...row);
  return Math.max((side * side * max) / (sum * sum), (sum * sum) / (side * side * min));
}

/**
 * Squarified treemap (Bruls, Huizing, van Wijk): one rectangle per value, in the order given
 * (largest first gives the best result), filling `width` x `height` so that each area is
 * proportional to its value and the shapes stay close to square. Zero or negative values
 * get an empty rectangle.
 */
export function squarify(values: number[], width: number, height: number): Rect[] {
  const total = values.reduce((a, v) => a + Math.max(v, 0), 0);
  const rects: Rect[] = values.map(() => ({ x: 0, y: 0, w: 0, h: 0 }));
  if (total <= 0 || width <= 0 || height <= 0) return rects;
  const scale = (width * height) / total;
  const items = values
    .map((v, i) => ({ i, area: Math.max(v, 0) * scale }))
    .filter((it) => it.area > 0);

  let x = 0;
  let y = 0;
  let w = width;
  let h = height;
  let at = 0;
  while (at < items.length) {
    const side = Math.min(w, h);
    // Grow the row while it keeps the shapes squarer.
    let end = at + 1;
    while (
      end < items.length &&
      worst(
        items.slice(at, end + 1).map((it) => it.area),
        side,
      ) <=
        worst(
          items.slice(at, end).map((it) => it.area),
          side,
        )
    ) {
      end++;
    }
    const row = items.slice(at, end);
    const sum = row.reduce((a, it) => a + it.area, 0);
    const thickness = sum / side;
    let offset = 0;
    for (const it of row) {
      const length = it.area / thickness;
      rects[it.i] =
        w >= h
          ? { x, y: y + offset, w: thickness, h: length }
          : { x: x + offset, y, w: length, h: thickness };
      offset += length;
    }
    if (w >= h) {
      x += thickness;
      w -= thickness;
    } else {
      y += thickness;
      h -= thickness;
    }
    at = end;
  }
  return rects;
}
