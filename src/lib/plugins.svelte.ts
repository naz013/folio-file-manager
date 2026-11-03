/** The plugins the page knows about: what is on disk, and which the person has turned on. */

import { pluginsList } from "$lib/api/fs";
import { load, type Grant, type Loaded, type Manifest } from "$lib/plugins";
import { settings, updateSettings } from "$lib/settings.svelte";

export const plugins = $state<{ list: Loaded[]; loaded: boolean }>({ list: [], loaded: false });

/** Where a plugin's short messages go: the page's toast. Set by the page. */
let notifier: (text: string) => void = () => {};
export const setPluginNotifier = (fn: (text: string) => void) => (notifier = fn);
export const pluginNotify = (text: string) => notifier(text);

/** Counts reloads, so a screen showing what a plugin asks can tell that it may have changed. */
export const reloaded = $state({ n: 0 });

/** Looks in the plugins folder again. */
export async function reloadPlugins(): Promise<void> {
  reloaded.n++;
  const found = await pluginsList().catch(() => []);
  plugins.list = load(found, settings.plugins);
  plugins.loaded = true;
}

/** Turns a plugin on for what its manifest asks for now: the person has just agreed to that. */
export function enablePlugin(manifest: Manifest): void {
  const grant: Grant = { version: manifest.version, permissions: [...manifest.permissions] };
  updateSettings({ plugins: { ...settings.plugins, [manifest.id]: grant } });
  plugins.list = plugins.list.map((p) =>
    p.id === manifest.id ? { ...p, enabled: true, changed: false } : p,
  );
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
