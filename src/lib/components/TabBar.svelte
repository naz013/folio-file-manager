<script lang="ts">
  import { onMount } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import Icon from "./Icon.svelte";
  import FolderIcon from "./FolderIcon.svelte";

  let {
    tabs,
    active,
    onselect,
    onclose,
    onnew,
    onmove,
    ondetach,
  }: {
    tabs: { id: number; title: string }[];
    active: number;
    onselect: (index: number) => void;
    onclose: (index: number) => void;
    onnew: () => void;
    onmove: (from: number, to: number) => void;
    /** Called when a tab is dropped outside the strip; (x, y) is the logical screen position. */
    ondetach: (index: number, x: number, y: number) => void;
  } = $props();

  /** Pixels the pointer must move before a press becomes a drag. */
  const DRAG_THRESHOLD = 5;
  /** How far outside the strip (vertically) a dragged tab tears off into a new window. */
  const DETACH_DISTANCE = 40;

  const appWindow = getCurrentWindow();
  let maximized = $state(false);

  let strip: HTMLDivElement;
  let drag = $state<{
    index: number;
    x: number;
    y: number;
    moved: boolean;
    outside: boolean;
  } | null>(null);

  function pointerDown(e: PointerEvent, index: number) {
    if (e.button !== 0) return;
    onselect(index);
    drag = { index, x: e.clientX, y: e.clientY, moved: false, outside: false };
    // Capture on the strip, not the tab: reordering moves tab nodes, which would drop capture.
    strip.setPointerCapture(e.pointerId);
  }

  function pointerMove(e: PointerEvent) {
    if (!drag) return;
    if (!drag.moved && Math.hypot(e.clientX - drag.x, e.clientY - drag.y) < DRAG_THRESHOLD) return;
    drag.moved = true;
    drag.outside = isOutside(e);
    if (drag.outside) return;
    // Reorder live: the dragged tab takes the slot under the pointer.
    const rects = [...strip.querySelectorAll<HTMLElement>(".tab")].map((el) =>
      el.getBoundingClientRect(),
    );
    const target = rects.findIndex((r) => e.clientX >= r.left && e.clientX < r.right);
    if (target >= 0 && target !== drag.index) {
      onmove(drag.index, target);
      drag.index = target;
    }
  }

  function pointerUp(e: PointerEvent) {
    if (drag?.moved && isOutside(e) && tabs.length > 1) {
      // Put the new window's tab strip under the pointer, roughly where the tab was grabbed.
      ondetach(drag.index, e.screenX - 120, e.screenY - 20);
    }
    drag = null;
  }

  function isOutside(e: PointerEvent): boolean {
    const r = strip.getBoundingClientRect();
    return (
      e.clientY < r.top - DETACH_DISTANCE ||
      e.clientY > r.bottom + DETACH_DISTANCE ||
      e.clientX < 0 ||
      e.clientX > window.innerWidth
    );
  }

  function auxClick(e: MouseEvent, index: number) {
    if (e.button === 1) onclose(index);
  }

  /**
   * Clicking a tab must not focus it, or arrows and Delete would keep acting on tabs
   * instead of files. Also stops the middle-button autoscroll from starting.
   */
  function noFocusDown(e: MouseEvent) {
    e.preventDefault();
  }

  /** Tab list keyboard pattern: arrows move between tabs, Delete closes. */
  function tabKey(e: KeyboardEvent, index: number) {
    const last = tabs.length - 1;
    const to =
      e.key === "ArrowLeft"
        ? index > 0
          ? index - 1
          : last
        : e.key === "ArrowRight"
          ? index < last
            ? index + 1
            : 0
          : e.key === "Home"
            ? 0
            : e.key === "End"
              ? last
              : -1;
    // Stop propagation: the window-level handler would move the file cursor on arrows
    // and move the selected files to the Recycle Bin on Delete.
    if (to >= 0) {
      e.preventDefault();
      e.stopPropagation();
      onselect(to);
    } else if (e.key === "Delete") {
      e.preventDefault();
      e.stopPropagation();
      onclose(index);
    }
  }

  // Keep the active tab visible and focused-in-place when the strip overflows.
  $effect(() => {
    const el = strip?.children[active] as HTMLElement | undefined;
    el?.scrollIntoView({ block: "nearest", inline: "nearest" });
    if (strip?.contains(document.activeElement)) el?.focus();
  });

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

<div class="tabbar" data-tauri-drag-region>
  <div
    class="strip"
    bind:this={strip}
    role="tablist"
    aria-label="Tabs"
    tabindex="-1"
    onpointermove={pointerMove}
    onpointerup={pointerUp}
    onpointercancel={() => (drag = null)}
    onlostpointercapture={() => (drag = null)}
  >
    {#each tabs as tab, index (tab.id)}
      <div
        class="tab"
        class:active={index === active}
        class:detaching={drag?.index === index && drag.outside}
        role="tab"
        tabindex={index === active ? 0 : -1}
        aria-selected={index === active}
        title={tab.title}
        onpointerdown={(e) => pointerDown(e, index)}
        onkeydown={(e) => tabKey(e, index)}
        onmousedown={noFocusDown}
        onauxclick={(e) => auxClick(e, index)}
      >
        <FolderIcon size={16} />
        <span class="label">{tab.title}</span>
        <!-- Not in the tab order: Delete or Ctrl+W close the focused tab from the keyboard. -->
        <button
          class="close"
          tabindex="-1"
          aria-label="Close tab"
          onpointerdown={(e) => e.stopPropagation()}
          onclick={() => onclose(index)}
        >
          <Icon name="x" size={14} stroke={2} />
        </button>
      </div>
    {/each}
  </div>
  <button class="new" aria-label="New tab (Ctrl+T)" onclick={onnew}>
    <Icon name="plus" />
  </button>
  <div class="spacer" data-tauri-drag-region></div>
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
  .tabbar {
    display: flex;
    align-items: center;
    gap: 4px;
    height: 46px;
    flex-shrink: 0;
    padding-left: 8px;
    background: var(--bg-sidebar);
    overflow: hidden;
  }

  .strip {
    display: flex;
    align-items: center;
    gap: 4px;
    min-width: 0;
    /* Too many tabs scroll inside the strip instead of covering "+" and the window controls.
       Vertical padding keeps the active tab's shadow from being clipped. */
    padding: 4px 2px;
    overflow-x: auto;
    scrollbar-width: none;
    outline: none;
  }

  .tab {
    display: flex;
    align-items: center;
    gap: 8px;
    flex: 0 1 200px;
    min-width: 72px;
    height: 34px;
    padding: 0 8px 0 12px;
    border-radius: 9px;
    color: var(--text-secondary);
    font-size: 12.5px;
    cursor: default;
    user-select: none;
  }

  .tab:hover:not(.active) {
    background: var(--bg-hover);
  }

  .tab:focus-visible {
    outline: 2px solid var(--accent-default);
    outline-offset: -2px;
  }

  .tab.active {
    background: var(--bg-window);
    box-shadow: var(--shadow-raised);
    color: var(--text-primary);
    font-weight: var(--font-weight-medium);
  }

  .tab.detaching {
    opacity: 0.5;
  }

  .label {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .close {
    display: grid;
    place-items: center;
    width: 18px;
    height: 18px;
    flex-shrink: 0;
    border-radius: var(--radius-xs);
    color: var(--text-secondary);
    opacity: 0;
  }

  .tab.active .close,
  .tab:hover .close {
    opacity: 1;
  }

  .close:hover {
    background: var(--bg-control);
    color: var(--text-primary);
  }

  .new {
    display: grid;
    place-items: center;
    width: 28px;
    height: 28px;
    flex-shrink: 0;
    border-radius: var(--radius-md);
    color: var(--text-secondary);
  }

  .new:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .spacer {
    flex: 1;
    align-self: stretch;
    min-width: 46px;
  }

  .control {
    display: grid;
    place-items: center;
    width: 46px;
    height: 46px;
    flex-shrink: 0;
    color: var(--text-secondary);
  }

  .control:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .close-window:hover {
    background: var(--status-danger);
    color: var(--text-on-accent);
  }
</style>
