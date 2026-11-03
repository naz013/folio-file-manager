/** The notes in `from` if they are for `version`, else none. */
export function notesFor(version: string, from: { version: string; notes: string }): string {
  return from.version === version ? from.notes.trim() : "";
}

/**
 * Whether to show the notes at start. A fresh install is told about Folio by the tour, not
 * by the notes; a version already seen needs nothing; a new version with no notes has
 * nothing to show, but is still marked seen.
 */
export function shouldShowNotes(opts: {
  /** The tour has been seen: false on a fresh install. */
  onboarded: boolean;
  seen: string;
  version: string;
  notes: string;
}): boolean {
  return opts.onboarded && opts.seen !== opts.version && opts.notes !== "";
}
