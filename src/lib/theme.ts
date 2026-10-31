/** Which look the app uses: follow the OS, or force one. */
export type Theme = "system" | "light" | "dark";

/** Also read by the inline script in app.html, which sets the theme before first paint. */
const KEY = "folio.theme";

export const themes: Theme[] = ["system", "light", "dark"];

export function loadTheme(): Theme {
  try {
    const stored = localStorage.getItem(KEY);
    if (stored === "light" || stored === "dark") return stored;
  } catch {
    // Storage can be unavailable; fall back to following the OS.
  }
  return "system";
}

/** Shows `theme` in this window. "system" hands the choice back to the OS. */
export function applyTheme(theme: Theme) {
  if (theme === "system") delete document.documentElement.dataset.theme;
  else document.documentElement.dataset.theme = theme;
}

/** Applies and remembers `theme`; every window picks it up through `onThemeChange`. */
export function setTheme(theme: Theme) {
  applyTheme(theme);
  try {
    if (theme === "system") localStorage.removeItem(KEY);
    else localStorage.setItem(KEY, theme);
  } catch {
    // The choice still applies to this window, it just won't outlive it.
  }
}

/** Calls `fn` when another window changes the theme; returns how to stop. */
export function onThemeChange(fn: (theme: Theme) => void): () => void {
  const handler = (e: StorageEvent) => {
    if (e.key === KEY || e.key === null) fn(loadTheme());
  };
  window.addEventListener("storage", handler);
  return () => window.removeEventListener("storage", handler);
}

export const nextTheme = (theme: Theme): Theme =>
  themes[(themes.indexOf(theme) + 1) % themes.length];
