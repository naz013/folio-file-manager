/**
 * Plugins (docs/decisions/0005-plugin-api.md, docs/PLUGINS.md): reading a manifest, matching a
 * plugin's actions and previews to files, and the bridge that answers a plugin page's requests.
 * Pure, so it can be tested; the page and its frames are in `plugins.svelte.ts` and the
 * components.
 */

/** The version of the plugin API this Folio speaks. */
export const API_VERSION = 1;

export const PERMISSIONS = {
  selection: "See which files you run its actions on",
  read: "Read the start of the text of those files, and of the files it previews",
  notify: "Show short messages",
} as const;

export type Permission = keyof typeof PERMISSIONS;

const isPermission = (p: unknown): p is Permission =>
  typeof p === "string" && Object.hasOwn(PERMISSIONS, p);

export interface ActionDef {
  id: string;
  title: string;
  kinds: ("file" | "folder")[];
  /** Lower case, no dot; empty means any file. */
  extensions: string[];
  /** The action may be run on several items at once. */
  multiple: boolean;
}

export interface PreviewDef {
  id: string;
  title: string;
  extensions: string[];
}

export interface Manifest {
  id: string;
  name: string;
  version: string;
  description: string;
  entry: string;
  permissions: Permission[];
  actions: ActionDef[];
  previews: PreviewDef[];
}

export type Parsed = { ok: true; manifest: Manifest } | { ok: false; error: string };

const NAME = /^[a-z0-9][a-z0-9_-]{0,39}$/;
const EXT = /^[a-z0-9]{1,10}$/;
const MAX_ACTIONS = 20;
const MAX_PREVIEWS = 10;
const MAX_EXTENSIONS = 50;

const fail = (error: string): Parsed => ({ ok: false, error });

const isObject = (v: unknown): v is Record<string, unknown> =>
  typeof v === "object" && v !== null && !Array.isArray(v);

/** Control and invisible formatting characters (zero-width, bidi overrides): never shown as is. */
const HIDDEN = /[\p{Cc}\p{Cf}]/u;

/** A short single-line text of at most `max` characters, or null. */
function text(v: unknown, max: number): string | null {
  return typeof v === "string" && v.trim() && v.length <= max && !HIDDEN.test(v) ? v.trim() : null;
}

/** Names a plugin may not take, so its messages can't pass for Folio's or the system's. */
const RESERVED = /^(folio|windows|microsoft|system|admin|administrator|security|defender|explorer)/;
const isReservedName = (name: string) =>
  RESERVED.test(
    name
      .normalize("NFKC")
      .toLowerCase()
      .replace(/[^\p{L}\p{N}]/gu, ""),
  );

/** Message text for a toast: one line, nothing hidden. */
const oneLine = (s: string) => s.replace(/[\p{Cc}\p{Cf}\s]+/gu, " ").trim();

/** File extensions written with or without a dot, in any case: `["PNG", ".jpg"]`. */
function extensions(v: unknown): string[] | null {
  if (!Array.isArray(v) || v.length > MAX_EXTENSIONS) return null;
  const out: string[] = [];
  for (const e of v) {
    if (typeof e !== "string") return null;
    const ext = e.replace(/^\./, "").toLowerCase();
    if (!EXT.test(ext)) return null;
    if (!out.includes(ext)) out.push(ext);
  }
  return out;
}

/** A path inside the plugin folder: forward slashes, plain names, an .html file. */
function entryPath(v: unknown): string | null {
  if (typeof v !== "string" || v.length > 100) return null;
  const parts = v.split("/");
  const plain = parts.every((p) => p && p !== "." && p !== ".." && !/[\\:]/.test(p));
  return plain && /\.html?$/i.test(v) ? v : null;
}

/**
 * Reads the manifest of the plugin in folder `id`. Whatever is wrong is said in the error, which
 * is shown to the person; fields Folio doesn't know are ignored, so a newer manifest still reads.
 */
export function parseManifest(id: string, source: string): Parsed {
  let raw: unknown;
  try {
    raw = JSON.parse(source);
  } catch {
    return fail("plugin.json is not valid JSON");
  }
  if (!isObject(raw)) return fail("plugin.json must be an object");
  if (raw.id !== id) return fail(`"id" must be "${id}", the name of the folder`);
  const name = text(raw.name, 60);
  if (!name) return fail('"name" is missing or too long (60 characters at most)');
  if (isReservedName(name)) return fail(`"name" can't start like a part of Windows or Folio`);
  const version = text(raw.version, 20);
  if (!version) return fail('"version" is missing or too long');
  const api = raw.api ?? 1;
  if (typeof api !== "number" || !Number.isInteger(api) || api < 1) {
    return fail('"api" must be a whole number');
  }
  if (api > API_VERSION) return fail(`It needs a newer Folio (plugin API ${api})`);
  const description = raw.description === undefined ? "" : text(raw.description, 200);
  if (description === null) return fail('"description" must be a line of at most 200 characters');
  const entry = raw.entry === undefined ? "index.html" : entryPath(raw.entry);
  if (!entry) return fail('"entry" must be an .html file inside the plugin folder');

  const asked = raw.permissions ?? [];
  if (!Array.isArray(asked)) return fail('"permissions" must be a list');
  const permissions: Permission[] = [];
  for (const p of asked) {
    if (!isPermission(p)) return fail(`Unknown permission ${JSON.stringify(p)}`);
    if (!permissions.includes(p)) permissions.push(p);
  }

  const actions: ActionDef[] = [];
  const rawActions = raw.actions ?? [];
  if (!Array.isArray(rawActions) || rawActions.length > MAX_ACTIONS) {
    return fail(`"actions" must be a list of at most ${MAX_ACTIONS}`);
  }
  for (const a of rawActions) {
    if (!isObject(a) || typeof a.id !== "string" || !NAME.test(a.id)) {
      return fail('Each action needs an "id" of lower-case letters, digits, - and _');
    }
    if (actions.some((x) => x.id === a.id)) return fail(`Two actions are called "${a.id}"`);
    const title = text(a.title, 60);
    if (!title) return fail(`Action "${a.id}" needs a "title" of at most 60 characters`);
    const kinds = a.kinds ?? ["file"];
    if (
      !Array.isArray(kinds) ||
      !kinds.length ||
      !kinds.every((k) => k === "file" || k === "folder")
    ) {
      return fail(`Action "${a.id}": "kinds" lists "file" and/or "folder"`);
    }
    const exts = a.extensions === undefined ? [] : extensions(a.extensions);
    if (!exts) return fail(`Action "${a.id}": "extensions" must be a list like ["txt", "md"]`);
    actions.push({
      id: a.id,
      title,
      kinds: [...new Set(kinds as ("file" | "folder")[])],
      extensions: exts,
      multiple: a.multiple === true,
    });
  }
  // An action is told which files it was run on; asking for that is part of the manifest.
  if (actions.length && !permissions.includes("selection")) {
    return fail('Actions need the "selection" permission');
  }

  const previews: PreviewDef[] = [];
  const rawPreviews = raw.previews ?? [];
  if (!Array.isArray(rawPreviews) || rawPreviews.length > MAX_PREVIEWS) {
    return fail(`"previews" must be a list of at most ${MAX_PREVIEWS}`);
  }
  for (const p of rawPreviews) {
    if (!isObject(p) || typeof p.id !== "string" || !NAME.test(p.id)) {
      return fail('Each preview needs an "id" of lower-case letters, digits, - and _');
    }
    if (previews.some((x) => x.id === p.id)) return fail(`Two previews are called "${p.id}"`);
    const title = text(p.title, 60);
    if (!title) return fail(`Preview "${p.id}" needs a "title"`);
    const exts = extensions(p.extensions);
    if (!exts?.length) return fail(`Preview "${p.id}" needs "extensions", like ["csv"]`);
    previews.push({ id: p.id, title, extensions: exts });
  }

  return {
    ok: true,
    manifest: { id, name, version, description, entry, permissions, actions, previews },
  };
}

/** What the person agreed to for one plugin, kept with the settings. */
export interface Grant {
  version: string;
  permissions: Permission[];
  /** A hash of the plugin's files when it was approved; "" for an approval made before hashes. */
  digest: string;
}

/** Reads the saved grants, dropping anything that isn't one. */
export function cleanGrants(saved: unknown): Record<string, Grant> {
  const out: Record<string, Grant> = {};
  if (!isObject(saved)) return out;
  for (const [id, g] of Object.entries(saved)) {
    if (
      id === "__proto__" ||
      !isObject(g) ||
      typeof g.version !== "string" ||
      !Array.isArray(g.permissions)
    )
      continue;
    out[id] = {
      version: g.version,
      permissions: g.permissions.filter(isPermission),
      digest: typeof g.digest === "string" ? g.digest : "",
    };
  }
  return out;
}

/**
 * A plugin is on when the person agreed to this version, these files and everything it asks for.
 * A plugin whose files can't be hashed (`digest` null) is never on.
 */
export function isGranted(
  manifest: Manifest,
  grant: Grant | undefined,
  digest: string | null,
): boolean {
  return (
    !!grant &&
    !!digest &&
    grant.digest === digest &&
    grant.version === manifest.version &&
    manifest.permissions.every((p) => grant.permissions.includes(p))
  );
}

/** A plugin as the page holds it. */
export interface Loaded {
  id: string;
  manifest: Manifest | null;
  /** Why it can't be used: no manifest, a bad one. */
  error: string | null;
  /** Hash of the plugin's files as they are now; null when they couldn't be read. */
  digest: string | null;
  enabled: boolean;
  /** Approved before, but its version or permissions have changed since. */
  changed: boolean;
}

/** What `plugins_list` returns for each folder. */
export interface Found {
  id: string;
  manifest: string | null;
  error: string | null;
  digest: string | null;
}

/** Turns what is on disk and what was granted into the page's list. */
export function load(found: Found[], grants: Record<string, Grant>): Loaded[] {
  return found.map((f) => {
    if (f.manifest === null) {
      return {
        id: f.id,
        manifest: null,
        error: f.error ?? "No manifest",
        digest: null,
        enabled: false,
        changed: false,
      };
    }
    const parsed = parseManifest(f.id, f.manifest);
    if (!parsed.ok) {
      return {
        id: f.id,
        manifest: null,
        error: parsed.error,
        digest: null,
        enabled: false,
        changed: false,
      };
    }
    const grant = Object.hasOwn(grants, f.id) ? grants[f.id] : undefined;
    const enabled = isGranted(parsed.manifest, grant, f.digest);
    return {
      id: f.id,
      manifest: parsed.manifest,
      error: null,
      digest: f.digest,
      enabled,
      changed: !!grant && !enabled,
    };
  });
}

/** The lower-case extension of a file name, without the dot; "" for none. */
export function extensionOf(name: string): string {
  const dot = name.lastIndexOf(".");
  return dot > 0 ? name.slice(dot + 1).toLowerCase() : "";
}

export interface Item {
  path: string;
  name: string;
  isDir: boolean;
}

export interface ActionHit {
  plugin: Loaded & { manifest: Manifest };
  action: ActionDef;
}

const usable = (p: Loaded): p is Loaded & { manifest: Manifest } => p.enabled && !!p.manifest;

/** The actions of enabled plugins that apply to every one of `items`. */
export function actionsFor(plugins: Loaded[], items: Item[]): ActionHit[] {
  if (!items.length) return [];
  const hits: ActionHit[] = [];
  for (const plugin of plugins.filter(usable)) {
    for (const action of plugin.manifest.actions) {
      if (items.length > 1 && !action.multiple) continue;
      const fits = items.every((i) =>
        i.isDir
          ? action.kinds.includes("folder")
          : action.kinds.includes("file") &&
            (!action.extensions.length || action.extensions.includes(extensionOf(i.name))),
      );
      if (fits) hits.push({ plugin, action });
    }
  }
  return hits;
}

export interface PreviewHit {
  plugin: Loaded & { manifest: Manifest };
  preview: PreviewDef;
}

/** The first enabled plugin that previews this kind of file. */
export function previewFor(plugins: Loaded[], item: Item): PreviewHit | null {
  if (item.isDir) return null;
  const ext = extensionOf(item.name);
  for (const plugin of plugins.filter(usable)) {
    const preview = plugin.manifest.previews.find((p) => p.extensions.includes(ext));
    if (preview) return { plugin, preview };
  }
  return null;
}

// ---- The page itself ----

const escapeAttr = (s: string) =>
  s.replace(/&/g, "&amp;").replace(/"/g, "&quot;").replace(/</g, "&lt;").replace(/>/g, "&gt;");

/** What a plugin page may load: its own folder's files, never the network or any frame. */
export const pageCsp = (origin: string) =>
  [
    "default-src 'none'",
    `script-src ${origin} 'unsafe-inline'`,
    `style-src ${origin} 'unsafe-inline'`,
    `img-src ${origin} data:`,
    `font-src ${origin} data:`,
    "connect-src 'none'",
    "frame-src 'none'",
    "form-action 'none'",
    "base-uri 'none'",
  ].join("; ");

/**
 * The document a plugin's frame is given (as `srcdoc`) in place of a link to its page.
 *
 * Tauri trusts a page whose address is on one of its own protocols (`plugin.localhost`) like
 * Folio's page, and on Windows it puts its command channel and key into every frame. A plugin
 * page at that address could therefore run Folio's commands. A `srcdoc` page has no such
 * address, so Tauri treats what it sends as coming from an untrusted page and refuses it.
 *
 * `base` is the folder the page's own files are loaded from, `origin` the protocol's origin
 * (`scheme://host`). Our `<base>` and policy come first, so they hold before any plugin code
 * runs, and the page's own `<base>` is refused by `base-uri 'none'`.
 */
export function pageDoc(html: string, base: string, origin: string): string {
  return (
    `<!doctype html><html><head><base href="${escapeAttr(base)}">` +
    `<meta http-equiv="Content-Security-Policy" content="${escapeAttr(pageCsp(origin))}">` +
    html
  );
}

// ---- The conversation with a plugin's page ----

/** Every message either way carries this, so a stray message isn't mistaken for one. */
export const TAG = 1;

export interface Call {
  folio: typeof TAG;
  id: number;
  method: string;
  params?: unknown;
}

export interface Answer {
  folio: typeof TAG;
  id: number;
  result?: unknown;
  error?: string;
}

/** What Folio tells the page, apart from answers. */
export type Event =
  | { folio: typeof TAG; event: "action"; action: string; paths: string[] }
  | {
      folio: typeof TAG;
      event: "preview";
      preview: string;
      path: string;
      name: string;
      size: number;
    };

export const actionEvent = (action: string, paths: string[]): Event => ({
  folio: TAG,
  event: "action",
  action,
  paths,
});

export const previewEvent = (preview: string, item: Item & { size: number }): Event => ({
  folio: TAG,
  event: "preview",
  preview,
  path: item.path,
  name: item.name,
  size: item.size,
});

/** True for a message a plugin page sends to say it has loaded and can be told things. */
export const isReady = (m: unknown): boolean =>
  isObject(m) && m.folio === TAG && m.event === "ready";

const isCall = (m: unknown): m is Call =>
  isObject(m) &&
  m.folio === TAG &&
  typeof m.id === "number" &&
  Number.isSafeInteger(m.id) &&
  typeof m.method === "string";

/** What the bridge needs from Folio; the page supplies it. */
export interface Host {
  notify: (message: string) => void;
  readText: (path: string) => Promise<{ text: string; truncated: boolean }>;
  /** Called once when the page keeps flooding the bridge; the host should remove the plugin. */
  abuse?: () => void;
  now?: () => number;
}

/** How many runs' files a plugin can still read. */
const MAX_RUNS = 5;
/** How long after a run the plugin may still read its files (ms): it can't poll them later. */
const RUN_TTL_MS = 60_000;
const NOTICE_MAX = 200;
/** A plugin may show this many messages within the window, then is told to wait. */
const NOTICE_BURST = 5;
const NOTICE_WINDOW_MS = 5000;
/** Messages of any kind a page may send per second; the rest are refused without any work. */
const CALLS_PER_SECOND = 50;
/** Refused messages after which the page is called abusive. */
const ABUSE_AFTER = 500;
/** Reads a plugin may have under way at once, and per window: it can't starve real previews. */
const MAX_READS_IN_FLIGHT = 2;
const READS_PER_WINDOW = 20;
const READ_WINDOW_MS = 10_000;

/**
 * Answers one plugin's requests. `allow` names the files the plugin was handed (the ones an
 * action ran on, or the one being previewed): it can read those, for a short time, and no
 * others. A call for something it was not granted is refused with a message, never silently
 * dropped.
 */
export function createBridge(manifest: Manifest, host: Host) {
  /** The files of the last few runs: a second run must not cut off the first one's reads. */
  const runs: { paths: string[]; at: number }[] = [];
  const stamps: number[] = [];
  const readStamps: number[] = [];
  let reading = 0;
  let second = { start: 0, count: 0 };
  let refused = 0;
  let reported = false;
  const now = host.now ?? Date.now;

  /** True while the page is within the rate it may send messages at. */
  function withinRate(t: number): boolean {
    if (t - second.start >= 1000 || t < second.start) second = { start: t, count: 0 };
    if (++second.count <= CALLS_PER_SECOND) return true;
    if (++refused >= ABUSE_AFTER && !reported) {
      reported = true;
      host.abuse?.();
    }
    return false;
  }

  async function run(call: Call): Promise<unknown> {
    const need = (p: Permission) => {
      if (!manifest.permissions.includes(p)) throw new Error(`This plugin was not given "${p}"`);
    };
    const params = isObject(call.params) ? call.params : {};
    const t = now();
    switch (call.method) {
      case "selection":
        need("selection");
        return [...(runs.at(-1)?.paths ?? [])];
      case "notify": {
        need("notify");
        if (typeof params.message !== "string") throw new Error('"message" must be text');
        const message = oneLine(params.message).slice(0, NOTICE_MAX);
        if (!message) throw new Error('"message" must be text');
        while (stamps.length && t - stamps[0] > NOTICE_WINDOW_MS) stamps.shift();
        if (stamps.length >= NOTICE_BURST) throw new Error("Too many messages: wait a moment");
        stamps.push(t);
        host.notify(`${oneLine(manifest.name)} (plugin): ${message}`);
        return null;
      }
      case "read": {
        need("read");
        const path = params.path;
        if (
          typeof path !== "string" ||
          !runs.some((r) => t - r.at <= RUN_TTL_MS && r.paths.includes(path))
        ) {
          throw new Error("That file was not given to this plugin");
        }
        while (readStamps.length && t - readStamps[0] > READ_WINDOW_MS) readStamps.shift();
        if (reading >= MAX_READS_IN_FLIGHT || readStamps.length >= READS_PER_WINDOW) {
          throw new Error("Too many reads: wait a moment");
        }
        readStamps.push(t);
        reading++;
        try {
          return await host.readText(path);
        } finally {
          reading--;
        }
      }
      default:
        throw new Error(`Unknown method "${call.method}"`);
    }
  }

  return {
    /**
     * The files a run hands the plugin, which it may now read. The last few runs stay readable
     * for a minute, so runs that overlap don't undo each other; older ones are forgotten.
     */
    allow(paths: string[]) {
      runs.push({ paths: [...paths], at: now() });
      if (runs.length > MAX_RUNS) runs.shift();
    },
    /** The answer to a message from the page, or null for a message that is not a call. */
    async handle(message: unknown): Promise<Answer | null> {
      if (!withinRate(now())) {
        return isCall(message)
          ? { folio: TAG, id: message.id, error: "Too many requests: slow down" }
          : null;
      }
      if (!isCall(message)) return null;
      try {
        return { folio: TAG, id: message.id, result: await run(message) };
      } catch (e) {
        return {
          folio: TAG,
          id: message.id,
          error: e instanceof Error ? e.message : String(e),
        };
      }
    },
  };
}
