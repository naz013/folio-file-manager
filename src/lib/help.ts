/** The help topics. They ship with the app and are searched here, so help works offline. */

export interface HelpTopic {
  id: string;
  title: string;
  /** Paragraphs, in order. */
  body: readonly string[];
}

export const TOPICS: readonly HelpTopic[] = [
  {
    id: "recycle-bin",
    title: "Deleting, the Recycle Bin and undo",
    body: [
      "Delete never removes a file for good. It moves the file to the Recycle Bin, and you can take it back from there at any time: open the Recycle Bin in the sidebar, pick the items and choose Restore.",
      "Press Ctrl+Z right after a move, rename, delete, copy or new folder to undo it, and Ctrl+Y to do it again. Undo only takes back what Folio did, and it stops rather than overwrite or remove anything that has changed since.",
      "Files are only removed for good when you choose Delete permanently on items in the Recycle Bin, or Empty Recycle Bin, and confirm. Cleanup never deletes anything itself: it sends files to the Recycle Bin like any other delete.",
    ],
  },
  {
    id: "tabs",
    title: "Tabs and windows",
    body: [
      "Ctrl+T opens a new tab and Ctrl+W closes it. Ctrl+Tab and Ctrl+Shift+Tab move between tabs, and Ctrl+1 to 9 jump to one.",
      "Right-click a folder and choose Open in new tab, or middle-click it. Drag a tab out of the tab bar to make a new window. Your tabs come back the next time you start Folio.",
    ],
  },
  {
    id: "drag",
    title: "Dragging files: copy or move",
    body: [
      "Drag files onto a folder to put them there. Without a key, files dropped on another drive are copied and files dropped on the same drive are moved.",
      "Hold Ctrl while you drop to copy, or Shift to move, whatever the drive. Hold the pointer over a folder for a moment and it opens, so you can reach a folder deep down in one drag. Press Esc to cancel a drag.",
      "You can also drag to Explorer and other programs: move the pointer out of the window.",
    ],
  },
  {
    id: "clipboard",
    title: "Copy, cut and the clipboard shelf",
    body: [
      "Ctrl+C and Ctrl+X mark files, and Ctrl+V puts them in the open folder. Nothing moves until you paste, so a cut file stays where it is until then.",
      "The shelf keeps everything you marked, from several folders, so you can paste it all in one go. Open it with Ctrl+Shift+V.",
    ],
  },
  {
    id: "tags",
    title: "Tags",
    body: [
      "Select files, right-click and choose Tags to colour-label them, or press 1 to 7 to toggle a colour tag. Tags are kept by Folio and never change your files.",
      "Pick a tag in the sidebar to see everything with it, from any folder. All tags, under the list, opens a screen to make, rename, recolour and delete your own tags. A smart folder saves a search, such as a tag and a date, and works out its answer afresh each time you open it.",
    ],
  },
  {
    id: "search",
    title: "Searching and going places",
    body: [
      "Ctrl+F searches the open folder and everything below it by name and size. Ctrl+L lets you type a path. Ctrl+K opens the command palette: type part of a folder name, or an action such as clean or settings.",
    ],
  },
  {
    id: "cleanup",
    title: "Cleanup and Storage",
    body: [
      "Storage shows what takes room on a drive. Large and old files, duplicates and unfinished copies come from a scan you run in Storage first. Temp files and old downloads are read straight from their folders, with no scan.",
      "Cleanup only suggests. Nothing is picked until you pick it, and what you remove goes to the Recycle Bin.",
    ],
  },
  {
    id: "views",
    title: "Views and the inspector",
    body: [
      "Switch between grid, list, columns and gallery from the toolbar. Ctrl+I shows the inspector with details, size and tags of what is selected, and Ctrl+H shows or hides hidden files.",
      "Space opens Quick Look on a file: use the arrow keys for the next file, and Esc to close.",
    ],
  },
];

/** The topics that match every word typed, by title or text. All of them for an empty query. */
export function findTopics(query: string, topics: readonly HelpTopic[] = TOPICS): HelpTopic[] {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  return topics.filter((t) => {
    const text = `${t.title} ${t.body.join(" ")}`.toLowerCase();
    return words.every((w) => text.includes(w));
  });
}
