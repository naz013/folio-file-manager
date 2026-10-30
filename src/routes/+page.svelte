<script lang="ts">
  import { onMount } from "svelte";
  import { homeDir, listDir, type Entry } from "$lib/api/fs";
  import Sidebar from "$lib/components/Sidebar.svelte";
  import Toolbar, { type View } from "$lib/components/Toolbar.svelte";
  import FileView from "$lib/components/FileView.svelte";

  let home = $state("");
  let path = $state("");
  let entries = $state<Entry[]>([]);
  let error = $state<string | null>(null);
  let view = $state<View>("grid");
  let selected = $state<string | null>(null);
  let showHidden = $state(false);

  let back = $state<string[]>([]);
  let forward = $state<string[]>([]);

  let visible = $derived(showHidden ? entries : entries.filter((e) => !e.hidden));

  async function load(target: string) {
    try {
      entries = await listDir(target);
      path = target;
      selected = null;
      error = null;
    } catch (e) {
      error = String(e);
    }
  }

  function navigate(target: string) {
    if (target === path) return;
    if (path) back.push(path);
    forward = [];
    load(target);
  }

  function goBack() {
    const prev = back.pop();
    if (prev) {
      forward.push(path);
      load(prev);
    }
  }

  function goForward() {
    const next = forward.pop();
    if (next) {
      back.push(path);
      load(next);
    }
  }

  function open(entry: Entry) {
    if (entry.isDir) navigate(entry.path);
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.altKey && e.key === "ArrowLeft") goBack();
    else if (e.altKey && e.key === "ArrowRight") goForward();
    else if (e.ctrlKey && e.key === "h") showHidden = !showHidden;
  }

  onMount(async () => {
    home = await homeDir();
    await load(home);
  });
</script>

<svelte:window {onkeydown} />

<div class="app">
  <Sidebar {home} current={path} onnavigate={navigate} />
  <main class="main">
    <Toolbar
      {path}
      bind:view
      canBack={back.length > 0}
      canForward={forward.length > 0}
      onback={goBack}
      onforward={goForward}
      onnavigate={navigate} />
    <section class="content">
      {#if error}
        <p class="error">{error}</p>
      {/if}
      <FileView entries={visible} {view} bind:selected onopen={open} />
    </section>
    <footer class="status">
      {visible.length} items{selected ? " · 1 selected" : ""}
    </footer>
  </main>
</div>

<style>
  .app {
    display: flex;
    height: 100vh;
  }

  .main {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
    background: var(--bg-window);
  }

  .content {
    flex: 1;
    overflow-y: auto;
  }

  .error {
    margin: 16px 24px 0;
    padding: 10px 12px;
    border-radius: var(--radius-md);
    background: var(--bg-subtle);
    color: var(--status-danger);
  }

  .status {
    display: flex;
    align-items: center;
    height: 30px;
    padding: 0 16px;
    border-top: 1px solid var(--border-default);
    font-size: 11.5px;
    font-weight: var(--font-weight-medium);
    color: var(--text-secondary);
  }
</style>
