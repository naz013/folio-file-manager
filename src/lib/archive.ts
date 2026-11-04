/** One entry of an archive as the core lists it (`list_archive`). */
export interface ArchiveEntry {
  /** The name with `/` between parts; control and hidden characters are already written out. */
  path: string;
  isDir: boolean;
  /** Bytes once unpacked. */
  size: number;
  /** `YYYY-MM-DD HH:MM`, local time. */
  modified: string | null;
  encrypted: boolean;
}

export interface ArchiveListing {
  format: string;
  status: "ok" | "encrypted" | "damaged" | "unsupported";
  message: string | null;
  entries: ArchiveEntry[];
  total: number | null;
  truncated: boolean;
}

/** A folder or file in the tree of an archive. */
export interface Node {
  /** The whole path, which is also the node's identity. */
  key: string;
  name: string;
  isDir: boolean;
  /** For a folder, what is inside it, all levels. */
  size: number;
  /** For a folder, how many files are inside it, all levels. */
  files: number;
  modified: string | null;
  encrypted: boolean;
  children: Node[];
}

/** A line of the list as drawn. */
export interface Row {
  node: Node;
  depth: number;
  open: boolean;
}

/** Names in order, numbers as numbers; one collator, since making one per comparison is slow. */
const collator = new Intl.Collator(undefined, { numeric: true });

/** Where a name sits: its parts, without the empty ones that `a//b` and `./a` leave. */
function parts(path: string): string[] {
  return path.split("/").filter((p) => p !== "" && p !== ".");
}

/**
 * The tree an archive's entries make. A folder an entry sits in is made even if the archive has
 * no entry for it (many don't). Folders come before files, each in name order; a folder's size
 * and file count add up everything below it.
 */
export function buildTree(entries: ArchiveEntry[]): Node[] {
  const root: Node = {
    key: "",
    name: "",
    isDir: true,
    size: 0,
    files: 0,
    modified: null,
    encrypted: false,
    children: [],
  };
  // Every node gets a key of its own, even when an archive has two things with one name (a file
  // and a folder, or the same member twice): a list keyed by name would break on it.
  const used = new Set<string>();
  const unique = (base: string): string => {
    let key = base;
    for (let n = 2; used.has(key); n++) key = `${base}#${n}`;
    used.add(key);
    return key;
  };
  // A folder's folders by name, so finding one is not a walk over everything in it.
  const folders = new WeakMap<Node, Map<string, Node>>();
  const folder = (parent: Node, name: string, key: string): Node => {
    let byName = folders.get(parent);
    if (!byName) folders.set(parent, (byName = new Map()));
    let node = byName.get(name);
    if (!node) {
      node = {
        key: unique(key),
        name,
        isDir: true,
        size: 0,
        files: 0,
        modified: null,
        encrypted: false,
        children: [],
      };
      parent.children.push(node);
      byName.set(name, node);
    }
    return node;
  };
  for (const e of entries) {
    const ps = parts(e.path);
    if (!ps.length) continue;
    let at = root;
    const chain = [root];
    ps.forEach((name, i) => {
      const key = ps.slice(0, i + 1).join("/");
      const last = i === ps.length - 1;
      if (last && !e.isDir) {
        at.children.push({
          key: unique(key),
          name,
          isDir: false,
          size: e.size,
          files: 1,
          modified: e.modified,
          encrypted: e.encrypted,
          children: [],
        });
      } else {
        at = folder(at, name, key);
        chain.push(at);
        if (last) at.modified = e.modified ?? at.modified;
      }
    });
    if (!e.isDir) {
      for (const f of chain) {
        f.size += e.size;
        f.files += 1;
        f.encrypted ||= e.encrypted;
      }
    }
  }
  const order = (nodes: Node[]) => {
    nodes.sort((a, b) =>
      a.isDir !== b.isDir ? (a.isDir ? -1 : 1) : collator.compare(a.name, b.name),
    );
    for (const n of nodes) order(n.children);
  };
  order(root.children);
  return root.children;
}

/** The lines to draw: the nodes in order, with the contents of the open folders only. */
export function flatten(nodes: Node[], open: ReadonlySet<string>): Row[] {
  const rows: Row[] = [];
  const walk = (list: Node[], depth: number) => {
    for (const node of list) {
      const isOpen = node.isDir && open.has(node.key);
      rows.push({ node, depth, open: isOpen });
      if (isOpen) walk(node.children, depth + 1);
    }
  };
  walk(nodes, 0);
  return rows;
}

/** Every folder's key, to open them all. */
export function folderKeys(nodes: Node[]): string[] {
  return nodes.flatMap((n) => (n.isDir ? [n.key, ...folderKeys(n.children)] : []));
}

/** What a tree holds in all: files, folders and unpacked bytes. */
export function counts(tree: Node[]): { files: number; folders: number; bytes: number } {
  return {
    files: tree.reduce((n, t) => n + t.files, 0),
    folders: folderKeys(tree).length,
    bytes: tree.reduce((n, t) => n + t.size, 0),
  };
}
