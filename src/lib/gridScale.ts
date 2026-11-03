/** The size of the grid's tiles and thumbnails, which pinch, Ctrl+wheel, Ctrl +/- and the
 *  setting change. 1 is the size the design draws (the Penpot "File Tile"). */

export const MIN_SCALE = 0.6;
export const MAX_SCALE = 2.2;
export const DEFAULT_SCALE = 1;

/** One press of Ctrl + or Ctrl -, as a factor. */
export const STEP = 1.15;

/** The sizes the Settings slider moves between: each about a fifth bigger than the last. */
export const SCALE_STOPS = [0.6, 0.75, 0.9, 1, 1.2, 1.45, 1.75, 2.2] as const;

export const clampScale = (v: number): number =>
  Number.isFinite(v) ? Math.min(MAX_SCALE, Math.max(MIN_SCALE, v)) : DEFAULT_SCALE;

/** A saved value, or the default if it isn't one. */
export const readScale = (v: unknown): number =>
  typeof v === "number" ? clampScale(v) : DEFAULT_SCALE;

// The tile at scale 1: a 112 x 88 picture box with 6 px padding and a two-line name, which
// is 62 px that do not scale.
const THUMB_W = 112;
const THUMB_H = 88;
const PAD_W = 20;
const PAD_H = 62;
/** The picture inside its box at scale 1, and the folder icon. */
const PICTURE = 72;
const FOLDER = 64;

export interface TileSize {
  w: number;
  h: number;
  thumbW: number;
  thumbH: number;
  picture: number;
  folder: number;
}

export function tileSize(scale: number): TileSize {
  const s = clampScale(scale);
  return {
    w: Math.round(THUMB_W * s + PAD_W),
    h: Math.round(THUMB_H * s + PAD_H),
    thumbW: Math.round(THUMB_W * s),
    thumbH: Math.round(THUMB_H * s),
    picture: Math.round(PICTURE * s),
    folder: Math.round(FOLDER * s),
  };
}

/** The size of picture to ask the core for, in steps so that a small change in scale doesn't
 *  ask for every picture again (and the cache keeps its entries). */
export const fetchSize = (picture: number): number => Math.max(16, Math.ceil(picture / 8) * 8);

/** The next scale a key press asks for; `dir` 0 is back to the default. */
export function stepScale(scale: number, dir: number): number {
  if (dir === 0) return DEFAULT_SCALE;
  return clampScale(scale * STEP ** Math.sign(dir));
}

/** The slider's stop nearest to a scale (and the scale of a stop). */
export function nearestStop(scale: number): number {
  let best = 0;
  SCALE_STOPS.forEach((s, i) => {
    if (Math.abs(Math.log(s / scale)) < Math.abs(Math.log(SCALE_STOPS[best] / scale))) best = i;
  });
  return best;
}

export interface Geometry {
  padX: number;
  padY: number;
  tileW: number;
  gapX: number;
  rowH: number;
  cols: number;
}

/**
 * Where to scroll so that the item that was under a point stays under it when the tiles change
 * size. `x` and `y` are the point in canvas coordinates (scrolling included) before the change;
 * `viewY` is its distance from the top of the window. The item keeps its place in the order, and
 * how far down its row the point was.
 */
export function scrollToKeep(
  before: Geometry,
  after: Geometry,
  point: { x: number; y: number },
  viewY: number,
  count: number,
  scrollHeight: number,
  viewHeight: number,
): number {
  if (count <= 0) return 0;
  const row = Math.max(0, Math.floor((point.y - before.padY) / before.rowH));
  const col = Math.min(
    before.cols - 1,
    Math.max(0, Math.floor((point.x - before.padX) / (before.tileW + before.gapX))),
  );
  const index = Math.min(count - 1, row * before.cols + col);
  const within = Math.min(1, Math.max(0, (point.y - before.padY) / before.rowH - row));
  const newRow = Math.floor(index / after.cols);
  const y = after.padY + (newRow + within) * after.rowH;
  return Math.min(Math.max(0, scrollHeight - viewHeight), Math.max(0, y - viewY));
}
