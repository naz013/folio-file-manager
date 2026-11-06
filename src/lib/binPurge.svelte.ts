import { binPurge, type BinRef } from "$lib/api/fs";

/**
 * The permanent delete in progress, if any. Kept outside the bin view so that leaving the
 * view mid-delete and coming back still shows how far it is (the core keeps going either way).
 */
export const purging = $state({ active: false, done: 0, total: 0 });

/** Deletes the items for good, reporting into `purging`. Rejects if a delete is already running. */
export async function purgeWithProgress(items: BinRef[]): Promise<void> {
  if (purging.active) throw new Error("Already deleting items from the bin");
  purging.active = true;
  purging.done = 0;
  purging.total = items.length;
  try {
    await binPurge(items, (done) => (purging.done = done));
  } finally {
    purging.active = false;
  }
}
