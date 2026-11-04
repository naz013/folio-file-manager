<script lang="ts">
  import { onDestroy, untrack } from "svelte";
  import { pluginUrl, pluginsPin, readTextPreview, readTextRange } from "$lib/api/fs";
  import {
    createBridge,
    isReady,
    pageDoc,
    type Event as PluginEvent,
    type Loaded,
    type Manifest,
  } from "$lib/plugins";
  import { disablePlugin, pluginNotify } from "$lib/plugins.svelte";

  /** How long a page has to say it has loaded before it is called unresponsive (ms). */
  const READY_MS = 8000;

  let {
    plugin,
    hidden = false,
    hello,
    onskip,
  }: {
    plugin: Loaded & { manifest: Manifest };
    /** An action's page runs unseen; a preview's fills its place. */
    hidden?: boolean;
    /** Sent as soon as the page says it is ready, and again if it reloads. */
    hello?: { event: PluginEvent; paths: string[] };
    /** Given for a preview: how to show the file the usual way if the plugin doesn't answer. */
    onskip?: () => void;
  } = $props();

  let frame: HTMLIFrameElement | undefined = $state();
  let ready = $state(false);
  let silent = $state(false);
  /** The page's document, once read; the frame is made from it (see `pageDoc`). */
  let doc: string | null = $state(null);
  let failed = $state(false);
  /** What waits for the page to be ready, in the order it was asked. */
  let waiting: { event: PluginEvent; paths: string[] }[] = [];

  const manifest = $derived(plugin.manifest);
  // One bridge for as long as this is the same plugin at the same version: a list that is
  // rebuilt around it (Reload, another plugin used) must not wipe what it has been handed.
  const bridge = $derived.by(() => {
    void plugin.id;
    void manifest.version;
    return untrack(() =>
      createBridge(plugin.manifest, {
        notify: pluginNotify,
        readText: readTextPreview,
        readRange: readTextRange,
        // A page that floods the bridge is turned off, which also removes its frame.
        abuse: () => {
          pluginNotify(
            `${plugin.manifest.name} (plugin) was turned off for sending too many requests`,
          );
          disablePlugin(plugin.id);
        },
      }),
    );
  });

  function post(message: unknown) {
    // The page has no origin of its own (it is sandboxed), so "*" is the only target there is.
    frame?.contentWindow?.postMessage(message, "*");
  }

  /** Tells the page something happens (an action runs), letting it read the files it names. */
  export function send(event: PluginEvent, paths: string[]) {
    if (ready) {
      bridge.allow(paths);
      post(event);
    } else {
      waiting.push({ event, paths });
    }
  }

  function onmessage(e: MessageEvent) {
    // Only the frame this component made, never another window or frame.
    if (!frame || e.source !== frame.contentWindow) return;
    if (isReady(e.data)) {
      ready = true;
      silent = false;
      const next = waiting.length ? waiting : hello ? [hello] : [];
      waiting = [];
      for (const item of next) send(item.event, item.paths);
      return;
    }
    bridge.handle(e.data).then((answer) => answer && post(answer));
  }

  // The page is read here and handed to the frame as its document, never as a link to
  // `plugin.localhost`: Tauri trusts that address like Folio's own page (see `pageDoc`).
  $effect(() => {
    const url = pluginUrl(plugin.id, manifest.entry);
    let current = true;
    doc = null;
    failed = false;
    // The core serves nothing of a plugin until it is pinned to the files that were approved,
    // so a file swapped after the approval is refused instead of run.
    (plugin.digest ? pluginsPin(plugin.id, plugin.digest) : Promise.reject(new Error("no digest")))
      .then(() => fetch(url, { cache: "no-store" }))
      .then((r) => (r.ok ? r.text() : Promise.reject(new Error(String(r.status)))))
      .then((html) => {
        if (!current) return;
        const where = new URL(url);
        doc = pageDoc(
          html,
          url.slice(0, url.lastIndexOf("/") + 1),
          `${where.protocol}//${where.host}`,
        );
      })
      .catch(() => {
        if (current) failed = true;
      });
    return () => {
      current = false;
    };
  });

  const timer = setTimeout(() => {
    if (!ready) silent = true;
  }, READY_MS);
  onDestroy(() => clearTimeout(timer));
  // A preview stays open as long as the person reads, so its file stays readable.
  const keep = setInterval(() => {
    if (hello) bridge.keepAlive(hello.paths);
  }, 20_000);
  onDestroy(() => clearInterval(keep));
</script>

<svelte:window {onmessage} />

<div class="plugin" class:hidden>
  {#if doc !== null}
    <iframe
      bind:this={frame}
      title={plugin.manifest.name}
      srcdoc={doc}
      sandbox="allow-scripts"
      referrerpolicy="no-referrer"
    ></iframe>
  {/if}
  {#if failed && !hidden}
    <p class="note" role="status">
      {manifest.name} could not be loaded.
      {#if onskip}<button onclick={onskip}>Show normally</button>{/if}
    </p>
  {:else if silent && !hidden}
    <p class="note" role="status">
      {manifest.name} did not respond.
      {#if onskip}<button onclick={onskip}>Show normally</button>{/if}
    </p>
  {/if}
</div>

<style>
  .plugin {
    position: relative;
    display: flex;
    flex: 1;
    min-width: 0;
    min-height: 0;
    width: 100%;
    height: 100%;
  }

  .plugin.hidden {
    display: none;
  }

  iframe {
    flex: 1;
    width: 100%;
    height: 100%;
    border: 0;
    background: transparent;
    color-scheme: light dark;
  }

  .note button {
    margin-left: var(--space-xs);
    padding: var(--space-2xs) var(--space-sm);
    border-radius: var(--radius-sm);
    background: var(--bg-control);
  }

  .note {
    position: absolute;
    inset: auto var(--space-md) var(--space-md) var(--space-md);
    margin: 0;
    padding: var(--space-xs) var(--space-sm);
    border-radius: var(--radius-md);
    background: var(--bg-subtle);
    color: var(--text-secondary);
  }
</style>
