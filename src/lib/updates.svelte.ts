import { check, type Update } from "@tauri-apps/plugin-updater";
import { updateGate, updateRestart } from "$lib/api/fs";
import { settings, updateSettings } from "$lib/settings.svelte";

const DAY_MS = 24 * 60 * 60 * 1000;

type Phase = "idle" | "checking" | "available" | "current" | "downloading" | "failed";

/** What the update banner shows. */
export const updates: {
  phase: Phase;
  /** The new version, while one is on offer or being fetched. */
  version: string;
  /** Percent downloaded, or null while the size is not known. */
  percent: number | null;
  /** Why the last check or install failed. */
  error: string;
  /** The banner is hidden ("Later"), though a version is still known. */
  hidden: boolean;
} = $state({ phase: "idle", version: "", percent: null, error: "", hidden: false });

/** The update the check found. Not reactive: it is a handle, not something to show. */
let pending: Update | null = null;
/** The failure shown is about installing, not about a check. */
let installFailed = false;

/**
 * Looks for a new version. A background check (`manual: false`) is silent unless it finds
 * one, runs at most once a day, and skips a version the user already said "Later" to. A
 * manual one always answers.
 */
export async function checkForUpdate(manual: boolean): Promise<void> {
  if (updates.phase === "checking" || updates.phase === "downloading") return;
  if (!manual && (!settings.updateCheck || Date.now() - settings.updateLastCheck < DAY_MS)) return;
  updates.phase = "checking";
  updates.error = "";
  try {
    const found = await check();
    updateSettings({ updateLastCheck: Date.now() });
    if (!found) {
      pending = null;
      updates.phase = manual ? "current" : "idle";
      return;
    }
    pending = found;
    updates.version = found.version;
    updates.hidden = !manual && settings.updateSkipped === found.version;
    updates.phase = "available";
  } catch (e) {
    updates.phase = manual ? "failed" : "idle";
    // Offline or no release yet is normal for a background check; say nothing.
    if (manual) updates.error = `Could not check for updates: ${e}`;
  }
}

/** Hides the banner and does not offer this version again by itself. */
export function postponeUpdate() {
  updateSettings({ updateSkipped: updates.version });
  updates.hidden = true;
}

/**
 * Downloads the update and installs it. The signature is checked by the updater before
 * anything is replaced, so a bad or tampered download leaves the current version as it is.
 * Nothing starts while Folio is working on files.
 */
export async function installUpdate(): Promise<void> {
  if (!pending || updates.phase === "downloading") return;
  try {
    await updateGate();
  } catch (e) {
    updates.phase = "failed";
    updates.error = String(e);
    return;
  }
  installFailed = false;
  updates.phase = "downloading";
  updates.percent = null;
  updates.error = "";
  let total = 0;
  let got = 0;
  try {
    await pending.download((event) => {
      if (event.event === "Started") total = event.data.contentLength ?? 0;
      else if (event.event === "Progress") {
        got += event.data.chunkLength;
        updates.percent = total ? Math.min(100, Math.floor((got / total) * 100)) : null;
      }
    });
    // The download can take long; a copy may have started meanwhile. On Windows the
    // installer ends the app, so the gate is looked at again right before it runs.
    await updateGate();
    await pending.install();
  } catch (e) {
    updates.phase = "failed";
    updates.error = `The update was not installed: ${e}`;
    installFailed = true;
    return;
  }
  // Windows has already closed the app for its installer; elsewhere it is started again.
  try {
    await updateRestart();
  } catch (e) {
    updates.phase = "failed";
    updates.error = `The update is installed. Restart Folio to use it: ${e}`;
    installFailed = true;
  }
}

/** Whether the failure on show came from installing (the banner reports those only). */
export const installDidFail = () => installFailed;

/** Dismisses a failure message. */
export function clearUpdateError() {
  installFailed = false;
  updates.error = "";
  updates.phase = pending ? "available" : "idle";
}
