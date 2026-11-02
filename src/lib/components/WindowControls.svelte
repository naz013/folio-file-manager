<script lang="ts">
  import { onMount } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import Icon from "./Icon.svelte";

  const appWindow = getCurrentWindow();
  let maximized = $state(false);

  onMount(() => {
    let frame = 0;
    const sync = () => appWindow.isMaximized().then((m) => (maximized = m));
    sync();
    // A resize fires many events; ask for the maximized state at most once per frame.
    const unlisten = appWindow.onResized(() => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(sync);
    });
    return () => {
      cancelAnimationFrame(frame);
      unlisten.then((u) => u());
    };
  });
</script>

<!-- Fixed to the top right of the window, whatever panels open beneath it. -->
<div class="controls">
  <button class="control" aria-label="Minimize" onclick={() => appWindow.minimize()}>
    <Icon name="minimize" size={15} stroke={1.5} />
  </button>
  <button
    class="control"
    aria-label={maximized ? "Restore" : "Maximize"}
    onclick={() => appWindow.toggleMaximize()}
  >
    <Icon name={maximized ? "restore" : "maximize"} size={13} stroke={1.5} />
  </button>
  <button class="control close-window" aria-label="Close window" onclick={() => appWindow.close()}>
    <Icon name="x" size={15} stroke={1.5} />
  </button>
</div>

<style>
  .controls {
    position: absolute;
    top: 0;
    right: 0;
    z-index: 5;
    display: flex;
    height: var(--size-titlebar);
  }

  .control {
    display: grid;
    place-items: center;
    width: 46px;
    height: var(--size-titlebar);
    color: var(--text-secondary);
  }

  .control:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .close-window:hover {
    background: var(--bg-danger);
    color: var(--text-on-danger);
  }
</style>
