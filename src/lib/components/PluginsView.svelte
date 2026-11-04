<script lang="ts">
  import { onMount } from "svelte";
  import { openPath, pluginsFolder } from "$lib/api/fs";
  import { PERMISSIONS } from "$lib/plugins";
  import {
    disablePlugin,
    enablePlugin,
    plugins,
    reloaded,
    reloadPlugins,
  } from "$lib/plugins.svelte";
  import EmptyState from "./EmptyState.svelte";
  import Icon from "./Icon.svelte";

  let {
    onopen,
  }: {
    /** Show a folder in a tab of Folio. */
    onopen: (path: string) => void;
  } = $props();

  let dialog: HTMLElement;
  onMount(() => dialog.focus());

  /** The plugin whose permissions are being read before it is turned on. */
  let reviewing = $state<string | null>(null);
  let note = $state("");

  // What a plugin asks may have changed on a reload: the person reads it again.
  $effect(() => {
    void reloaded.n;
    reviewing = null;
  });

  /** The plugins folder in a tab of Folio, or in the system's file manager. */
  async function openFolder(inSystem = false) {
    note = "";
    try {
      const folder = await pluginsFolder();
      if (inSystem) await openPath(folder);
      else onopen(folder);
    } catch (e) {
      note = String(e);
    }
  }

  const dotted = (ext: string) => `.${ext}`;
</script>

<section bind:this={dialog} class="plugins" aria-labelledby="plugins-title" tabindex="-1">
  <div class="page">
    <header>
      <h2 id="plugins-title">Plugins</h2>
      <div class="buttons">
        <button onclick={() => openFolder()}>Open folder</button>
        <button onclick={() => openFolder(true)}>Show in Explorer</button>
        <button onclick={reloadPlugins}>Reload</button>
      </div>
    </header>

    <p class="lead">
      Plugins add actions to the right-click menu and previews to Quick Look. Each runs in a
      sandbox: it can't read or change your files, or use the network, except for what you allow. A
      plugin is a folder with a <code>plugin.json</code> inside the plugins folder.
    </p>

    {#if note}
      <p class="callout danger" role="alert">
        <Icon name="alert" size={14} /><span>{note}</span>
      </p>
    {/if}

    {#each plugins.list as p (p.id)}
      {@const m = p.manifest}
      <article class="card" class:reviewing={reviewing === p.id} aria-label={m?.name ?? p.id}>
        <div class="top">
          <div class="who">
            <h3>
              <span class="name">{m?.name ?? p.id}</span>
              {#if m}<span class="version">v{m.version}</span>{/if}
            </h3>
            {#if m?.description}<p class="desc">{m.description}</p>{/if}
          </div>
          {#if m}
            <input
              type="checkbox"
              class="switch"
              role="switch"
              aria-label="Turn on {m.name}"
              checked={p.enabled}
              onchange={(e) => {
                if (!e.currentTarget.checked) {
                  disablePlugin(p.id);
                  reviewing = null;
                } else {
                  // Turned on only once the person has read what it can do.
                  e.currentTarget.checked = false;
                  reviewing = p.id;
                }
              }}
            />
          {/if}
        </div>

        {#if p.error}
          <p class="callout danger" role="alert">
            <Icon name="alert" size={14} /><span>{p.error}</span>
          </p>
        {/if}
        {#if p.changed}
          <p class="callout warn" role="status">
            <Icon name="alert" size={14} /><span
              >Its version, permissions or files have changed since you turned it on, so it is off.
              Turn it on again to review it.</span
            >
          </p>
        {/if}

        {#if m && (p.enabled || reviewing === p.id)}
          <div class="details">
            <section>
              <h4>Can</h4>
              <ul class="list">
                {#each m.permissions as perm (perm)}
                  <li class="perm">
                    <Icon name="check" size={13} /><span>{PERMISSIONS[perm]}</span>
                  </li>
                {/each}
                {#if !m.permissions.length}
                  <li class="none"><span>Nothing beyond drawing its own page</span></li>
                {/if}
              </ul>
            </section>

            {#if m.actions.length}
              <section>
                <h4>Actions</h4>
                <ul class="list">
                  {#each m.actions as a (a.id)}
                    <li>
                      <span class="what">{a.title}</span>
                      {#each a.extensions as ext (ext)}<span class="chip">{dotted(ext)}</span
                        >{/each}
                      {#if !a.extensions.length}<span class="chip">{a.kinds.join(", ")}</span>{/if}
                    </li>
                  {/each}
                </ul>
              </section>
            {/if}

            {#if m.previews.length}
              <section>
                <h4>Previews</h4>
                <ul class="list">
                  {#each m.previews as v (v.id)}
                    <li>
                      <span class="what">{v.title}</span>
                      {#each v.extensions as ext (ext)}<span class="chip">{dotted(ext)}</span
                        >{/each}
                    </li>
                  {/each}
                </ul>
              </section>
            {/if}
          </div>
        {/if}

        {#if m && reviewing === p.id}
          <div class="review" role="group" aria-label="Turn on {m.name}">
            <span>Allow {m.name} to do the above?</span>
            <div class="buttons">
              <button
                class="primary"
                onclick={() => {
                  enablePlugin(m, p.digest);
                  reviewing = null;
                }}>Allow and turn on</button
              >
              <button onclick={() => (reviewing = null)}>Cancel</button>
            </div>
          </div>
        {/if}
      </article>
    {/each}

    {#if plugins.loaded && !plugins.list.length}
      <EmptyState
        icon="plugin"
        text="No plugins yet. Put a plugin folder in the plugins folder, then Reload."
        action="Open folder"
        onaction={() => openFolder()}
      />
    {/if}
  </div>
</section>

<style>
  .plugins {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    background: var(--bg-window);
    outline: none;
  }

  .page {
    width: min(640px, 100%);
    margin: 0 auto;
    padding: var(--space-xl) var(--space-lg) var(--space-3xl);
    box-sizing: border-box;
  }

  header {
    display: flex;
    flex-wrap: wrap;
    justify-content: space-between;
    align-items: center;
    gap: var(--space-sm) var(--space-lg);
  }

  h2 {
    margin: 0;
    font-size: var(--font-size-title);
    font-weight: var(--font-weight-semibold);
  }

  .lead {
    margin: var(--space-sm) 0 var(--space-lg);
    color: var(--text-tertiary);
    font-size: var(--font-size-caption);
    line-height: 1.5;
  }

  code {
    font-family: var(--font-mono);
  }

  .buttons {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-xs);
  }

  .buttons button {
    padding: var(--space-2xs) var(--space-sm);
    border-radius: var(--radius-sm);
    background: var(--bg-control);
  }

  .buttons button:hover {
    background: var(--bg-hover);
  }

  .buttons button.primary {
    background: var(--accent-default);
    color: var(--text-on-accent);
  }

  .buttons button.primary:hover {
    background: var(--accent-hover);
  }

  /* One card for each plugin. */
  .card {
    display: grid;
    gap: var(--space-md);
    margin-bottom: var(--space-md);
    padding: var(--space-lg);
    border: 1px solid var(--border-default);
    border-radius: var(--radius-lg);
    background: var(--bg-subtle);
  }

  .card.reviewing {
    border-color: var(--accent-default);
  }

  .top {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    align-items: start;
    column-gap: var(--space-lg);
  }

  .who {
    min-width: 0;
  }

  h3 {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: var(--space-2xs) var(--space-sm);
    margin: 0;
    font-size: var(--font-size-body);
    font-weight: var(--font-weight-semibold);
  }

  .name {
    min-width: 0;
    overflow-wrap: anywhere;
  }

  .version {
    color: var(--text-tertiary);
    font-size: var(--font-size-caption);
    font-weight: var(--font-weight-regular);
  }

  .desc {
    margin: var(--space-xs) 0 0;
    color: var(--text-secondary);
    font-size: var(--font-size-caption);
    line-height: 1.5;
    overflow-wrap: anywhere;
  }

  /* A message that belongs to the card: coloured edge and icon, never colour alone. */
  .callout {
    display: flex;
    align-items: flex-start;
    gap: var(--space-sm);
    margin: 0;
    padding: var(--space-sm) var(--space-md);
    border-left: 3px solid var(--text-tertiary);
    border-radius: var(--radius-sm);
    background: var(--bg-window);
    color: var(--text-secondary);
    font-size: var(--font-size-caption);
    line-height: 1.5;
    overflow-wrap: anywhere;
  }

  .callout :global(svg) {
    flex-shrink: 0;
    margin-top: 1px;
  }

  .callout.danger {
    border-left-color: var(--status-danger);
  }

  .callout.danger :global(svg) {
    color: var(--status-danger);
  }

  .callout.warn {
    border-left-color: var(--status-warning);
  }

  .callout.warn :global(svg) {
    color: var(--status-warning);
  }

  .details {
    display: grid;
    gap: var(--space-md);
    padding-top: var(--space-md);
    border-top: 1px solid var(--border-default);
  }

  h4 {
    margin: 0 0 var(--space-xs);
    color: var(--text-tertiary);
    font-size: var(--font-size-caption);
    font-weight: var(--font-weight-semibold);
    letter-spacing: 0.4px;
    text-transform: uppercase;
  }

  .list {
    display: grid;
    gap: var(--space-xs);
    margin: 0;
    padding: 0;
    list-style: none;
    color: var(--text-secondary);
    font-size: var(--font-size-caption);
  }

  .list li {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-xs) var(--space-sm);
    min-width: 0;
  }

  .list li :global(svg) {
    flex-shrink: 0;
    color: var(--status-success);
  }

  /* The tick stays beside the first line of a permission that wraps. */
  .list li.perm {
    flex-wrap: nowrap;
    align-items: flex-start;
  }

  .list li.perm :global(svg) {
    margin-top: 1px;
  }

  .list li.none {
    color: var(--text-tertiary);
  }

  .what {
    color: var(--text-primary);
    overflow-wrap: anywhere;
  }

  .chip {
    padding: 0 var(--space-sm);
    border-radius: var(--radius-full);
    background: var(--bg-control);
    color: var(--text-secondary);
    font-family: var(--font-mono);
    font-size: var(--font-size-caption);
    line-height: 18px;
  }

  /* The question and its buttons, in the card of the plugin they are about. */
  .review {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-sm) var(--space-lg);
    padding-top: var(--space-md);
    border-top: 1px solid var(--border-default);
    font-size: var(--font-size-caption);
    color: var(--text-primary);
  }

  .switch {
    appearance: none;
    position: relative;
    flex-shrink: 0;
    width: 36px;
    height: 20px;
    margin: 0;
    border-radius: 10px;
    background: var(--border-strong);
    transition: background var(--duration-fast) var(--ease-standard);
  }

  .switch::after {
    content: "";
    position: absolute;
    top: 2px;
    left: 2px;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: var(--bg-window);
    transition: translate var(--duration-fast) var(--ease-standard);
  }

  .switch:checked {
    background: var(--accent-default);
  }

  .switch:checked::after {
    translate: 16px 0;
  }
</style>
