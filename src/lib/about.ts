import type { AppInfo } from "$lib/api/fs";

/** Where the project lives. One place, so a move is one edit. */
export const REPO = "https://github.com/naz013/folio-file-manager";
export const LINKS = [
  { label: "Source code", url: REPO },
  { label: "Report an issue", url: `${REPO}/issues/new` },
  { label: "Website", url: "https://naz013.github.io/folio-file-manager/" },
  { label: "License text", url: `${REPO}/blob/main/LICENSE` },
] as const;

export const LICENSE = "AGPL-3.0-only";

/** The one line to paste into a bug report: version, build, and what it runs on. */
export function versionLine(info: AppInfo): string {
  const build = info.commit === "unknown" ? info.buildDate : `${info.commit}, ${info.buildDate}`;
  const parts = [
    `Folio ${info.version} (${build})`,
    `Tauri ${info.tauri}`,
    info.webview ? `WebView ${info.webview}` : null,
    `${info.os} ${info.arch}`,
  ];
  return parts.filter(Boolean).join(" · ");
}
