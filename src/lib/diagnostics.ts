import type { CrashState } from "$lib/api/fs";

/** What the page can tell about the screen and language; nothing about files. */
export interface Environment {
  locale: string;
  /** The monitors, by size in pixels and scale (100% is 1). */
  monitors: { width: number; height: number; scale: number }[];
}

/**
 * The text to paste into a bug report. `build` is the one line that names the version, the
 * system and its web view (`versionLine`). The rest names the build, the system and the screens, and
 * how often Folio has crashed. It never holds a file name, a path or a tag: nothing in it
 * comes from what the person works on.
 */
export function diagnosticsText(
  build: string,
  env: Environment,
  crash: Pick<CrashState, "consent" | "sessions" | "crashed"> | null,
): string {
  const screens = env.monitors.length
    ? env.monitors.map((m) => `${m.width}×${m.height} at ${Math.round(m.scale * 100)}%`).join(", ")
    : "unknown";
  return [
    build,
    `Locale: ${env.locale || "unknown"}`,
    `Monitors: ${env.monitors.length || "unknown"} (${screens})`,
    // Without consent nothing is counted, and a count of zero would read as "never crashed".
    crash?.consent === true
      ? `Sessions: ${crash.sessions}, ended without a clean exit: ${crash.crashed}`
      : "Crash reports: off, so sessions are not counted",
  ]
    .filter(Boolean)
    .join("\n");
}

/** A new-issue address with the form filled in. Opening it sends nothing: the person submits. */
export function reportUrl(repo: string, diagnostics: string): string {
  const body = [
    "**What happened?**",
    "",
    "",
    "**What did you expect?**",
    "",
    "",
    "**Steps to reproduce**",
    "",
    "1. ",
    "",
    "<details><summary>Diagnostics</summary>",
    "",
    "```",
    diagnostics,
    "```",
    "",
    "</details>",
  ].join("\n");
  const query = new URLSearchParams({ body });
  return `${repo}/issues/new?${query}`;
}
