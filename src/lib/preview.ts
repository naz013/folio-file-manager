/** What Quick Look can show for a file, judged by its extension. */
export type PreviewKind = "image" | "pdf" | "audio" | "video" | "text" | "other";

const IMAGE = new Set(["png", "jpg", "jpeg", "gif", "webp", "bmp", "svg", "ico", "avif"]);
const AUDIO = new Set(["mp3", "wav", "ogg", "oga", "m4a", "flac", "aac", "opus"]);
const VIDEO = new Set(["mp4", "webm", "mov", "m4v", "ogv"]);

export function previewKind(name: string, isDir: boolean): PreviewKind {
  if (isDir) return "other";
  const dot = name.lastIndexOf(".");
  const ext = dot > 0 ? name.slice(dot + 1).toLowerCase() : "";
  if (IMAGE.has(ext)) return "image";
  if (ext === "pdf") return "pdf";
  if (AUDIO.has(ext)) return "audio";
  if (VIDEO.has(ext)) return "video";
  // Anything else is tried as text; the core says no for binary files.
  return ext === "exe" || ext === "dll" || ext === "zip" ? "other" : "text";
}
