/** How much the interface moves. "system" follows the OS's reduced-motion choice. */
export type Motion = "system" | "full" | "reduced";

export const MOTIONS: readonly Motion[] = ["system", "full", "reduced"];

/** Past this many entries a folder's list skips its own animations. */
export const LIST_ANIMATION_MAX = 2000;

export const isMotion = (v: unknown): v is Motion => MOTIONS.includes(v as Motion);

/** Whether motion is off, given the setting and whether the OS asked for less. */
export const motionReduced = (motion: Motion, systemReduced: boolean) =>
  motion === "reduced" || (motion === "system" && systemReduced);

/** Whether a list of `count` entries should animate. */
export const animatesList = (count: number, reduced: boolean) =>
  !reduced && count <= LIST_ANIMATION_MAX;
