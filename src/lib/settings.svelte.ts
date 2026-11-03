/**
 * The few settings Folio has (docs/PRODUCT.md › Minimal settings), kept in local storage and
 * shared by every window: a change in one shows in the others. The theme has its own key
 * (`theme.ts`), because a script in app.html reads it before the first paint.
 */

const KEY = "folio.settings";

export const SPRING_DELAYS_MS = [500, 1000, 1500, 2000, 2500, 3000] as const;
export type ViewMode = "grid" | "list" | "columns" | "gallery";
/** The thresholds cleanup offers for "large" (MB) and "old" (days). */
export const CLEANUP_SIZES_MB = [10, 50, 100, 500, 1000, 5000] as const;
export const CLEANUP_AGES_DAYS = [30, 90, 180, 365, 730] as const;
/** The smallest file a duplicate search looks at, in MB (the scan keeps files from 1 MB). */
export const DUPLICATE_SIZES_MB = [1, 10, 50, 100, 500] as const;
/** How long an unfinished copy must have been left alone to be suggested. */
export const PARTIAL_AGES_DAYS = [3, 7, 14, 30, 90] as const;
export const DOWNLOAD_AGES_DAYS = [14, 30, 60, 90, 180, 365] as const;

export interface Settings {
  /** Where new tabs and a fresh start open: "home", "desktop", "documents", "downloads", or a path. */
  startLocation: string;
  showHidden: boolean;
  /** The inspector panel at the right of the list. */
  showInspector: boolean;
  defaultView: ViewMode;
  /** How long a drag hovers over a folder or tab before it opens; null turns it off. */
  springDelayMs: number | null;
  /** After a drop or cancel in a spring-loaded trip, go back to the folder it began in. */
  returnAfterDrop: boolean;
  /** Folders pinned to the sidebar's Favorites by dragging, in the order shown. */
  pinnedFolders: string[];
  /** Cleanup suggests files bigger than this many MB ... */
  cleanupMinMb: number;
  /** ... that have not changed for this many days. */
  cleanupAgeDays: number;
  /** Installers and archives in Downloads older than this many days are suggested. */
  cleanupDownloadsDays: number;
  /** Duplicates are looked for among files at least this many MB big. */
  cleanupDupMinMb: number;
  /** Unfinished copies untouched for this many days are suggested. */
  cleanupPartialDays: number;
  /** Use the system's own title bar and window buttons instead of drawing them. */
  nativeFrame: boolean;
  /** The first-run tour was shown (finished or skipped). */
  onboarded: boolean;
  /** Look for a new version in the background, once a day. */
  updateCheck: boolean;
  /** When the last background check ran (ms since 1970); 0 for never. */
  updateLastCheck: number;
  /** A version the user said "Later" to: not offered again until a newer one is out. */
  updateSkipped: string;
  /** Show the hints in empty areas that say what to do there. */
  showHints: boolean;
}

export const DEFAULTS: Settings = {
  startLocation: "home",
  showHidden: false,
  showInspector: false,
  defaultView: "grid",
  springDelayMs: 2000,
  returnAfterDrop: false,
  pinnedFolders: [],
  cleanupMinMb: 100,
  cleanupAgeDays: 365,
  cleanupDownloadsDays: 90,
  cleanupDupMinMb: 10,
  cleanupPartialDays: 7,
  nativeFrame: false,
  onboarded: false,
  updateCheck: true,
  updateLastCheck: 0,
  updateSkipped: "",
  showHints: true,
};

/** The allowed delay closest to `ms`, so a hand-edited value still shows in the list. */
function nearestDelay(ms: number): number {
  return SPRING_DELAYS_MS.reduce((best, d) => (Math.abs(d - ms) < Math.abs(best - ms) ? d : best));
}

/** The saved settings, with anything missing or out of range replaced by its default. */
function read(): Settings {
  let saved: Partial<Record<keyof Settings, unknown>> = {};
  try {
    saved = JSON.parse(localStorage.getItem(KEY) ?? "{}") ?? {};
  } catch {
    // Unreadable: start from the defaults.
  }
  const delay = saved.springDelayMs;
  return {
    startLocation:
      typeof saved.startLocation === "string" && saved.startLocation.trim()
        ? saved.startLocation
        : DEFAULTS.startLocation,
    showHidden: saved.showHidden === true,
    showInspector: saved.showInspector === true,
    defaultView:
      saved.defaultView === "list" ||
      saved.defaultView === "columns" ||
      saved.defaultView === "gallery"
        ? saved.defaultView
        : "grid",
    springDelayMs:
      delay === null
        ? null
        : typeof delay === "number"
          ? nearestDelay(delay)
          : DEFAULTS.springDelayMs,
    returnAfterDrop: saved.returnAfterDrop === true,
    cleanupMinMb: (CLEANUP_SIZES_MB as readonly unknown[]).includes(saved.cleanupMinMb)
      ? (saved.cleanupMinMb as number)
      : DEFAULTS.cleanupMinMb,
    cleanupAgeDays: (CLEANUP_AGES_DAYS as readonly unknown[]).includes(saved.cleanupAgeDays)
      ? (saved.cleanupAgeDays as number)
      : DEFAULTS.cleanupAgeDays,
    cleanupDownloadsDays: (DOWNLOAD_AGES_DAYS as readonly unknown[]).includes(
      saved.cleanupDownloadsDays,
    )
      ? (saved.cleanupDownloadsDays as number)
      : DEFAULTS.cleanupDownloadsDays,
    cleanupDupMinMb: (DUPLICATE_SIZES_MB as readonly unknown[]).includes(saved.cleanupDupMinMb)
      ? (saved.cleanupDupMinMb as number)
      : DEFAULTS.cleanupDupMinMb,
    cleanupPartialDays: (PARTIAL_AGES_DAYS as readonly unknown[]).includes(saved.cleanupPartialDays)
      ? (saved.cleanupPartialDays as number)
      : DEFAULTS.cleanupPartialDays,
    nativeFrame: saved.nativeFrame === true,
    onboarded: saved.onboarded === true,
    updateCheck: saved.updateCheck !== false,
    updateLastCheck:
      typeof saved.updateLastCheck === "number" && Number.isFinite(saved.updateLastCheck)
        ? saved.updateLastCheck
        : DEFAULTS.updateLastCheck,
    updateSkipped: typeof saved.updateSkipped === "string" ? saved.updateSkipped : "",
    showHints: saved.showHints !== false,
    pinnedFolders: Array.isArray(saved.pinnedFolders)
      ? saved.pinnedFolders.filter(
          (p, i, all): p is string => typeof p === "string" && !!p && all.indexOf(p) === i,
        )
      : [],
  };
}

/** The live settings: read them anywhere; change them with `updateSettings`. */
export const settings: Settings = $state(read());

export function updateSettings(patch: Partial<Settings>) {
  Object.assign(settings, patch);
  try {
    localStorage.setItem(KEY, JSON.stringify($state.snapshot(settings)));
  } catch {
    // The change still applies to this window, it just won't outlive it.
  }
}

// Another window changed them: follow.
if (typeof window !== "undefined") {
  window.addEventListener("storage", (e) => {
    if (e.key === KEY || e.key === null) Object.assign(settings, read());
  });
}

/** Pins `paths` to the end of the Favorites; ones already pinned stay where they are. */
export function pinFolders(paths: string[]) {
  // Windows paths differ by case and trailing separators without being different folders.
  const key = (p: string) => p.replace(/[\\/]+$/, "").toLowerCase();
  const have = settings.pinnedFolders.map(key);
  const fresh = paths.filter((p) => {
    if (have.includes(key(p))) return false;
    have.push(key(p));
    return true;
  });
  if (fresh.length) updateSettings({ pinnedFolders: [...settings.pinnedFolders, ...fresh] });
}

export function unpinFolder(path: string) {
  updateSettings({ pinnedFolders: settings.pinnedFolders.filter((p) => p !== path) });
}

/** Moves a pinned folder one place up (`-1`) or down (`1`). */
export function movePinned(path: string, by: -1 | 1) {
  const list = [...settings.pinnedFolders];
  const from = list.indexOf(path);
  const to = from + by;
  if (from < 0 || to < 0 || to >= list.length) return;
  [list[from], list[to]] = [list[to], list[from]];
  updateSettings({ pinnedFolders: list });
}

/** The folder new tabs open in, given the user's home folder. */
export function startPath(home: string): string {
  const named: Record<string, string> = {
    desktop: "Desktop",
    documents: "Documents",
    downloads: "Downloads",
  };
  const where = settings.startLocation;
  if (where === "home") return home;
  const sub = named[where];
  if (sub)
    return home.endsWith("\\") || home.endsWith("/")
      ? home + sub
      : `${home}${home.includes("\\") ? "\\" : "/"}${sub}`;
  return where;
}
