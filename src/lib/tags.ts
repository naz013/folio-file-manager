/** The seven colours a tag can have, in the order of keys 1 to 7. */
export const TAG_COLORS = ["red", "orange", "yellow", "green", "blue", "purple", "gray"] as const;

/** The hues of the tag screen's palette, left to right. */
export const TAG_HUES = [
  "red",
  "orange",
  "yellow",
  "green",
  "teal",
  "blue",
  "purple",
  "pink",
  "gray",
] as const;

/** Shades from lightest (1) to darkest (5). Shade 3 is the hue itself, saved as its plain name. */
export const TAG_SHADES = [1, 2, 3, 4, 5] as const;

/** The name a tag stores for a hue and shade. */
export const shadeName = (hue: string, shade: number): string =>
  shade === 3 ? hue : `${hue}-${shade}`;

/** The CSS colour of a tag: its colour token, or a neutral one for a tag without a colour. */
export const tagColor = (color: string | null): string =>
  color ? `var(--tag-${color})` : "var(--text-tertiary)";
