// Tests for the shortcut registry (src/lib/shortcuts.ts). Run with `npm run test:js`; Node reads
// the TypeScript directly, so this needs no build and no test framework.
import assert from "node:assert/strict";
import { test } from "node:test";
import {
  SHORTCUTS,
  chordMatches,
  forSystem,
  hintFor,
  matchShortcut,
  shortcutGroups,
} from "../../src/lib/shortcuts.ts";

/** A key press: `press("Ctrl+Shift+n")`, `press("ArrowLeft")`, `press("Alt+ArrowLeft")`. */
function press(text) {
  const parts = text.split("+");
  // A "+" key is written "Ctrl++".
  const key = text.endsWith("++") ? "+" : parts.at(-1);
  const mods = text.endsWith("++") ? parts.slice(0, -2) : parts.slice(0, -1);
  return {
    key,
    ctrlKey: mods.includes("Ctrl"),
    metaKey: mods.includes("Cmd"),
    shiftKey: mods.includes("Shift"),
    altKey: mods.includes("Alt"),
  };
}

const idOf = (text, ...scopes) => matchShortcut(press(text), ...scopes)?.id ?? null;
const FILES = ["files"];

test("every shortcut has a unique id, a group, and words for the list", () => {
  const ids = SHORTCUTS.map((s) => s.id);
  assert.equal(new Set(ids).size, ids.length, "ids are unique");
  for (const s of SHORTCUTS) {
    assert.ok(s.group && s.keys && s.does, `${s.id} is described`);
    // A gesture has no key press; everything else has at least one.
    assert.equal(s.chords.length === 0, s.scope === "pointer", `${s.id} chords`);
  }
});

test("no key press means two shortcuts of one scope", () => {
  // Every chord, written as the key press that satisfies it, must find its own shortcut first.
  for (const s of SHORTCUTS) {
    for (const c of s.chords) {
      const e = {
        key: c.key,
        ctrlKey: c.ctrl === true,
        metaKey: false,
        shiftKey: c.shift === true,
        altKey: c.alt === true,
      };
      const found = matchShortcut(e, s.scope);
      assert.equal(found?.id, s.id, `${s.id} (${c.key}) is shadowed by ${found?.id}`);
    }
  }
});

test("tab shortcuts need Ctrl, except the help key", () => {
  assert.equal(idOf("Ctrl+t", "tabs"), "tabNew");
  assert.equal(idOf("Ctrl+T", "tabs"), "tabNew", "caps lock or shift still names the letter");
  assert.equal(idOf("t", "tabs"), null);
  assert.equal(idOf("Ctrl+w", "tabs"), "tabClose");
  assert.equal(idOf("Ctrl+Tab", "tabs"), "tabCycle");
  assert.equal(idOf("Ctrl+Shift+Tab", "tabs"), "tabCycle");
  assert.equal(idOf("Ctrl+PageDown", "tabs"), "tabPage");
  assert.equal(idOf("Ctrl+9", "tabs"), "tabGo");
  assert.equal(idOf("Ctrl+Shift+9", "tabs"), null);
  assert.equal(idOf("Ctrl+k", "tabs"), "palette");
  assert.equal(idOf("Ctrl+Shift+k", "tabs"), null);
  assert.equal(idOf("Ctrl+Alt+k", "tabs"), null);
  assert.equal(idOf("Ctrl+,", "tabs"), "settings");
  assert.equal(idOf("Ctrl+f", "tabs"), "search");
  assert.equal(idOf("Ctrl+l", "tabs"), "goTo");
  assert.equal(idOf("F1", "tabs"), "shortcuts");
  assert.equal(idOf("Ctrl+/", "tabs"), "shortcuts");
  assert.equal(idOf("Cmd+t", "tabs"), "tabNew", "Cmd counts as Ctrl");
});

test("file shortcuts tell Ctrl+Shift combinations apart", () => {
  assert.equal(idOf("Ctrl+c", ...FILES), "copy");
  assert.equal(idOf("Ctrl+Shift+c", ...FILES), null, "left alone");
  assert.equal(idOf("Ctrl+v", ...FILES), "paste");
  assert.equal(idOf("Ctrl+Shift+v", ...FILES), "shelf");
  assert.equal(idOf("Ctrl+Shift+n", ...FILES), "newFolder");
  assert.equal(idOf("Ctrl+n", ...FILES), null);
  assert.equal(idOf("Ctrl+a", ...FILES), "selectAll");
  assert.equal(idOf("Ctrl+Shift+a", ...FILES), null);
  assert.equal(idOf("Ctrl+z", ...FILES), "undo");
  assert.equal(idOf("Ctrl+Shift+z", ...FILES), "redo");
  assert.equal(idOf("Ctrl+y", ...FILES), "redo");
  assert.equal(idOf("Ctrl+Shift+y", ...FILES), null);
  assert.equal(idOf("Ctrl+h", ...FILES), "hidden");
});

test("Alt+arrows go back, forward and up; the plain arrows move", () => {
  assert.equal(idOf("Alt+ArrowLeft", ...FILES), "back");
  assert.equal(idOf("Alt+ArrowRight", ...FILES), "forward");
  assert.equal(idOf("Alt+ArrowUp", ...FILES), "up");
  assert.equal(idOf("ArrowLeft", ...FILES), "move");
  assert.equal(idOf("Shift+ArrowDown", ...FILES), "move", "Shift extends the selection");
  // Other Alt combinations act like the plain key, as they always have.
  assert.equal(idOf("Alt+ArrowDown", ...FILES), "move");
  assert.equal(idOf("Alt+Backspace", ...FILES), "upBackspace");
});

test("plain file keys", () => {
  for (const [key, id] of [
    ["Enter", "open"],
    [" ", "quickLook"],
    ["F2", "rename"],
    ["Delete", "trash"],
    ["Escape", "clear"],
    ["Home", "ends"],
    ["End", "ends"],
    ["Backspace", "upBackspace"],
    ["ContextMenu", "menu"],
    ["Shift+F10", "menu"],
    ["1", "tag"],
    ["7", "tag"],
  ]) {
    assert.equal(idOf(key, ...FILES), id, key);
  }
  assert.equal(idOf("8", ...FILES), null);
  assert.equal(idOf("F10", ...FILES), null, "F10 alone is not the menu");
  assert.equal(idOf("q", ...FILES), null);
});

test("scopes keep to themselves", () => {
  assert.equal(idOf("Ctrl+t", ...FILES), null);
  assert.equal(idOf("Ctrl+c", "tabs"), null);
  // Ctrl+1 is a tab; the colour tag key is the plain 1.
  assert.equal(idOf("Ctrl+1", "tabs"), "tabGo");
  assert.equal(idOf("1", "tabs"), null);
});

test("Quick Look keys", () => {
  assert.equal(idOf("ArrowLeft", "look"), "lookStep");
  assert.equal(idOf("ArrowDown", "look"), "lookStep");
  assert.equal(idOf("Ctrl+ArrowLeft", "look"), null);
  assert.equal(idOf(" ", "look"), "lookClose");
  assert.equal(idOf("Escape", "look"), "lookClose");
  assert.equal(idOf("Alt+Escape", "look"), null);
});

test("a chord's modifiers: required, barred, or ignored", () => {
  const e = (extra) => ({
    key: "x",
    ctrlKey: false,
    metaKey: false,
    shiftKey: false,
    altKey: false,
    ...extra,
  });
  assert.ok(chordMatches({ key: "x" }, e({ ctrlKey: true, shiftKey: true })), "ignored");
  assert.ok(!chordMatches({ key: "x", ctrl: true }, e()), "required");
  assert.ok(!chordMatches({ key: "x", shift: false }, e({ shiftKey: true })), "barred");
  assert.ok(chordMatches({ key: "x", ctrl: true }, e({ metaKey: true })), "Cmd is Ctrl");
});

test("the list is drawn from the registry, with every shortcut and its extra lines", () => {
  const groups = shortcutGroups(false);
  assert.deepEqual(
    groups.map((g) => g.title),
    ["Files", "Moving around", "Tabs", "Window", "Quick Look"],
  );
  const rows = groups.flatMap((g) => g.items);
  const extra = SHORTCUTS.reduce((n, s) => n + ("also" in s ? s.also.length : 0), 0);
  assert.equal(rows.length, SHORTCUTS.length + extra);
  assert.ok(rows.some((r) => r.keys === "F1  /  Ctrl+/"));
});

test("keys are written for the system", () => {
  assert.equal(forSystem("Ctrl+Shift+N", true), "⌘+Shift+N");
  assert.equal(forSystem("Alt+←", true), "⌥+←");
  assert.equal(forSystem("Ctrl+Shift+N", false), "Ctrl+Shift+N");
  assert.equal(forSystem("Ctrlish", true), "Ctrlish", "only whole words");
  // The palette hint is the short form where there is one.
  assert.equal(hintFor("shortcuts"), forSystem("F1"));
  assert.equal(hintFor("copy"), forSystem("Ctrl+C"));
});

test("on a Mac the list says Cmd and Option, except where the keys really are Ctrl", () => {
  const rows = shortcutGroups(true).flatMap((g) => g.items);
  const keysOf = (does) => rows.find((r) => r.does === does)?.keys;
  assert.equal(keysOf("Copy"), "⌘+C");
  assert.equal(keysOf("Back"), "⌥+←");
  // Cycling tabs and zooming by scroll are Control, not Command, on a Mac.
  assert.equal(keysOf("Zoom a picture"), "Pinch  /  Ctrl+scroll");
  assert.ok(rows.some((r) => r.keys === "Ctrl+Tab  /  Ctrl+Shift+Tab"));
  assert.equal(keysOf("Keyboard shortcuts"), "F1  /  ⌘+/");
  assert.equal(matchShortcut(press("Shift+F1"), "tabs"), null, "F1 alone");
});
