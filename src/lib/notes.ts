import bundled from "$lib/release-notes.json";
import { notesFor } from "$lib/whatsnew";

/**
 * The notes of the release this build is, written into the app by the release workflow
 * (scripts/release-notes.mjs), so they are there offline. A build made by hand has none.
 */
export const bundledNotes = (version: string) => notesFor(version, bundled);
