/** The keyboard shortcuts Folio answers to, for the list in Settings. Keep in step with the
 *  handlers in `routes/+page.svelte`. */
export interface ShortcutGroup {
  title: string;
  items: { keys: string; does: string }[];
}

export const shortcutGroups: ShortcutGroup[] = [
  {
    title: "Files",
    items: [
      { keys: "Ctrl+C", does: "Copy" },
      { keys: "Ctrl+X", does: "Cut" },
      { keys: "Ctrl+V", does: "Paste" },
      { keys: "Ctrl+Shift+V", does: "Open the clipboard shelf" },
      { keys: "Ctrl+A", does: "Select all" },
      { keys: "Ctrl+Shift+N", does: "New folder" },
      { keys: "F2", does: "Rename (several files: bulk rename)" },
      { keys: "Delete", does: "Move to the Recycle Bin" },
      { keys: "Ctrl+Z", does: "Undo" },
      { keys: "Ctrl+Y", does: "Redo (also Ctrl+Shift+Z)" },
      { keys: "1 – 7", does: "Toggle a color tag on the selection" },
      { keys: "Shift+F10", does: "Context menu (also the Menu key)" },
      { keys: "Esc", does: "Clear the selection and clipboard marks" },
    ],
  },
  {
    title: "Moving around",
    items: [
      { keys: "Arrow keys", does: "Move the selection (Shift extends it)" },
      { keys: "Home / End", does: "First or last item" },
      { keys: "Enter", does: "Open" },
      { keys: "Space", does: "Quick Look" },
      { keys: "Backspace", does: "Up one folder" },
      { keys: "Alt+←  /  Alt+→", does: "Back and forward" },
      { keys: "Alt+↑", does: "Up one folder" },
      { keys: "Ctrl+L", does: "Go to a path" },
      { keys: "Ctrl+F", does: "Search the folder and its subfolders" },
      { keys: "Ctrl+K", does: "Command palette" },
    ],
  },
  {
    title: "Tabs",
    items: [
      { keys: "Ctrl+T", does: "New tab" },
      { keys: "Ctrl+W", does: "Close tab" },
      { keys: "Ctrl+Tab  /  Ctrl+Shift+Tab", does: "Next or previous tab" },
      { keys: "Ctrl+PageDown  /  Ctrl+PageUp", does: "Next or previous tab" },
      { keys: "Ctrl+1 – 9", does: "Go to a tab (9 is the last)" },
    ],
  },
  {
    title: "Window",
    items: [
      { keys: "Ctrl+H", does: "Show or hide hidden files" },
      { keys: "Ctrl+I", does: "Show or hide the inspector" },
      { keys: "Ctrl+,", does: "Settings" },
    ],
  },
  {
    title: "Quick Look",
    items: [
      { keys: "←  /  →", does: "Previous or next file" },
      { keys: "Space  /  Esc", does: "Close" },
      { keys: "Pinch  /  Ctrl+scroll", does: "Zoom a picture" },
      { keys: "Two-finger scroll  /  drag", does: "Move a zoomed picture" },
      { keys: "Double-click", does: "Fit the picture, or show it at full size" },
    ],
  },
];
