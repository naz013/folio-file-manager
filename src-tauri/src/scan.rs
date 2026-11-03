//! The background disk scanner and its size cache (docs/ARCHITECTURE.md › Disk scanning).
//!
//! A scan walks a root in parallel and keeps, per folder, the bytes of the files directly in
//! it and the running total below it, plus the files big enough to matter for cleanup. That is
//! all the folder sizes, the space breakdown and the large-file lists need, at a fraction of
//! what a row per file would cost. The cache lives in SQLite, so it survives restarts, and a
//! recursive watcher on each scanned root keeps it current by re-reading only the folders that
//! changed.

use std::collections::HashMap;
use std::path::{MAIN_SEPARATOR, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use jwalk::WalkDirGeneric;
use notify_debouncer_mini::notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_mini::{DebounceEventResult, Debouncer, new_debouncer};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

/// Files at least this large are listed individually, for the large and old file cleanups.
pub const BIG_MIN: u64 = 1 << 20;

/// How often a running scan reports, in entries.
const PROGRESS_EVERY: u64 = 5_000;

/// How the cache names a folder: lower case and without a trailing separator, like the tag
/// index, because Windows paths are not case sensitive. (On a case-sensitive file system
/// `/a/Foo` and `/a/foo` would collide: fine while Windows is the target, to revisit in 1.1.)
pub(crate) fn key(path: &Path) -> String {
    let mut lower = path.to_string_lossy().to_lowercase();
    if cfg!(windows) {
        // `a/b` and `a` are the same folder.
        lower = lower.replace('/', "\\");
    }
    let trimmed = lower.trim_end_matches(['\\', '/']);
    if trimmed.is_empty() || trimmed.ends_with(':') {
        format!("{trimmed}{MAIN_SEPARATOR}")
    } else {
        trimmed.to_owned()
    }
}

fn parent_key(path: &Path) -> Option<String> {
    path.parent().map(key)
}

/// What the space breakdown sorts files into, by extension.
pub const CATEGORIES: [&str; 7] = [
    "Documents",
    "Images",
    "Video",
    "Audio",
    "Archives",
    "Apps",
    "Other",
];

/// The index into [`CATEGORIES`] of a file name.
pub(crate) fn category(name: &std::ffi::OsStr) -> usize {
    let ext = Path::new(name)
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "doc" | "docx" | "odt" | "rtf" | "txt" | "md" | "pdf" | "xls" | "xlsx" | "ods" | "csv"
        | "ppt" | "pptx" | "odp" | "epub" => 0,
        "jpg" | "jpeg" | "png" | "gif" | "bmp" | "webp" | "heic" | "tif" | "tiff" | "svg"
        | "raw" | "cr2" | "nef" | "psd" => 1,
        "mp4" | "mkv" | "avi" | "mov" | "wmv" | "webm" | "m4v" | "flv" => 2,
        "mp3" | "wav" | "flac" | "aac" | "ogg" | "m4a" | "wma" | "opus" => 3,
        "zip" | "rar" | "7z" | "tar" | "gz" | "bz2" | "xz" | "iso" | "cab" => 4,
        "exe" | "dll" | "msi" | "sys" | "apk" | "appx" | "msix" | "jar" => 5,
        _ => 6,
    }
}

/// What was found directly in one folder.
#[derive(Default, Clone)]
pub(crate) struct Acc {
    pub own: u64,
    pub own_files: u64,
    /// Bytes per category.
    pub cats: [u64; CATEGORIES.len()],
}

pub(crate) struct Big {
    pub path: PathBuf,
    pub dir: PathBuf,
    pub size: u64,
    pub mtime: i64,
}

/// A walked tree: each folder's own contents and the big files.
#[derive(Default)]
pub(crate) struct Tree {
    pub dirs: HashMap<PathBuf, Acc>,
    pub big: Vec<Big>,
    pub files: u64,
    pub bytes: u64,
}

/// The scan was cancelled.
struct Cancelled;

fn mtime_of(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// Walks `root` in parallel. Links are not followed, and what can't be read is skipped.
fn walk(root: &Path, cancel: &AtomicBool, progress: &dyn Fn(u64, u64)) -> Result<Tree, Cancelled> {
    let mut tree = Tree::default();
    let walker = WalkDirGeneric::<((), Option<(u64, i64)>)>::new(root)
        .skip_hidden(false)
        .follow_links(false)
        .process_read_dir(|_, _, (), children| {
            // The stat of each file runs on the walker's threads, not on this one.
            for child in children.iter_mut().flatten() {
                if child.file_type().is_file() {
                    child.client_state = child.metadata().ok().map(|m| (m.len(), mtime_of(&m)));
                }
            }
        });
    let mut seen = 0u64;
    for entry in walker {
        if cancel.load(Ordering::Relaxed) {
            return Err(Cancelled);
        }
        let Ok(entry) = entry else { continue };
        let file_type = entry.file_type();
        if file_type.is_dir() {
            tree.dirs.entry(entry.path()).or_default();
        } else if file_type.is_file() {
            let (size, mtime) = entry.client_state.unwrap_or((0, 0));
            let dir = entry.parent_path();
            let cat = category(entry.file_name());
            if let Some(acc) = tree.dirs.get_mut(dir) {
                acc.own += size;
                acc.own_files += 1;
                acc.cats[cat] += size;
            } else {
                let mut acc = Acc {
                    own: size,
                    own_files: 1,
                    ..Acc::default()
                };
                acc.cats[cat] = size;
                tree.dirs.insert(dir.to_path_buf(), acc);
            }
            tree.files += 1;
            tree.bytes += size;
            if size >= BIG_MIN {
                tree.big.push(Big {
                    path: entry.path(),
                    dir: dir.to_path_buf(),
                    size,
                    mtime,
                });
            }
        }
        seen += 1;
        if seen.is_multiple_of(PROGRESS_EVERY) {
            progress(tree.files, tree.bytes);
        }
    }
    progress(tree.files, tree.bytes);
    Ok(tree)
}

/// Each folder's total (bytes, files) including everything below it.
fn totals(tree: &Tree) -> HashMap<&Path, (u64, u64)> {
    let mut sums: HashMap<&Path, (u64, u64)> = tree
        .dirs
        .iter()
        .map(|(p, a)| (p.as_path(), (a.own, a.own_files)))
        .collect();
    // Deepest first, so a folder is complete before it is added to its parent.
    let mut order: Vec<&Path> = tree.dirs.keys().map(PathBuf::as_path).collect();
    order.sort_by_cached_key(|p| std::cmp::Reverse(p.components().count()));
    for path in order {
        let (bytes, files) = sums[path];
        if let Some(parent) = path.parent()
            && let Some(up) = sums.get_mut(parent)
        {
            up.0 += bytes;
            up.1 += files;
        }
    }
    sums
}

/// Bumped when the cache gains data that older scans lack.
const SCHEMA_VERSION: i64 = 1;

const SCHEMA: &str = "
    CREATE TABLE IF NOT EXISTS roots(
        root TEXT PRIMARY KEY, display TEXT NOT NULL, scanned_at INTEGER NOT NULL);
    CREATE TABLE IF NOT EXISTS dirs(
        path TEXT PRIMARY KEY, display TEXT NOT NULL, parent TEXT, root TEXT NOT NULL,
        own INTEGER NOT NULL, own_files INTEGER NOT NULL,
        total INTEGER NOT NULL, total_files INTEGER NOT NULL);
    CREATE INDEX IF NOT EXISTS dirs_parent ON dirs(parent);
    CREATE INDEX IF NOT EXISTS dirs_root ON dirs(root);
    CREATE TABLE IF NOT EXISTS big(
        path TEXT PRIMARY KEY, display TEXT NOT NULL, dir TEXT NOT NULL, root TEXT NOT NULL,
        size INTEGER NOT NULL, mtime INTEGER NOT NULL);
    CREATE INDEX IF NOT EXISTS big_dir ON big(dir);
    CREATE TABLE IF NOT EXISTS dir_cats(
        path TEXT NOT NULL, cat INTEGER NOT NULL, bytes INTEGER NOT NULL,
        PRIMARY KEY (path, cat)) WITHOUT ROWID;
    CREATE INDEX IF NOT EXISTS big_size ON big(size DESC);
";

/// A folder with its size, for the breakdown and the list view.
#[derive(Serialize, Debug, PartialEq, Eq)]
pub struct Sized {
    pub path: String,
    pub name: String,
    pub bytes: u64,
    pub files: u64,
}

/// What one folder holds, counted fresh from the disk.
#[derive(Serialize, Debug, PartialEq, Eq)]
pub struct Measured {
    pub bytes: u64,
    pub files: u64,
    pub folders: u64,
    /// Why it was not counted, if it was not; the numbers are then zero.
    pub skipped: Option<Skipped>,
}

/// Why a folder was not counted.
#[derive(Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Skipped {
    /// On a network share: reading the whole tree would go over the network.
    Network,
    /// A folder whose content is only in the cloud: listing it would make the provider fetch it.
    Cloud,
    /// Too big to count here: the top of a drive, or a count that took too long.
    Large,
}

impl Measured {
    /// The answer for a folder that was not counted.
    pub fn skip(why: Skipped) -> Self {
        Self {
            bytes: 0,
            files: 0,
            folders: 0,
            skipped: Some(why),
        }
    }
}

/// Whether a count nobody asked for (the folder being looked at) should not be made, and why:
/// the top of a drive or a network share is too much to read unasked, and a cloud-only folder
/// must not be listed.
pub fn unasked_skip(dir: &Path) -> Option<Skipped> {
    if crate::platform::is_network(dir) {
        Some(Skipped::Network)
    } else if crate::cleanup::is_cloud_only(dir) {
        Some(Skipped::Cloud)
    } else if dir.parent().is_none() {
        Some(Skipped::Large)
    } else {
        None
    }
}

/// A file from the big-file list.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct BigFile {
    pub path: String,
    pub size: u64,
    pub modified: i64,
}

/// A scanned root.
#[derive(Serialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RootInfo {
    pub root: String,
    pub scanned_at: i64,
    pub bytes: u64,
    pub files: u64,
}

fn as_u64(n: i64) -> u64 {
    u64::try_from(n).unwrap_or(0)
}

fn as_i64(n: u64) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

fn name_of(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map_or_else(|| path.to_owned(), |n| n.to_string_lossy().into_owned())
}

/// What one folder holds directly.
struct Folder {
    own: u64,
    own_files: u64,
    cats: [u64; CATEGORIES.len()],
    big: Vec<(PathBuf, u64, i64)>,
    subdirs: HashMap<String, PathBuf>,
}

/// Reads one folder from the disk; `None` if it can't be read.
fn read_folder(dir: &Path) -> Option<Folder> {
    let read = std::fs::read_dir(dir).ok()?;
    let mut folder = Folder {
        own: 0,
        own_files: 0,
        cats: [0; CATEGORIES.len()],
        big: Vec::new(),
        subdirs: HashMap::new(),
    };
    for entry in read.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            folder.subdirs.insert(key(&entry.path()), entry.path());
        } else if kind.is_file() {
            // Counted even when it can't be statted, as a scan does.
            let (size, mtime) = entry.metadata().map_or((0, 0), |m| (m.len(), mtime_of(&m)));
            folder.own += size;
            folder.own_files += 1;
            folder.cats[category(&entry.file_name())] += size;
            if size >= BIG_MIN {
                folder.big.push((entry.path(), size, mtime));
            }
        }
    }
    Some(folder)
}

/// The cache and what keeps it fresh.
pub struct Scanner {
    inner: Arc<Inner>,
}

struct Inner {
    db: Mutex<Connection>,
    /// The cancel flag of the scan in progress, if one is.
    running: Mutex<Option<Arc<AtomicBool>>>,
    watchers: Mutex<HashMap<String, Debouncer<RecommendedWatcher>>>,
    /// Folders that changed while a scan was running; refreshed once it is stored.
    dirty: Mutex<Vec<PathBuf>>,
}

/// How a scan ended.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Done { files: u64, bytes: u64 },
    Cancelled,
}

impl Scanner {
    /// Opens the cache; `None` keeps it in memory.
    pub fn open(file: Option<&Path>) -> rusqlite::Result<Self> {
        let db = match file {
            Some(f) => Connection::open(f)?,
            None => Connection::open_in_memory()?,
        };
        db.execute_batch(
            "PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL;
             PRAGMA busy_timeout = 5000;",
        )?;
        db.execute_batch(SCHEMA)?;
        // A cache from before the category sums has none for what it holds: drop it, so
        // the roots are scanned again rather than shown with a wrong breakdown.
        let version: i64 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version < SCHEMA_VERSION {
            db.execute_batch(
                "DELETE FROM roots; DELETE FROM dirs; DELETE FROM big; DELETE FROM dir_cats;",
            )?;
            db.execute_batch(&format!("PRAGMA user_version = {SCHEMA_VERSION}"))?;
        }
        Ok(Self {
            inner: Arc::new(Inner {
                db: Mutex::new(db),
                running: Mutex::new(None),
                watchers: Mutex::new(HashMap::new()),
                dirty: Mutex::new(Vec::new()),
            }),
        })
    }

    fn db(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.inner.db.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Claims the single scan slot, or `None` if a scan is already running.
    pub fn begin(&self) -> Option<Arc<AtomicBool>> {
        let mut running = self
            .inner
            .running
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if running.is_some() {
            return None;
        }
        let flag = Arc::new(AtomicBool::new(false));
        *running = Some(flag.clone());
        Some(flag)
    }

    /// Stops the scan in progress; a no-op if none is running.
    pub fn cancel(&self) {
        if let Some(flag) = &*self
            .inner
            .running
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
        {
            flag.store(true, Ordering::Relaxed);
        }
    }

    pub fn is_running(&self) -> bool {
        self.inner
            .running
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_some()
    }

    /// Scans `root` and replaces what the cache holds for it. A cancelled scan changes
    /// nothing. The slot taken by [`begin`](Self::begin) is released when this returns, or
    /// unwinds. The root is watched from before the walk, and what changed meanwhile is
    /// refreshed after it is stored.
    #[cfg(test)]
    pub fn scan(
        &self,
        root: &Path,
        cancel: &AtomicBool,
        progress: &dyn Fn(u64, u64),
    ) -> Result<Outcome, String> {
        self.scan_with(root, cancel, progress, false)
            .map(|(outcome, _)| outcome)
    }

    /// Like [`scan`](Self::scan), reading the drive's file table through the elevated helper
    /// when `fast` is set. When that can't be done the folder is walked instead; the second
    /// part of the answer is then the reason, for the person to be told.
    pub fn scan_with(
        &self,
        root: &Path,
        cancel: &AtomicBool,
        progress: &dyn Fn(u64, u64),
        fast: bool,
    ) -> Result<(Outcome, Option<String>), String> {
        struct Slot<'a>(&'a Inner);
        impl Drop for Slot<'_> {
            fn drop(&mut self) {
                *self
                    .0
                    .running
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner) = None;
            }
        }
        let slot = Slot(&self.inner);
        let was_cached = self.size_of(root).is_some();
        let watching = self.watch(root).is_ok();
        let mut note = None;
        let result = (|| {
            let found = if fast {
                match crate::mft::scan(root, cancel, progress) {
                    crate::mft::Fast::Tree(tree) => Some(tree),
                    crate::mft::Fast::Cancelled => return Ok(Outcome::Cancelled),
                    crate::mft::Fast::Fallback(why) => {
                        note = Some(why);
                        None
                    }
                }
            } else {
                None
            };
            let tree = match found {
                Some(tree) => tree,
                None => match walk(root, cancel, progress) {
                    Ok(tree) => tree,
                    Err(Cancelled) => return Ok(Outcome::Cancelled),
                },
            };
            if cancel.load(Ordering::Relaxed) {
                return Ok(Outcome::Cancelled);
            }
            self.store(root, &tree).map_err(|e| e.to_string())?;
            Ok(Outcome::Done {
                files: tree.files,
                bytes: tree.bytes,
            })
        })();
        drop(slot);
        if matches!(result, Ok(Outcome::Done { .. })) {
            let dirty = std::mem::take(
                &mut *self
                    .inner
                    .dirty
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner),
            );
            for dir in dirty {
                let _ = self.refresh_dir(&dir);
            }
        } else {
            self.inner
                .dirty
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clear();
            if watching && !was_cached {
                self.unwatch(root);
            }
        }
        result.map(|outcome| (outcome, note))
    }

    fn store(&self, root: &Path, tree: &Tree) -> rusqlite::Result<()> {
        let root_key = key(root);
        let sums = totals(tree);
        let mut db = self.db();
        let tx = db.transaction()?;
        let (old_bytes, old_files): (i64, i64) = tx
            .query_row(
                "SELECT total, total_files FROM dirs WHERE path = ?1",
                [&root_key],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap_or((0, 0));
        // Everything cached at or below the root goes, whichever scan put it there; a root
        // nested inside it is replaced along with it.
        let (start, end) = prefix_range(&root_key);
        delete_subtree(&tx, &root_key)?;
        tx.execute(
            "DELETE FROM roots WHERE root = ?1 OR (root >= ?2 AND root < ?3)",
            params![root_key, start, end],
        )?;
        insert_tree(&tx, &root_key, tree, &sums)?;
        tx.execute(
            "INSERT OR REPLACE INTO roots (root, display, scanned_at) VALUES (?1, ?2, ?3)",
            params![root_key, root.to_string_lossy(), crate::journal::unix_now()],
        )?;
        // A root inside another scanned root changes the sizes above it.
        let (new_bytes, new_files) = sums.get(root).copied().unwrap_or((0, 0));
        add_above(
            &tx,
            root,
            as_i64(new_bytes) - old_bytes,
            as_i64(new_files) - old_files,
        )?;
        tx.commit()
    }

    /// Re-reads one folder's own contents and brings the cache and every folder above it up
    /// to date: new subfolders are scanned, vanished ones dropped. Does nothing for a folder
    /// outside every scanned root. The cache is locked only to look and to write, never
    /// while the disk is read.
    pub fn refresh_dir(&self, dir: &Path) -> rusqlite::Result<()> {
        let dir_key = key(dir);
        let known_before: Vec<String> = {
            let db = self.db();
            let cached = db
                .query_row("SELECT 1 FROM dirs WHERE path = ?1", [&dir_key], |_| Ok(()))
                .is_ok();
            if !cached {
                return Ok(());
            }
            let mut stmt = db.prepare("SELECT path FROM dirs WHERE parent = ?1")?;
            stmt.query_map([&dir_key], |r| r.get(0))?
                .collect::<Result<_, _>>()?
        };
        let Some(Folder {
            own,
            own_files,
            cats,
            big,
            subdirs,
        }) = read_folder(dir)
        else {
            // Gone (or unreadable): the parent's refresh drops it; here just empty it.
            return remove_subtree(&mut self.db(), dir, &dir_key);
        };
        // Subfolders that appeared are walked before the cache is locked.
        let never = AtomicBool::new(false);
        let walked: Vec<(PathBuf, Tree)> = subdirs
            .iter()
            .filter(|(k, _)| !known_before.contains(k))
            .filter_map(|(_, p)| walk(p, &never, &|_, _| {}).ok().map(|t| (p.clone(), t)))
            .collect();

        let mut db = self.db();
        let tx = db.transaction()?;
        let Ok((root_key, old_own, old_own_files)) = tx.query_row(
            "SELECT root, own, own_files FROM dirs WHERE path = ?1",
            [&dir_key],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                ))
            },
        ) else {
            return Ok(());
        };
        let known: Vec<(String, i64, i64)> = {
            let mut stmt =
                tx.prepare("SELECT path, total, total_files FROM dirs WHERE parent = ?1")?;
            stmt.query_map([&dir_key], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
                .collect::<Result<_, _>>()?
        };
        let mut delta_bytes = as_i64(own) - old_own;
        let mut delta_files = as_i64(own_files) - old_own_files;
        for (path, total, total_files) in &known {
            if !subdirs.contains_key(path) {
                delta_bytes -= total;
                delta_files -= total_files;
                delete_subtree(&tx, path)?;
            }
        }
        for (path, tree) in &walked {
            // Another refresh may have added it meanwhile, or it may have vanished again.
            if known.iter().any(|(k, ..)| *k == key(path)) {
                continue;
            }
            let sums = totals(tree);
            let Some(&(total, total_files)) = sums.get(path.as_path()) else {
                continue;
            };
            delta_bytes += as_i64(total);
            delta_files += as_i64(total_files);
            insert_tree(&tx, &root_key, tree, &sums)?;
        }
        tx.execute(
            "UPDATE dirs SET own = ?2, own_files = ?3 WHERE path = ?1",
            params![dir_key, as_i64(own), as_i64(own_files)],
        )?;
        replace_own(&tx, &dir_key, &root_key, &cats, &big)?;
        add_to_ancestors(&tx, dir, delta_bytes, delta_files)?;
        tx.commit()
    }

    /// The subfolders of `dir` with their sizes, largest first.
    pub fn children(&self, dir: &Path) -> rusqlite::Result<Vec<Sized>> {
        let db = self.db();
        let mut stmt = db.prepare(
            "SELECT display, total, total_files FROM dirs WHERE parent = ?1
             ORDER BY total DESC, path",
        )?;
        stmt.query_map([key(dir)], |r| {
            let display: String = r.get(0)?;
            Ok(Sized {
                name: name_of(&display),
                path: display,
                bytes: as_u64(r.get(1)?),
                files: as_u64(r.get(2)?),
            })
        })?
        .collect()
    }

    /// The size of `dir` and everything in it, if it has been scanned.
    pub fn size_of(&self, dir: &Path) -> Option<(u64, u64)> {
        self.db()
            .query_row(
                "SELECT total, total_files FROM dirs WHERE path = ?1",
                [key(dir)],
                |r| Ok((as_u64(r.get(0)?), as_u64(r.get(1)?))),
            )
            .ok()
    }

    /// Bytes per category ([`CATEGORIES`]) in `dir` and everything below it.
    pub fn categories(&self, dir: &Path) -> rusqlite::Result<Vec<u64>> {
        let dir_key = key(dir);
        let (start, end) = prefix_range(&dir_key);
        let db = self.db();
        let mut stmt = db.prepare(
            "SELECT cat, SUM(bytes) FROM dir_cats
             WHERE path = ?1 OR (path >= ?2 AND path < ?3) GROUP BY cat",
        )?;
        let mut out = vec![0; CATEGORIES.len()];
        let rows = stmt.query_map(params![dir_key, start, end], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))
        })?;
        for row in rows {
            let (cat, bytes) = row?;
            if let Some(slot) = usize::try_from(cat).ok().and_then(|c| out.get_mut(c)) {
                *slot = as_u64(bytes);
            }
        }
        Ok(out)
    }

    /// The biggest files of at least `min` bytes, largest first.
    pub fn large(&self, min: u64, limit: u32) -> rusqlite::Result<Vec<BigFile>> {
        let db = self.db();
        let mut stmt = db.prepare(
            "SELECT display, size, mtime FROM big WHERE size >= ?1 ORDER BY size DESC LIMIT ?2",
        )?;
        stmt.query_map(params![as_i64(min), limit], |r| {
            Ok(BigFile {
                path: r.get(0)?,
                size: as_u64(r.get(1)?),
                modified: r.get(2)?,
            })
        })?
        .collect()
    }

    /// The biggest files of at least `min` bytes last changed at or before `before`
    /// (seconds since 1970), largest first, that `keep` accepts, up to `limit`. The cache
    /// is read a page at a time and unlocked while `keep` runs, so a filter that touches the
    /// disk (a slow share, a hung drive) never holds up a scan or the Storage view.
    pub fn large_before(
        &self,
        min: u64,
        before: i64,
        limit: usize,
        mut keep: impl FnMut(&BigFile) -> bool,
    ) -> rusqlite::Result<Vec<BigFile>> {
        const PAGE: usize = 200;
        let mut found = Vec::new();
        // The last row of the previous page: the next one starts after it.
        let mut after: Option<(i64, String)> = None;
        loop {
            let page: Vec<(i64, String, BigFile)> = {
                let db = self.db();
                let mut stmt = db.prepare(
                    "SELECT path, display, size, mtime FROM big
                     WHERE size >= ?1 AND mtime <= ?2
                       AND (?3 IS NULL OR size < ?3 OR (size = ?3 AND path > ?4))
                     ORDER BY size DESC, path LIMIT ?5",
                )?;
                let (size, path) = after
                    .clone()
                    .map_or((None, None), |(s, p)| (Some(s), Some(p)));
                stmt.query_map(
                    params![
                        as_i64(min),
                        before,
                        size,
                        path,
                        i64::try_from(PAGE).unwrap_or(200)
                    ],
                    |r| {
                        Ok((
                            r.get::<_, i64>(2)?,
                            r.get::<_, String>(0)?,
                            BigFile {
                                path: r.get(1)?,
                                size: as_u64(r.get(2)?),
                                modified: r.get(3)?,
                            },
                        ))
                    },
                )?
                .collect::<Result<_, _>>()?
            };
            let Some((size, path, _)) = page.last() else {
                return Ok(found);
            };
            after = Some((*size, path.clone()));
            let last_page = page.len() < PAGE;
            for (_, _, file) in page {
                if keep(&file) {
                    found.push(file);
                    if found.len() >= limit {
                        return Ok(found);
                    }
                }
            }
            if last_page {
                return Ok(found);
            }
        }
    }

    /// Scanned files of at least `min` bytes whose size another scanned file has too,
    /// biggest size first, up to `limit`. Candidates for a duplicate search.
    pub fn same_size(&self, min: u64, limit: usize) -> rusqlite::Result<Vec<BigFile>> {
        let db = self.db();
        let mut stmt = db.prepare(
            "SELECT display, size, mtime FROM big
             WHERE size >= ?1 AND size IN (SELECT size FROM big GROUP BY size HAVING COUNT(*) > 1)
             ORDER BY size DESC, path LIMIT ?2",
        )?;
        stmt.query_map(
            params![as_i64(min), i64::try_from(limit).unwrap_or(i64::MAX)],
            |r| {
                Ok(BigFile {
                    path: r.get(0)?,
                    size: as_u64(r.get(1)?),
                    modified: r.get(2)?,
                })
            },
        )?
        .collect()
    }

    /// Scanned folders and files (of 1 MB or more) whose names say they are unfinished copies,
    /// as the scan spelled them, up to `limit`.
    pub fn partial_candidates(&self, limit: usize) -> rusqlite::Result<Vec<String>> {
        let db = self.db();
        let mut stmt = db.prepare(
            "SELECT display FROM dirs WHERE path LIKE '%.folio-partial'
             UNION SELECT display FROM big WHERE path LIKE '%.folio-partial' LIMIT ?1",
        )?;
        stmt.query_map([i64::try_from(limit).unwrap_or(i64::MAX)], |r| r.get(0))?
            .collect()
    }

    /// Every scanned root, with its size.
    pub fn roots(&self) -> rusqlite::Result<Vec<RootInfo>> {
        let db = self.db();
        let mut stmt = db.prepare(
            "SELECT r.display, r.scanned_at, d.total, d.total_files
             FROM roots r JOIN dirs d ON d.path = r.root ORDER BY r.display",
        )?;
        stmt.query_map([], |r| {
            Ok(RootInfo {
                root: r.get(0)?,
                scanned_at: r.get(1)?,
                bytes: as_u64(r.get(2)?),
                files: as_u64(r.get(3)?),
            })
        })?
        .collect()
    }

    /// Keeps `root` current from now on: every change below it re-reads the folder it
    /// happened in, after a short pause so a burst is one refresh.
    pub fn watch(&self, root: &Path) -> notify_debouncer_mini::notify::Result<()> {
        let inner = Arc::downgrade(&self.inner);
        let mut debouncer = new_debouncer(
            Duration::from_millis(500),
            move |res: DebounceEventResult| {
                let (Ok(events), Some(inner)) = (res, inner.upgrade()) else {
                    return;
                };
                let mut dirs: Vec<PathBuf> = events
                    .iter()
                    .filter_map(|e| e.path.parent().map(Path::to_path_buf))
                    .collect();
                dirs.sort();
                dirs.dedup();
                let scanner = Scanner { inner };
                if scanner.is_running() {
                    // The scan will replace the cache; these are replayed after it.
                    scanner
                        .inner
                        .dirty
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .extend(dirs);
                    return;
                }
                for dir in dirs {
                    let _ = scanner.refresh_dir(&dir);
                }
            },
        )?;
        debouncer.watcher().watch(root, RecursiveMode::Recursive)?;
        self.inner
            .watchers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(key(root), debouncer);
        Ok(())
    }
}

impl Scanner {
    fn unwatch(&self, root: &Path) {
        self.inner
            .watchers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&key(root));
    }

    /// Watches every root scanned in an earlier run, so the cache stays current again.
    /// Changes made while Folio was closed are only seen by scanning the root again.
    pub fn resume_watching(&self) {
        for root in self.roots().unwrap_or_default() {
            let _ = self.watch(Path::new(&root.root));
        }
    }
}

fn insert_tree(
    tx: &rusqlite::Transaction<'_>,
    root_key: &str,
    tree: &Tree,
    sums: &HashMap<&Path, (u64, u64)>,
) -> rusqlite::Result<()> {
    let mut dir = tx.prepare_cached(
        "INSERT OR REPLACE INTO dirs
         (path, display, parent, root, own, own_files, total, total_files)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
    )?;
    for (path, acc) in &tree.dirs {
        let (total, total_files) = sums.get(path.as_path()).copied().unwrap_or((0, 0));
        dir.execute(params![
            key(path),
            path.to_string_lossy(),
            parent_key(path),
            root_key,
            as_i64(acc.own),
            as_i64(acc.own_files),
            as_i64(total),
            as_i64(total_files),
        ])?;
    }
    let mut cats = tx
        .prepare_cached("INSERT OR REPLACE INTO dir_cats (path, cat, bytes) VALUES (?1, ?2, ?3)")?;
    for (path, acc) in &tree.dirs {
        for (cat, bytes) in acc.cats.iter().enumerate() {
            if *bytes > 0 {
                cats.execute(params![key(path), as_i64(cat as u64), as_i64(*bytes)])?;
            }
        }
    }
    let mut big = tx.prepare_cached(
        "INSERT OR REPLACE INTO big (path, display, dir, root, size, mtime)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )?;
    for f in &tree.big {
        big.execute(params![
            key(&f.path),
            f.path.to_string_lossy(),
            key(&f.dir),
            root_key,
            as_i64(f.size),
            f.mtime
        ])?;
    }
    Ok(())
}

/// Replaces the category sums and big files kept for the files directly in one folder.
fn replace_own(
    tx: &rusqlite::Transaction<'_>,
    dir_key: &str,
    root_key: &str,
    cats: &[u64],
    big: &[(PathBuf, u64, i64)],
) -> rusqlite::Result<()> {
    tx.execute("DELETE FROM dir_cats WHERE path = ?1", [dir_key])?;
    for (cat, bytes) in cats.iter().enumerate() {
        if *bytes > 0 {
            tx.execute(
                "INSERT INTO dir_cats (path, cat, bytes) VALUES (?1, ?2, ?3)",
                params![dir_key, as_i64(cat as u64), as_i64(*bytes)],
            )?;
        }
    }
    tx.execute("DELETE FROM big WHERE dir = ?1", [dir_key])?;
    for (path, size, mtime) in big {
        tx.execute(
            "INSERT OR REPLACE INTO big (path, display, dir, root, size, mtime)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                key(path),
                path.to_string_lossy(),
                dir_key,
                root_key,
                as_i64(*size),
                mtime
            ],
        )?;
    }
    Ok(())
}

/// The range of keys that lie below `dir_key`: from `dir_key\` up to, not including, the
/// next character after the separator. A range can use the index; a prefix test cannot.
fn prefix_range(dir_key: &str) -> (String, String) {
    let mut start = dir_key.trim_end_matches(MAIN_SEPARATOR).to_owned();
    start.push(MAIN_SEPARATOR);
    let mut end = start.clone();
    end.pop();
    end.push(char::from_u32(u32::from(MAIN_SEPARATOR) + 1).unwrap_or(MAIN_SEPARATOR));
    (start, end)
}

/// Removes a folder and everything below it from the cache.
fn delete_subtree(tx: &rusqlite::Transaction<'_>, dir_key: &str) -> rusqlite::Result<()> {
    let (start, end) = prefix_range(dir_key);
    tx.execute(
        "DELETE FROM dirs WHERE path = ?1 OR (path >= ?2 AND path < ?3)",
        params![dir_key, start, end],
    )?;
    tx.execute(
        "DELETE FROM big WHERE dir = ?1 OR (dir >= ?2 AND dir < ?3)",
        params![dir_key, start, end],
    )?;
    tx.execute(
        "DELETE FROM dir_cats WHERE path = ?1 OR (path >= ?2 AND path < ?3)",
        params![dir_key, start, end],
    )?;
    Ok(())
}

/// A folder that can no longer be read: its cached contents are dropped and its ancestors
/// lose what it held. The folder's own row stays until its parent is refreshed.
fn remove_subtree(db: &mut Connection, dir: &Path, dir_key: &str) -> rusqlite::Result<()> {
    let tx = db.transaction()?;
    let Ok((total, files)) = tx.query_row(
        "SELECT total, total_files FROM dirs WHERE path = ?1",
        [dir_key],
        |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)),
    ) else {
        return Ok(());
    };
    let (start, end) = prefix_range(dir_key);
    tx.execute(
        "DELETE FROM dirs WHERE path >= ?1 AND path < ?2",
        params![start, end],
    )?;
    tx.execute(
        "DELETE FROM big WHERE dir = ?1 OR (dir >= ?2 AND dir < ?3)",
        params![dir_key, start, end],
    )?;
    tx.execute(
        "DELETE FROM dir_cats WHERE path = ?1 OR (path >= ?2 AND path < ?3)",
        params![dir_key, start, end],
    )?;
    tx.execute(
        "UPDATE dirs SET own = 0, own_files = 0 WHERE path = ?1",
        [dir_key],
    )?;
    add_to_ancestors(&tx, dir, -total, -files)?;
    tx.commit()
}

/// Adds a change in size to `dir` and every folder above it that the cache knows.
fn add_to_ancestors(
    tx: &rusqlite::Transaction<'_>,
    dir: &Path,
    bytes: i64,
    files: i64,
) -> rusqlite::Result<()> {
    for ancestor in dir.ancestors() {
        tx.execute(
            "UPDATE dirs SET total = total + ?2, total_files = total_files + ?3 WHERE path = ?1",
            params![key(ancestor), bytes, files],
        )?;
    }
    Ok(())
}

/// The same for the folders above `dir` only.
fn add_above(
    tx: &rusqlite::Transaction<'_>,
    dir: &Path,
    bytes: i64,
    files: i64,
) -> rusqlite::Result<()> {
    match dir.parent() {
        Some(parent) if bytes != 0 || files != 0 => add_to_ancestors(tx, parent, bytes, files),
        _ => Ok(()),
    }
}

/// Counts what is in `root` and below it, straight from the disk and without the cache, for
/// a folder the user is looking at. Links are not followed and what can't be read is
/// skipped. A folder whose content is only in the cloud is counted as a folder but not
/// entered: listing it would make the sync provider fetch it. `None` if `cancel` was set
/// before it finished.
pub fn measure(root: &Path, cancel: &AtomicBool) -> Option<Measured> {
    let mut out = Measured {
        bytes: 0,
        files: 0,
        folders: 0,
        skipped: None,
    };
    let walker = WalkDirGeneric::<((), Option<u64>)>::new(root)
        .skip_hidden(false)
        .follow_links(false)
        .process_read_dir(|_, _, (), children| {
            for child in children.iter_mut().flatten() {
                if child.file_type().is_file() {
                    child.client_state = child.metadata().ok().map(|m| m.len());
                } else if child.file_type().is_dir()
                    && child
                        .metadata()
                        .is_ok_and(|m| crate::cleanup::cloud_only_attributes(&m))
                {
                    child.read_children = None;
                }
            }
        });
    for entry in walker {
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        let Ok(entry) = entry else { continue };
        if entry.file_type().is_dir() {
            // The root itself is not one of the folders it holds.
            out.folders += u64::from(entry.depth > 0);
        } else if entry.file_type().is_file() {
            out.files += 1;
            out.bytes += entry.client_state.unwrap_or(0);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write(path: &Path, bytes: usize) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, vec![0u8; bytes]).unwrap();
    }

    fn scanned(root: &Path) -> Scanner {
        let scanner = Scanner::open(None).unwrap();
        let flag = scanner.begin().unwrap();
        let outcome = scanner.scan(root, &flag, &|_, _| {}).unwrap();
        assert!(matches!(outcome, Outcome::Done { .. }));
        scanner
    }

    fn sample() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("a.bin"), 10);
        write(&dir.path().join("sub/b.bin"), 100);
        write(&dir.path().join("sub/deep/c.bin"), 1_000);
        write(
            &dir.path().join("sub/deep/big.bin"),
            usize::try_from(BIG_MIN).unwrap(),
        );
        write(&dir.path().join("other/d.bin"), 5);
        dir
    }

    #[test]
    fn a_scan_totals_every_folder_and_lists_the_big_files() {
        let dir = sample();
        let s = scanned(dir.path());
        let big = BIG_MIN + 1_000;
        assert_eq!(s.size_of(dir.path()), Some((big + 115, 5)));
        assert_eq!(s.size_of(&dir.path().join("sub")), Some((big + 100, 3)));
        assert_eq!(s.size_of(&dir.path().join("sub/deep")), Some((big, 2)));
        let kids = s.children(dir.path()).unwrap();
        assert_eq!(
            kids.iter().map(|k| k.name.as_str()).collect::<Vec<_>>(),
            ["sub", "other"],
            "largest first"
        );
        let large = s.large(BIG_MIN, 10).unwrap();
        assert_eq!(large.len(), 1);
        assert_eq!(large[0].size, BIG_MIN);
        assert_eq!(s.roots().unwrap().len(), 1);
    }

    #[test]
    fn a_cancelled_scan_leaves_the_cache_as_it_was() {
        let dir = sample();
        let s = scanned(dir.path());
        write(&dir.path().join("late.bin"), 7_000);
        let flag = s.begin().unwrap();
        flag.store(true, Ordering::Relaxed);
        assert_eq!(
            s.scan(dir.path(), &flag, &|_, _| {}).unwrap(),
            Outcome::Cancelled
        );
        assert_eq!(s.size_of(dir.path()).unwrap().1, 5, "the old numbers stay");
        assert!(!s.is_running(), "the slot is released");
    }

    #[test]
    fn the_cache_survives_a_restart() {
        let dir = sample();
        let file = tempfile::tempdir().unwrap();
        let file = file.path().join("scan.sqlite");
        {
            let s = Scanner::open(Some(&file)).unwrap();
            let flag = s.begin().unwrap();
            s.scan(dir.path(), &flag, &|_, _| {}).unwrap();
        }
        let s = Scanner::open(Some(&file)).unwrap();
        assert_eq!(s.size_of(dir.path()).unwrap().1, 5);
    }

    #[test]
    fn scanning_a_folder_inside_a_scanned_root_keeps_the_root_right() {
        let dir = sample();
        let s = scanned(dir.path());
        write(&dir.path().join("sub/more.bin"), 700);
        fs::remove_file(dir.path().join("sub/b.bin")).unwrap();
        let flag = s.begin().unwrap();
        s.scan(&dir.path().join("sub"), &flag, &|_, _| {}).unwrap();
        let big = BIG_MIN + 1_000;
        assert_eq!(s.size_of(&dir.path().join("sub")), Some((big + 700, 3)));
        assert_eq!(s.size_of(dir.path()), Some((big + 700 + 15, 5)));
    }

    #[test]
    fn rescanning_a_root_replaces_a_root_inside_it() {
        let dir = sample();
        let s = scanned(&dir.path().join("sub"));
        let flag = s.begin().unwrap();
        s.scan(dir.path(), &flag, &|_, _| {}).unwrap();
        fs::remove_dir_all(dir.path().join("sub/deep")).unwrap();
        let flag = s.begin().unwrap();
        s.scan(dir.path(), &flag, &|_, _| {}).unwrap();
        assert_eq!(s.size_of(&dir.path().join("sub/deep")), None);
        assert_eq!(s.size_of(dir.path()), Some((115, 3)));
        assert_eq!(s.roots().unwrap().len(), 1);
    }

    #[test]
    fn a_panic_in_a_scan_does_not_keep_the_slot() {
        let s = Scanner::open(None).unwrap();
        let flag = s.begin().unwrap();
        let dir = sample();
        let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = s.scan(dir.path(), &flag, &|_, _| panic!("progress failed"));
        }));
        assert!(caught.is_err());
        assert!(!s.is_running());
    }

    #[test]
    fn bytes_are_sorted_into_categories_below_a_folder() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("a.jpg"), 100);
        write(&dir.path().join("sub/b.PNG"), 50);
        write(&dir.path().join("sub/c.mp4"), 1_000);
        write(&dir.path().join("sub/d.unknown"), 7);
        let s = scanned(dir.path());
        let all = s.categories(dir.path()).unwrap();
        assert_eq!(all, [0, 150, 1_000, 0, 0, 0, 7]);
        let sub = s.categories(&dir.path().join("sub")).unwrap();
        assert_eq!(sub, [0, 50, 1_000, 0, 0, 0, 7]);
        // A refresh keeps them current.
        write(&dir.path().join("sub/e.mp3"), 30);
        fs::remove_file(dir.path().join("sub/c.mp4")).unwrap();
        s.refresh_dir(&dir.path().join("sub")).unwrap();
        assert_eq!(s.categories(dir.path()).unwrap(), [0, 150, 0, 30, 0, 0, 7]);
    }

    #[test]
    fn a_cache_from_before_the_categories_is_dropped() {
        let dir = sample();
        let file = tempfile::tempdir().unwrap();
        let file = file.path().join("scan.sqlite");
        {
            let s = Scanner::open(Some(&file)).unwrap();
            let flag = s.begin().unwrap();
            s.scan(dir.path(), &flag, &|_, _| {}).unwrap();
            s.db().execute_batch("PRAGMA user_version = 0").unwrap();
        }
        let s = Scanner::open(Some(&file)).unwrap();
        assert_eq!(s.roots().unwrap(), []);
        assert_eq!(s.size_of(dir.path()), None);
    }

    #[test]
    fn only_one_scan_runs_at_a_time() {
        let s = Scanner::open(None).unwrap();
        let flag = s.begin().unwrap();
        assert!(s.begin().is_none());
        s.cancel();
        assert!(flag.load(Ordering::Relaxed));
    }

    #[test]
    fn refreshing_a_folder_updates_it_and_every_folder_above() {
        let dir = sample();
        let s = scanned(dir.path());
        let before = s.size_of(dir.path()).unwrap();
        write(&dir.path().join("sub/deep/new.bin"), 500);
        fs::remove_file(dir.path().join("sub/b.bin")).unwrap();
        s.refresh_dir(&dir.path().join("sub/deep")).unwrap();
        s.refresh_dir(&dir.path().join("sub")).unwrap();
        assert_eq!(s.size_of(dir.path()), Some((before.0 + 500 - 100, 5)));
        assert_eq!(s.size_of(&dir.path().join("sub/deep")).unwrap().1, 3);
    }

    #[test]
    fn a_new_subfolder_is_scanned_and_a_removed_one_is_dropped() {
        let dir = sample();
        let s = scanned(dir.path());
        let before = s.size_of(dir.path()).unwrap().0;
        write(&dir.path().join("fresh/x/y.bin"), 2_000);
        fs::remove_dir_all(dir.path().join("other")).unwrap();
        s.refresh_dir(dir.path()).unwrap();
        assert_eq!(s.size_of(dir.path()).unwrap().0, before + 2_000 - 5);
        assert_eq!(s.size_of(&dir.path().join("fresh/x")), Some((2_000, 1)));
        assert_eq!(s.size_of(&dir.path().join("other")), None);
        let names: Vec<_> = s
            .children(dir.path())
            .unwrap()
            .into_iter()
            .map(|k| k.name)
            .collect();
        assert!(names.contains(&"fresh".to_owned()) && !names.contains(&"other".to_owned()));
    }

    #[test]
    fn a_big_file_that_is_removed_leaves_the_list() {
        let dir = sample();
        let s = scanned(dir.path());
        fs::remove_file(dir.path().join("sub/deep/big.bin")).unwrap();
        s.refresh_dir(&dir.path().join("sub/deep")).unwrap();
        assert_eq!(s.large(BIG_MIN, 10).unwrap(), []);
    }

    #[test]
    fn a_folder_outside_every_scanned_root_is_ignored() {
        let dir = sample();
        let s = scanned(&dir.path().join("sub"));
        let outside = tempfile::tempdir().unwrap();
        s.refresh_dir(outside.path()).unwrap();
        assert_eq!(s.size_of(outside.path()), None);
        // The root's sibling is not cached either.
        assert_eq!(s.size_of(&dir.path().join("other")), None);
    }

    #[test]
    fn folder_names_are_not_case_sensitive() {
        let dir = sample();
        let s = scanned(dir.path());
        let upper = PathBuf::from(dir.path().to_string_lossy().to_uppercase());
        if cfg!(windows) {
            assert_eq!(s.size_of(&upper), s.size_of(dir.path()));
        }
    }

    #[test]
    fn the_watcher_keeps_a_scanned_root_current() {
        let dir = sample();
        let s = scanned(dir.path());
        s.watch(dir.path()).unwrap();
        let before = s.size_of(dir.path()).unwrap();
        write(&dir.path().join("sub/deep/added.bin"), 4_000);
        fs::remove_file(dir.path().join("other/d.bin")).unwrap();
        let want = (before.0 + 4_000 - 5, before.1);
        let mut got = s.size_of(dir.path());
        for _ in 0..100 {
            if got == Some(want) {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
            got = s.size_of(dir.path());
        }
        assert_eq!(got, Some(want));
    }

    /// A real scan, for timing: `FOLIO_SCAN_ROOT=C:\Windows cargo test -- --ignored --nocapture
    /// scan_a_real_folder`.
    #[test]
    #[ignore = "scans a real folder named by FOLIO_SCAN_ROOT"]
    fn scan_a_real_folder() {
        let root = PathBuf::from(std::env::var("FOLIO_SCAN_ROOT").expect("FOLIO_SCAN_ROOT"));
        let s = Scanner::open(None).unwrap();
        let flag = s.begin().unwrap();
        let t = std::time::Instant::now();
        let outcome = s.scan(&root, &flag, &|_, _| {}).unwrap();
        println!("scanned {outcome:?} in {:?}", t.elapsed());
        let t = std::time::Instant::now();
        let top = s.children(&root).unwrap();
        println!(
            "children of the root in {:?}: {} folders",
            t.elapsed(),
            top.len()
        );
    }

    #[test]
    fn measure_counts_everything_below_the_folder() {
        let dir = sample();
        let got = measure(dir.path(), &AtomicBool::new(false)).unwrap();
        assert_eq!(got.files, 5);
        assert_eq!(got.folders, 3);
        assert_eq!(got.bytes, 10 + 100 + 1_000 + BIG_MIN + 5);
    }

    #[test]
    fn measuring_a_folder_of_only_subfolders_counts_what_is_inside_them() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("a/one.bin"), 30);
        write(&dir.path().join("b/c/two.bin"), 70);
        let got = measure(dir.path(), &AtomicBool::new(false)).unwrap();
        assert_eq!((got.bytes, got.files, got.folders), (100, 2, 3));
        assert_eq!(got.skipped, None);
    }

    #[test]
    fn an_unasked_count_skips_a_drive_top_and_not_an_ordinary_folder() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(unasked_skip(dir.path()), None);
        let top = if cfg!(windows) { "C:\\" } else { "/" };
        assert_eq!(unasked_skip(Path::new(top)), Some(Skipped::Large));
        assert_eq!(
            unasked_skip(Path::new("\\\\server\\share\\a")).is_some(),
            cfg!(windows)
        );
    }

    #[cfg(windows)]
    #[test]
    fn an_unasked_count_skips_a_cloud_only_folder_itself() {
        use windows::Win32::Storage::FileSystem::{FILE_ATTRIBUTE_OFFLINE, SetFileAttributesW};
        use windows::core::HSTRING;

        let dir = sample();
        let sub = dir.path().join("sub");
        assert_eq!(unasked_skip(&sub), None);
        // SAFETY: the path is an owned HSTRING that outlives the call.
        unsafe { SetFileAttributesW(&HSTRING::from(sub.as_os_str()), FILE_ATTRIBUTE_OFFLINE) }
            .unwrap();
        assert_eq!(unasked_skip(&sub), Some(Skipped::Cloud));
    }

    /// A folder marked offline holds files that are only in the cloud: measuring must not
    /// enter it.
    #[cfg(windows)]
    #[test]
    fn measure_does_not_enter_a_cloud_only_folder() {
        use windows::Win32::Storage::FileSystem::{FILE_ATTRIBUTE_OFFLINE, SetFileAttributesW};
        use windows::core::HSTRING;

        let dir = sample();
        let before = measure(dir.path(), &AtomicBool::new(false)).unwrap();
        let sub = dir.path().join("sub");
        // SAFETY: the path is an owned HSTRING that outlives the call.
        unsafe { SetFileAttributesW(&HSTRING::from(sub.as_os_str()), FILE_ATTRIBUTE_OFFLINE) }
            .unwrap();
        let after = measure(dir.path(), &AtomicBool::new(false)).unwrap();
        assert!(after.files < before.files, "{before:?} then {after:?}");
        assert!(after.bytes < before.bytes);
    }

    #[test]
    fn measure_stops_when_cancelled() {
        let dir = sample();
        assert_eq!(measure(dir.path(), &AtomicBool::new(true)), None);
    }
}
