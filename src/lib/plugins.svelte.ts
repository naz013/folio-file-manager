/** The plugins the page knows about: what is on disk, and which the person has turned on. */

import { pluginUrl, pluginsList, pluginsPin } from "$lib/api/fs";
import { load, thumbnailFor, type Grant, type Loaded, type Manifest } from "$lib/plugins";
import { settings, updateSettings } from "$lib/settings.svelte";

export const plugins = $state<{ list: Loaded[]; loaded: boolean }>({ list: [], loaded: false });

/** Where a plugin's short messages go: the page's toast. Set by the page. */
let notifier: (text: string) => void = () => {};
export const setPluginNotifier = (fn: (text: string) => void) => (notifier = fn);
export const pluginNotify = (text: string) => notifier(text);

/** Counts reloads, so a screen showing what a plugin asks can tell that it may have changed. */
export const reloaded = $state({ n: 0 });

/**
 * The plugins whose pictures can be fetched: the core serves a plugin's files only once it is
 * pinned to what was approved, so a thumbnail waits for that.
 */
const pinned = $state<Record<string, boolean>>({});

async function pinThumbnails(): Promise<void> {
  for (const p of plugins.list) {
    if (!p.enabled || !p.digest || !p.manifest?.thumbnails.length) {
      delete pinned[p.id];
      continue;
    }
    if (pinned[p.id]) continue;
    try {
      await pluginsPin(p.id, p.digest);
      pinned[p.id] = true;
    } catch {
      // Files that changed since the approval: no pictures, and the plugin stays off.
    }
  }
}

/**
 * The picture and badge colour a plugin gives to files with this extension: nothing unless that
 * plugin is on and pinned, so a turned-off plugin leaves the usual icon. Read in a component, it
 * updates when plugins change.
 */
export function thumbnailArt(ext: string): { url: string; color: string } | null {
  const hit = thumbnailFor(plugins.list, ext);
  if (!hit || !pinned[hit.plugin.id]) return null;
  return { url: pluginUrl(hit.plugin.id, hit.thumb.image), color: hit.thumb.color };
}

/** Looks in the plugins folder again. */
export async function reloadPlugins(): Promise<void> {
  reloaded.n++;
  const found = await pluginsList().catch(() => null);
  for (const id of Object.keys(pinned)) delete pinned[id];
  plugins.list = load(found ?? [], settings.plugins);
  plugins.loaded = true;
  void pinThumbnails();
  // An approval belongs to the plugin that was approved: when its folder is gone, so is the
  // approval, and a later folder of the same name starts from nothing. Not done when the
  // folder couldn't be read at all.
  if (found) {
    const kept = Object.entries(settings.plugins).filter(([id]) => found.some((f) => f.id === id));
    if (kept.length !== Object.keys(settings.plugins).length) {
      updateSettings({ plugins: Object.fromEntries(kept) });
    }
  }
}

/** Turns a plugin on for what its manifest and files are now: the person has just agreed to that. */
export function enablePlugin(manifest: Manifest, digest: string | null): void {
  if (digest === null) return;
  const grant: Grant = {
    version: manifest.version,
    permissions: [...manifest.permissions],
    digest,
  };
  updateSettings({ plugins: { ...settings.plugins, [manifest.id]: grant } });
  plugins.list = plugins.list.map((p) =>
    p.id === manifest.id ? { ...p, enabled: true, changed: false } : p,
  );
  void pinThumbnails();
}

/** Turns a plugin off and forgets what was agreed to. */
export function disablePlugin(id: string): void {
  const rest = { ...settings.plugins };
  delete rest[id];
  updateSettings({ plugins: rest });
  plugins.list = plugins.list.map((p) =>
    p.id === id ? { ...p, enabled: false, changed: false } : p,
  );
}
