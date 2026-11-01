/** The seven colours a tag can have, in the order of keys 1 to 7. */
export const TAG_COLORS = ["red", "orange", "yellow", "green", "blue", "purple", "gray"] as const;

/** The CSS colour of a tag: its colour token, or a neutral one for a tag without a colour. */
export const tagColor = (color: string | null): string =>
  color ? `var(--tag-${color})` : "var(--text-tertiary)";
