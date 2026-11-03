/**
 * Every keyboard shortcut Folio answers to, declared once. The page finds what a key press
 * means by looking it up here (`matchShortcut`), the list of shortcuts is drawn from here, and
 * the command palette takes its hints from here, so a shortcut that works is a shortcut that
 * is listed. Adding one means adding a line below and its handler in `routes/+page.svelte`,
 * which the type checker insists on.
 */

/** One way of pressing a shortcut. */
export interface Chord {
  /** `KeyboardEvent.key`; a letter is written in lower case and matches either case. */
  key: string;
  /** Ctrl, or Cmd on a Mac. Left out: whether it is held doesn't matter. */
  ctrl?: boolean;
  /** Left out: whether it is held doesn't matter. `false`: it must not be held. */
  shift?: boolean;
  alt?: boolean;
}

/**
 * Where a shortcut applies. `tabs`: everywhere, including while a text field has focus.
 * `files`: on the file list, not while renaming or typing. `look`: while Quick Look is open.
 * `pointer`: a mouse or touch gesture, listed here but handled where it happens.
 */
export type Scope = "tabs" | "files" | "look" | "pointer";

export interface Shortcut {
  /** Names the handler in the page. */
  id: string;
  scope: Scope;
  group: string;
  /** How it is written in the list: "Ctrl+C". Where several keys do the same, they are joined. */
  keys: string;
  does: string;
  /** Written as it is on every system: the keys are not Ctrl and Alt in the way a Mac swaps them. */
  plain?: boolean;
  /** What the command palette shows beside it, if shorter than `keys`. */
  hint?: string;
  /** The key presses that mean it, tried in order of the list below. Empty for a gesture. */
  chords: readonly Chord[];
  /** More lines for the list: what the same keys do in another view. */
  also?: readonly { keys: string; does: string }[];
}

/** The chords for a run of digit keys. */
const digits = (from: number, to: number, mods: Omit<Chord, "key"> = {}): Chord[] =>
  Array.from({ length: to - from + 1 }, (_, i) => ({ key: String(from + i), ...mods }));

// Order matters inside a scope: the first shortcut a key press fits is the one that runs, so a
// more particular one (Alt+Left) comes before the plain key it would otherwise be (Left).
export const SHORTCUTS = [
  {
    id: "copy",
    scope: "files",
    group: "Files",
    keys: "Ctrl+C",
    does: "Copy",
    chords: [{ key: "c", ctrl: true, shift: false }],
  },
  {
    id: "cut",
    scope: "files",
    group: "Files",
    keys: "Ctrl+X",
    does: "Cut",
    chords: [{ key: "x", ctrl: true, shift: false }],
  },
  {
    id: "paste",
    scope: "files",
    group: "Files",
    keys: "Ctrl+V",
    does: "Paste",
    chords: [{ key: "v", ctrl: true, shift: false }],
  },
  {
    id: "shelf",
    scope: "files",
    group: "Files",
    keys: "Ctrl+Shift+V",
    does: "Open the clipboard shelf",
    chords: [{ key: "v", ctrl: true, shift: true }],
  },
  {
    id: "selectAll",
    scope: "files",
    group: "Files",
    keys: "Ctrl+A",
    does: "Select all",
    chords: [{ key: "a", ctrl: true, shift: false }],
  },
  {
    id: "newFolder",
    scope: "files",
    group: "Files",
    keys: "Ctrl+Shift+N",
    does: "New folder",
    chords: [{ key: "n", ctrl: true, shift: true }],
  },
  {
    id: "rename",
    scope: "files",
    group: "Files",
    keys: "F2",
    does: "Rename (several files: bulk rename)",
    chords: [{ key: "F2" }],
  },
  {
    id: "trash",
    scope: "files",
    group: "Files",
    keys: "Delete",
    does: "Move to the Recycle Bin",
    chords: [{ key: "Delete" }],
  },
  {
    id: "undo",
    scope: "files",
    group: "Files",
    keys: "Ctrl+Z",
    does: "Undo",
    chords: [{ key: "z", ctrl: true, shift: false }],
  },
  {
    id: "redo",
    scope: "files",
    group: "Files",
    keys: "Ctrl+Y",
    does: "Redo (also Ctrl+Shift+Z)",
    chords: [
      { key: "y", ctrl: true, shift: false },
      { key: "z", ctrl: true, shift: true },
    ],
  },
  {
    id: "tag",
    scope: "files",
    group: "Files",
    keys: "1 – 7",
    does: "Toggle a color tag on the selection",
    chords: digits(1, 7),
  },
  {
    id: "menu",
    scope: "files",
    group: "Files",
    keys: "Shift+F10",
    does: "Context menu (also the Menu key)",
    chords: [{ key: "F10", shift: true }, { key: "ContextMenu" }],
  },
  {
    id: "clear",
    scope: "files",
    group: "Files",
    keys: "Esc",
    does: "Clear the selection and clipboard marks",
    chords: [{ key: "Escape" }],
  },

  {
    id: "back",
    scope: "files",
    group: "Moving around",
    keys: "Alt+←",
    does: "Back",
    chords: [{ key: "ArrowLeft", alt: true }],
  },
  {
    id: "forward",
    scope: "files",
    group: "Moving around",
    keys: "Alt+→",
    does: "Forward",
    chords: [{ key: "ArrowRight", alt: true }],
  },
  {
    id: "up",
    scope: "files",
    group: "Moving around",
    keys: "Alt+↑",
    does: "Up one folder",
    chords: [{ key: "ArrowUp", alt: true }],
  },
  {
    id: "upBackspace",
    scope: "files",
    group: "Moving around",
    keys: "Backspace",
    does: "Up one folder",
    chords: [{ key: "Backspace" }],
  },
  {
    id: "move",
    scope: "files",
    group: "Moving around",
    keys: "Arrow keys",
    does: "Move the selection (Shift extends it)",
    chords: [{ key: "ArrowDown" }, { key: "ArrowUp" }, { key: "ArrowLeft" }, { key: "ArrowRight" }],
    also: [{ keys: "→  /  ←", does: "Column view: into the folder, or back to the one above" }],
  },
  {
    id: "ends",
    scope: "files",
    group: "Moving around",
    keys: "Home / End",
    does: "First or last item",
    chords: [{ key: "Home" }, { key: "End" }],
  },
  {
    id: "open",
    scope: "files",
    group: "Moving around",
    keys: "Enter",
    does: "Open",
    chords: [{ key: "Enter" }],
  },
  {
    id: "quickLook",
    scope: "files",
    group: "Moving around",
    keys: "Space",
    does: "Quick Look",
    chords: [{ key: " " }],
  },
  {
    id: "goTo",
    scope: "tabs",
    group: "Moving around",
    keys: "Ctrl+L",
    does: "Go to a path",
    chords: [{ key: "l", ctrl: true, shift: false }],
  },
  {
    id: "search",
    scope: "tabs",
    group: "Moving around",
    keys: "Ctrl+F",
    does: "Search the folder and its subfolders",
    chords: [{ key: "f", ctrl: true, shift: false, alt: false }],
  },
  {
    id: "palette",
    scope: "tabs",
    group: "Moving around",
    keys: "Ctrl+K",
    does: "Command palette",
    chords: [{ key: "k", ctrl: true, shift: false, alt: false }],
  },

  {
    id: "tabNew",
    scope: "tabs",
    group: "Tabs",
    keys: "Ctrl+T",
    does: "New tab",
    chords: [{ key: "t", ctrl: true }],
  },
  {
    id: "tabClose",
    scope: "tabs",
    group: "Tabs",
    keys: "Ctrl+W",
    does: "Close tab",
    chords: [{ key: "w", ctrl: true }],
  },
  {
    id: "tabCycle",
    scope: "tabs",
    group: "Tabs",
    keys: "Ctrl+Tab  /  Ctrl+Shift+Tab",
    does: "Next or previous tab",
    plain: true,
    chords: [{ key: "Tab", ctrl: true }],
  },
  {
    id: "tabPage",
    scope: "tabs",
    group: "Tabs",
    keys: "Ctrl+PageDown  /  Ctrl+PageUp",
    does: "Next or previous tab",
    plain: true,
    chords: [
      { key: "PageDown", ctrl: true },
      { key: "PageUp", ctrl: true },
    ],
  },
  {
    id: "tabGo",
    scope: "tabs",
    group: "Tabs",
    keys: "Ctrl+1 – 9",
    does: "Go to a tab (9 is the last)",
    chords: digits(1, 9, { ctrl: true, shift: false }),
  },

  {
    id: "hidden",
    scope: "files",
    group: "Window",
    keys: "Ctrl+H",
    does: "Show or hide hidden files",
    chords: [{ key: "h", ctrl: true, shift: false }],
  },
  {
    id: "inspector",
    scope: "tabs",
    group: "Window",
    keys: "Ctrl+I",
    does: "Show or hide the inspector",
    chords: [{ key: "i", ctrl: true, shift: false, alt: false }],
  },
  {
    id: "settings",
    scope: "tabs",
    group: "Window",
    keys: "Ctrl+,",
    does: "Settings",
    chords: [{ key: ",", ctrl: true, shift: false }],
  },
  {
    id: "shortcuts",
    scope: "tabs",
    group: "Window",
    keys: "F1  /  Ctrl+/",
    hint: "F1",
    does: "Keyboard shortcuts",
    chords: [
      { key: "F1", shift: false, alt: false },
      { key: "/", ctrl: true },
    ],
  },
  {
    id: "help",
    scope: "tabs",
    group: "Window",
    keys: "Shift+F1",
    does: "Help",
    chords: [{ key: "F1", shift: true, alt: false }],
  },

  {
    id: "lookStep",
    scope: "look",
    group: "Quick Look",
    keys: "←  /  →",
    does: "Previous or next file",
    chords: [
      { key: "ArrowLeft", ctrl: false, alt: false },
      { key: "ArrowUp", ctrl: false, alt: false },
      { key: "ArrowRight", ctrl: false, alt: false },
      { key: "ArrowDown", ctrl: false, alt: false },
    ],
  },
  {
    id: "lookClose",
    scope: "look",
    group: "Quick Look",
    keys: "Space  /  Esc",
    does: "Close",
    chords: [
      { key: " ", ctrl: false, alt: false },
      { key: "Escape", ctrl: false, alt: false },
    ],
  },
  {
    id: "lookZoom",
    scope: "pointer",
    group: "Quick Look",
    keys: "Pinch  /  Ctrl+scroll",
    does: "Zoom a picture",
    // Quick Look reads Ctrl itself, not Cmd.
    plain: true,
    chords: [],
  },
  {
    id: "lookPan",
    scope: "pointer",
    group: "Quick Look",
    keys: "Two-finger scroll  /  drag",
    does: "Move a zoomed picture",
    chords: [],
  },
  {
    id: "lookFit",
    scope: "pointer",
    group: "Quick Look",
    keys: "Double-click",
    does: "Fit the picture, or show it at full size",
    chords: [],
  },
] as const satisfies readonly Shortcut[];

type Declared = (typeof SHORTCUTS)[number];
export type ShortcutId = Declared["id"];
/** The ids of the shortcuts a scope answers, which the page must have a handler for. */
export type ScopeId<S extends Scope> = Extract<Declared, { scope: S }>["id"];

/** The parts of a key event a shortcut looks at (so it can be tried without a browser). */
export interface KeyLike {
  key: string;
  ctrlKey: boolean;
  metaKey: boolean;
  shiftKey: boolean;
  altKey: boolean;
}

/** Whether the key press is this chord: what is required is held, what is barred is not. */
export function chordMatches(chord: Chord, e: KeyLike): boolean {
  const key = e.key.length === 1 ? e.key.toLowerCase() : e.key;
  if (key !== chord.key) return false;
  if (chord.ctrl !== undefined && (e.ctrlKey || e.metaKey) !== chord.ctrl) return false;
  if (chord.shift !== undefined && e.shiftKey !== chord.shift) return false;
  if (chord.alt !== undefined && e.altKey !== chord.alt) return false;
  return true;
}

/** The first shortcut in `scopes` the key press is, if any. */
export function matchShortcut<S extends Scope>(
  e: KeyLike,
  ...scopes: S[]
): Extract<Declared, { scope: S }> | null {
  const found = SHORTCUTS.find(
    (s) =>
      (scopes as Scope[]).includes(s.scope) &&
      (s.chords as readonly Chord[]).some((c) => chordMatches(c, e)),
  );
  return (found ?? null) as Extract<Declared, { scope: S }> | null;
}

/** Whether keys are written for a Mac: Cmd and Option in place of Ctrl and Alt. */
export const onMac = () => typeof navigator !== "undefined" && /Mac/i.test(navigator.userAgent);

/** Writes the keys for this system. */
export function forSystem(keys: string, mac = onMac()): string {
  return mac ? keys.replace(/\bCtrl\b/g, "⌘").replace(/\bAlt\b/g, "⌥") : keys;
}

const byId = new Map<string, Shortcut>(SHORTCUTS.map((s) => [s.id, s]));

/** `keys` of `shortcut` as shown: for this system, unless the shortcut is the same everywhere. */
const written = (shortcut: Shortcut, keys: string, mac = onMac()) =>
  shortcut.plain ? keys : forSystem(keys, mac);

/** What the command palette shows beside an action for the shortcut with this id. */
export function hintFor(id: ShortcutId): string {
  const s = byId.get(id);
  return s ? written(s, s.hint ?? s.keys) : "";
}

export interface ShortcutGroup {
  title: string;
  items: { keys: string; does: string }[];
}

/** The shortcuts by group, in the order they are declared, for the list. */
export function shortcutGroups(mac = onMac()): ShortcutGroup[] {
  const groups = new Map<string, ShortcutGroup>();
  for (const s of SHORTCUTS) {
    let group = groups.get(s.group);
    if (!group) groups.set(s.group, (group = { title: s.group, items: [] }));
    group.items.push({ keys: written(s, s.keys, mac), does: s.does });
    for (const more of "also" in s ? s.also : []) {
      group.items.push({ keys: written(s, more.keys, mac), does: more.does });
    }
  }
  return [...groups.values()];
}
