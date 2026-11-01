/**
 * The few settings Folio has (docs/PRODUCT.md › Minimal settings), kept in local storage and
 * shared by every window: a change in one shows in the others. The theme has its own key
 * (`theme.ts`), because a script in app.html reads it before the first paint.
 */

const KEY = "folio.settings";

export const SPRING_DELAYS_MS = [500, 1000, 1500, 2000, 2500, 3000] as const;
export type ViewMode = "grid" | "list";

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
}

export const DEFAULTS: Settings = {
  startLocation: "home",
  showHidden: false,
  showInspector: false,
  defaultView: "grid",
  springDelayMs: 2000,
  returnAfterDrop: false,
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
    defaultView: saved.defaultView === "list" ? "list" : "grid",
    springDelayMs:
      delay === null
        ? null
        : typeof delay === "number"
          ? nearestDelay(delay)
          : DEFAULTS.springDelayMs,
    returnAfterDrop: saved.returnAfterDrop === true,
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
