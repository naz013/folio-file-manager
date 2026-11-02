//! The undo journal (ADR 0003): every mutating file operation is written here before it
//! runs, so it can be undone and redone, and so an operation that was cut short is known.
//!
//! An entry is `pending` while its operation runs and `done` once the steps it took are
//! recorded. Undoing makes it `undone` (redoable); finishing a new operation drops the
//! redoable entries. Entries still `pending` at startup were interrupted and can't be
//! undone, and undo stops at them rather than skip over what they may have changed.
//!
//! Undo never overwrites or removes anything it didn't create, and checks that the item
//! is still the one the operation touched (by file identity, and for a copy by size and
//! modified time) before reversing it.

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::ops;

/// Entries kept; older ones are forgotten.
const KEEP: i64 = 1000;

/// Entries visited when sealing a copied folder; past this the seal can't tell edits.
const SEAL_LIMIT: usize = 200_000;

/// What a copy looked like when it was made. Undoing a copy sends it to the trash, which
/// must not take edits made since with it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Seal {
    id: String,
    len: u64,
    /// The newest modified time of the item, or of anything in a folder (nanoseconds).
    newest: i64,
}

/// One thing an operation did, with what is needed to reverse it. `id` is the file
/// identity of the item afterwards, so an undo can tell it is still the same item.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "step", rename_all = "snake_case")]
pub enum Step {
    Rename {
        from: PathBuf,
        to: PathBuf,
        #[serde(default)]
        id: Option<String>,
    },
    CreateFolder {
        path: PathBuf,
        #[serde(default)]
        id: Option<String>,
    },
    CreateFile {
        path: PathBuf,
        #[serde(default)]
        id: Option<String>,
    },
    Move {
        from: PathBuf,
        to: PathBuf,
        #[serde(default)]
        id: Option<String>,
    },
    Copy {
        from: PathBuf,
        to: PathBuf,
        #[serde(default)]
        seal: Option<Seal>,
    },
    /// A file or folder made by a tool (a merged or compressed PDF, a zip or the folder it
    /// was unpacked into). Undo trashes it, if it is still exactly as made (for a folder
    /// of over `SEAL_LIMIT` entries nothing can be promised, so undo is refused). It can't
    /// be made again by redo.
    Made {
        path: PathBuf,
        #[serde(default)]
        seal: Option<Seal>,
        /// What it was made from, when it is a stand-in for it (a zip of folders, the folder
        /// a zip was unpacked into). Undo is refused once none of it is left: the result
        /// would be the only copy.
        #[serde(default)]
        from: Vec<PathBuf>,
    },
    /// `id` identifies the item in the Recycle Bin / Trash; `None` where it can't be
    /// found again (macOS, or a bin that wouldn't list it).
    Trash {
        original: PathBuf,
        id: Option<String>,
    },
}

impl Step {
    pub fn rename(from: PathBuf, to: PathBuf) -> Self {
        let id = ops::identity(&to);
        Self::Rename { from, to, id }
    }

    pub fn create_folder(path: PathBuf) -> Self {
        let id = ops::identity(&path);
        Self::CreateFolder { path, id }
    }

    pub fn create_file(path: PathBuf) -> Self {
        let id = ops::identity(&path);
        Self::CreateFile { path, id }
    }

    pub fn moved(from: PathBuf, to: PathBuf) -> Self {
        let id = ops::identity(&to);
        Self::Move { from, to, id }
    }

    pub fn made(path: PathBuf) -> Self {
        Self::made_from(path, Vec::new())
    }

    /// A file or folder made from `from`, which undo checks is still there.
    pub fn made_from(path: PathBuf, from: Vec<PathBuf>) -> Self {
        let seal = seal(&path);
        Self::Made { path, seal, from }
    }

    pub fn copied(from: PathBuf, to: PathBuf) -> Self {
        let seal = seal(&to);
        Self::Copy { from, to, seal }
    }
}

/// An entry that has been written and whose operation is about to run.
#[derive(Debug)]
pub struct Pending(i64);

pub struct Journal {
    conn: Mutex<Connection>,
    /// Where items went in the last undo or redo, as (from, to), until taken.
    relocations: Mutex<Vec<(PathBuf, PathBuf)>>,
}

impl Journal {
    /// Opens the journal in `file`, or in memory when there is none. Operations that were
    /// still running when the app last stopped are marked interrupted.
    pub fn open(file: Option<&Path>) -> rusqlite::Result<Self> {
        let conn = match file {
            Some(file) => {
                if let Some(dir) = file.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                Connection::open(file)?
            }
            None => Connection::open_in_memory()?,
        };
        conn.busy_timeout(Duration::from_secs(5))?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS journal (
                id    INTEGER PRIMARY KEY AUTOINCREMENT,
                at    INTEGER NOT NULL,
                label TEXT NOT NULL,
                steps TEXT NOT NULL DEFAULT '[]',
                state TEXT NOT NULL,
                ord   INTEGER
            );
            UPDATE journal SET state = 'interrupted' WHERE state = 'pending';",
        )?;
        // Journals written before entries had a place in the history.
        if conn.prepare("SELECT ord FROM journal LIMIT 0").is_err() {
            conn.execute_batch(
                "ALTER TABLE journal ADD COLUMN ord INTEGER; UPDATE journal SET ord = id;",
            )?;
        }
        Ok(Self {
            conn: Mutex::new(conn),
            relocations: Mutex::default(),
        })
    }

    /// The renames and moves the last undo or redo carried out, as (from, to). They are
    /// handed over once, for whatever keeps track of items by place (the tag index).
    pub fn take_relocations(&self) -> Vec<(PathBuf, PathBuf)> {
        std::mem::take(
            &mut *self
                .relocations
                .lock()
                .unwrap_or_else(PoisonError::into_inner),
        )
    }

    /// The connection. A panic in another thread must not stop every later operation.
    fn conn(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Writes the entry for an operation that is about to run. Call it before touching
    /// any file; an `Err` means the operation must not run.
    pub fn begin(&self, label: &str) -> io::Result<Pending> {
        let conn = self.conn();
        let written = (|| {
            conn.execute(
                "INSERT INTO journal (at, label, state) VALUES (?1, ?2, 'pending')",
                params![now(), label],
            )?;
            let id = conn.last_insert_rowid();
            conn.execute("UPDATE journal SET ord = id WHERE id = ?1", params![id])?;
            conn.execute("DELETE FROM journal WHERE id <= ?1", params![id - KEEP])?;
            Ok::<_, rusqlite::Error>(id)
        })();
        written
            .map(Pending)
            .map_err(|e| io::Error::other(format!("Can't write the undo journal: {e}")))
    }

    /// Records what the operation did. An operation that did nothing leaves no entry and
    /// keeps what could be redone; one that did something ends it.
    #[expect(
        clippy::needless_pass_by_value,
        reason = "taken by value so an entry can only be finished once"
    )]
    pub fn finish(&self, pending: Pending, steps: &[Step]) {
        let conn = self.conn();
        let result = match (steps.is_empty(), serde_json::to_string(steps)) {
            (true, _) => conn.execute("DELETE FROM journal WHERE id = ?1", params![pending.0]),
            (false, Ok(json)) => conn
                .execute(
                    "UPDATE journal SET steps = ?2, state = 'done' WHERE id = ?1",
                    params![pending.0, json],
                )
                .and_then(|n| {
                    conn.execute(
                        "DELETE FROM journal WHERE state IN ('undone', 'dropped')",
                        [],
                    )
                    .map(|_| n)
                }),
            // A path that isn't valid text can't be recorded, so it can't be undone.
            (false, Err(_)) => conn.execute(
                "UPDATE journal SET state = 'interrupted' WHERE id = ?1",
                params![pending.0],
            ),
        };
        if let Err(e) = result {
            eprintln!("writing the undo journal failed: {e}");
            // Not left pending, which would make undo wait for it for the rest of the
            // session: its steps are unknown, so it is a cut-short entry.
            let _ = conn.execute(
                "UPDATE journal SET state = 'interrupted' WHERE id = ?1",
                params![pending.0],
            );
        }
    }

    /// What Ctrl+Z would undo, and what Ctrl+Y would redo.
    pub fn next(&self) -> (Option<String>, Option<String>) {
        let conn = self.conn();
        (
            latest(&conn, "done", "DESC").map(|e| e.label),
            latest(&conn, "undone", "ASC").map(|e| e.label),
        )
    }

    /// The Recycle Bin ids already held by journal entries, so a new trash isn't matched
    /// to an item an earlier operation put there.
    pub fn trash_ids_in_use(&self) -> HashSet<String> {
        known_trash_ids(&self.conn())
    }

    /// Undoes the latest operation. Returns what was undone.
    pub fn undo(&self) -> Result<String, String> {
        self.replay(Direction::Undo)
    }

    /// Redoes the latest undone operation. Returns what was redone.
    pub fn redo(&self) -> Result<String, String> {
        self.replay(Direction::Redo)
    }

    fn replay(&self, direction: Direction) -> Result<String, String> {
        let conn = self.conn();
        let (from_state, to_state, order, verb) = match direction {
            Direction::Undo => ("done", "undone", "DESC", "undo"),
            Direction::Redo => ("undone", "done", "ASC", "redo"),
        };
        let running: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM journal WHERE state = 'pending'",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);
        if running > 0 {
            return Err(format!(
                "Wait for the current operation to finish, then {verb}"
            ));
        }
        let entry = latest(&conn, from_state, order);
        if matches!(direction, Direction::Undo) {
            // Older operations may depend on what a cut-short one changed.
            let cut_short = latest(&conn, "interrupted", "DESC");
            if let Some(stopped) = cut_short.filter(|s| entry.as_ref().is_none_or(|e| s.id > e.id))
            {
                save(&conn, stopped.id, "dropped", &[]);
                return Err(format!(
                    "\"{}\" didn't finish, so earlier operations can't be undone safely. \
                     Check its files, then undo again to go on.",
                    stopped.label
                ));
            }
        }
        let Some(entry) = entry else {
            return Err(format!("Nothing to {verb}"));
        };
        let steps: Vec<Step> =
            serde_json::from_str(&entry.steps).map_err(|e| format!("Can't {verb}: {e}"))?;
        if matches!(direction, Direction::Redo)
            && steps.iter().any(|s| matches!(s, Step::Made { .. }))
        {
            return Err(drop_unredoable(&conn, &entry));
        }
        let in_use = known_trash_ids(&conn);

        // Undo runs the steps backwards, redo forwards.
        let ordered: Vec<&Step> = match direction {
            Direction::Undo => steps.iter().rev().collect(),
            Direction::Redo => steps.iter().collect(),
        };
        // `done`: the steps that worked, as they now stand. `retry`: failures that may
        // go away. Failures that can't (the item is gone) are dropped.
        let mut done = Vec::new();
        let mut retry = Vec::new();
        let mut problems = Vec::new();
        for step in ordered {
            let result = match direction {
                Direction::Undo => undo_step(step).map(|()| step.clone()),
                Direction::Redo => redo_step(step, &in_use),
            };
            match result {
                Ok(step) => done.push(step),
                Err(e) => {
                    if !is_permanent(&e) {
                        retry.push(step.clone());
                    }
                    problems.push(e.to_string());
                }
            }
        }
        // Back to the order they were recorded in.
        if matches!(direction, Direction::Undo) {
            done.reverse();
            retry.reverse();
        }
        *self
            .relocations
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = relocations(&done, direction);
        if problems.is_empty() {
            save(&conn, entry.id, to_state, &done);
            return Ok(entry.label);
        }
        // What did work stays reversible as its own entry; what is left to do stays here.
        if !done.is_empty() {
            let state = match direction {
                Direction::Undo => "undone",
                Direction::Redo => "done",
            };
            insert(&conn, &entry, state, &done);
        }
        let forgotten = retry.is_empty();
        if forgotten {
            save(&conn, entry.id, "dropped", &[]);
        } else {
            save(&conn, entry.id, from_state, &retry);
        }
        let mut message = format!(
            "Couldn't {verb} \"{}\": {}",
            entry.label,
            problems.join("; ")
        );
        if forgotten {
            message.push_str(". It was removed from the history");
        }
        Err(message)
    }
}

/// Where `done` (steps in the order they were recorded) put things when replayed in
/// `direction`, as (from, to) in the order they happened.
fn relocations(done: &[Step], direction: Direction) -> Vec<(PathBuf, PathBuf)> {
    let moved = |step: &Step| match (step, direction) {
        (Step::Rename { from, to, .. } | Step::Move { from, to, .. }, Direction::Undo) => {
            Some((to.clone(), from.clone()))
        }
        (Step::Rename { from, to, .. } | Step::Move { from, to, .. }, Direction::Redo) => {
            Some((from.clone(), to.clone()))
        }
        _ => None,
    };
    let mut order: Vec<_> = done.iter().filter_map(moved).collect();
    if matches!(direction, Direction::Undo) {
        order.reverse();
    }
    order
}

#[derive(Clone, Copy)]
enum Direction {
    Undo,
    Redo,
}

struct Entry {
    id: i64,
    label: String,
    steps: String,
    /// Place in the history; a part split off an entry keeps the entry's place.
    ord: i64,
}

/// The newest (`DESC`) or oldest (`ASC`) entry in `state`.
fn latest(conn: &Connection, state: &str, order: &str) -> Option<Entry> {
    conn.query_row(
        &format!(
            "SELECT id, label, steps, COALESCE(ord, id) FROM journal WHERE state = ?1 \
             ORDER BY COALESCE(ord, id) {order}, id {order} LIMIT 1"
        ),
        params![state],
        |row| {
            Ok(Entry {
                id: row.get(0)?,
                label: row.get(1)?,
                steps: row.get(2)?,
                ord: row.get(3)?,
            })
        },
    )
    .optional()
    .ok()
    .flatten()
}

fn save(conn: &Connection, id: i64, state: &str, steps: &[Step]) {
    let Ok(json) = serde_json::to_string(steps) else {
        return;
    };
    if let Err(e) = conn.execute(
        "UPDATE journal SET state = ?2, steps = ?3 WHERE id = ?1",
        params![id, state, json],
    ) {
        eprintln!("writing the undo journal failed: {e}");
    }
}

/// Adds an entry for part of `parent`, in the same place in the history.
fn insert(conn: &Connection, parent: &Entry, state: &str, steps: &[Step]) {
    let Ok(json) = serde_json::to_string(steps) else {
        return;
    };
    if let Err(e) = conn.execute(
        "INSERT INTO journal (at, label, steps, state, ord) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![now(), parent.label, json, state, parent.ord],
    ) {
        eprintln!("writing the undo journal failed: {e}");
    }
}

fn known_trash_ids(conn: &Connection) -> HashSet<String> {
    let Ok(mut query) = conn.prepare("SELECT steps FROM journal WHERE state IN ('done', 'undone')")
    else {
        return HashSet::new();
    };
    let Ok(rows) = query.query_map([], |row| row.get::<_, String>(0)) else {
        return HashSet::new();
    };
    rows.flatten()
        .filter_map(|json| serde_json::from_str::<Vec<Step>>(&json).ok())
        .flatten()
        .filter_map(|step| match step {
            Step::Trash { id, .. } => id,
            _ => None,
        })
        .collect()
}

/// The current time as unix seconds, the clock the Recycle Bin uses for deletions.
pub fn unix_now() -> i64 {
    now()
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(0))
}

/// A failure that retrying can't fix: the item is gone, changed, or the system can't do
/// it. Those are given up on, so one doesn't block every older operation.
fn is_permanent(e: &io::Error) -> bool {
    matches!(e.kind(), ErrorKind::NotFound | ErrorKind::Unsupported)
}

/// A refusal because the item is no longer what the operation left. Permanent.
fn changed(message: String) -> io::Error {
    io::Error::new(ErrorKind::Unsupported, message)
}

fn name_of(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

fn gone(path: &Path) -> io::Error {
    io::Error::new(
        ErrorKind::NotFound,
        format!("\"{}\" isn't there any more", name_of(path)),
    )
}

fn in_the_way(path: &Path) -> io::Error {
    io::Error::new(
        ErrorKind::AlreadyExists,
        format!("something else is already at \"{}\"", path.display()),
    )
}

fn exists(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

/// Fails unless `path` is still the item with file identity `id`.
fn is_same_item(path: &Path, id: Option<&String>) -> io::Result<()> {
    match id {
        Some(id) if ops::identity(path).as_ref() != Some(id) => Err(changed(format!(
            "\"{}\" has been replaced by something else",
            name_of(path)
        ))),
        _ => Ok(()),
    }
}

/// What a copy looks like now. For a folder, the newest modified time of anything in it.
fn seal(path: &Path) -> Option<Seal> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    let nanos = |m: &std::fs::Metadata| {
        m.modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |d| i64::try_from(d.as_nanos()).unwrap_or(i64::MAX))
    };
    let mut newest = nanos(&meta);
    let mut len = meta.len();
    if meta.is_dir() {
        len = 0;
        let mut folders = vec![path.to_path_buf()];
        let mut seen = 0;
        while let Some(folder) = folders.pop() {
            let Ok(entries) = std::fs::read_dir(&folder) else {
                continue;
            };
            for entry in entries.flatten() {
                seen += 1;
                if seen > SEAL_LIMIT {
                    // Too much to look through: nothing can be promised about edits.
                    return None;
                }
                let Ok(meta) = entry.metadata() else { continue };
                newest = newest.max(nanos(&meta));
                if meta.is_dir() {
                    folders.push(entry.path());
                } else {
                    len += meta.len();
                }
            }
        }
    }
    Some(Seal {
        id: ops::identity(path)?,
        len,
        newest,
    })
}

/// Reverses one step. Refuses, with a message, rather than overwrite or guess.
fn undo_step(step: &Step) -> io::Result<()> {
    match step {
        Step::Rename { from, to, id } => {
            if !exists(to) {
                return Err(gone(to));
            }
            is_same_item(to, id.as_ref())?;
            let name = from.file_name().ok_or_else(|| gone(from))?;
            ops::rename(to, &name.to_string_lossy()).map(drop)
        }
        Step::Move { from, to, id } => {
            if !exists(to) {
                return Err(gone(to));
            }
            is_same_item(to, id.as_ref())?;
            if exists(from) {
                return Err(in_the_way(from));
            }
            if !from.parent().is_some_and(Path::is_dir) {
                return Err(io::Error::new(
                    ErrorKind::NotFound,
                    format!(
                        "the folder \"{}\" isn't there any more",
                        from.parent().map_or_else(String::new, name_of)
                    ),
                ));
            }
            ops::move_exact(to, from)?;
            ops::repoint_after_move(to, from);
            Ok(())
        }
        // A folder made by the operation goes to the trash, and only while it is empty.
        Step::CreateFolder { path, id } => {
            let mut entries = std::fs::read_dir(path).map_err(|_| gone(path))?;
            is_same_item(path, id.as_ref())?;
            if entries.next().is_some() {
                return Err(changed(format!(
                    "\"{}\" has files in it now",
                    name_of(path)
                )));
            }
            ops::trash(std::slice::from_ref(path)).map_err(|e| trash_error(&e))
        }
        // A file made by the operation goes to the trash, and only while it is empty.
        Step::CreateFile { path, id } => {
            let meta = std::fs::metadata(path).map_err(|_| gone(path))?;
            is_same_item(path, id.as_ref())?;
            if meta.len() > 0 {
                return Err(changed(format!("\"{}\" has content now", name_of(path))));
            }
            ops::trash(std::slice::from_ref(path)).map_err(|e| trash_error(&e))
        }
        // The copy goes to the trash, if it is still exactly what was copied; the
        // original is untouched.
        Step::Copy {
            from,
            to,
            seal: sealed,
        } => {
            if !exists(to) {
                return Err(gone(to));
            }
            // The trash may not exist on this drive, and then nothing could bring a
            // lone copy back.
            if !exists(from) {
                return Err(changed(format!(
                    "the original \"{}\" is gone, and this copy is all that is left of it",
                    name_of(from)
                )));
            }
            // Without a seal there is no telling whether the copy was edited.
            if sealed.is_none() || seal(to) != *sealed {
                return Err(changed(format!(
                    "Folio can't tell that \"{}\" is unchanged since it was copied",
                    name_of(to)
                )));
            }
            ops::trash(std::slice::from_ref(to)).map_err(|e| trash_error(&e))
        }
        Step::Made {
            path,
            seal: sealed,
            from,
        } => {
            if !exists(path) {
                return Err(gone(path));
            }
            // Without what it was made from, it is the only copy.
            if !from.is_empty() && !from.iter().any(|p| exists(p)) {
                return Err(changed(format!(
                    "What \"{}\" was made from is gone, so Folio won't remove the only copy",
                    name_of(path)
                )));
            }
            // Without a seal there is no telling whether the file was edited since.
            if sealed.is_none() || seal(path) != *sealed {
                return Err(changed(format!(
                    "Folio can't tell that \"{}\" is unchanged since it was made",
                    name_of(path)
                )));
            }
            ops::trash(std::slice::from_ref(path)).map_err(|e| trash_error(&e))
        }
        Step::Trash { original, id } => restore(original, id.as_deref()),
    }
}

/// Does one step again. Returns the step as it is now: a name may have been made
/// unique, and a trashed item has a new id. `in_use` are Recycle Bin ids already held.
fn redo_step(step: &Step, in_use: &HashSet<String>) -> io::Result<Step> {
    match step {
        Step::Rename { from, to, .. } => {
            if !exists(from) {
                return Err(gone(from));
            }
            let name = to.file_name().ok_or_else(|| gone(to))?;
            let to = ops::rename(from, &name.to_string_lossy())?;
            Ok(Step::rename(from.clone(), to))
        }
        Step::Move { from, to, .. } => {
            if !exists(from) {
                return Err(gone(from));
            }
            if exists(to) {
                return Err(in_the_way(to));
            }
            ops::move_exact(from, to)?;
            ops::repoint_after_move(from, to);
            Ok(Step::moved(from.clone(), to.clone()))
        }
        Step::CreateFolder { path, .. } => {
            let parent = path.parent().ok_or_else(|| gone(path))?;
            let path = ops::create_folder(parent, &name_of(path))?;
            Ok(Step::create_folder(path))
        }
        Step::CreateFile { path, .. } => {
            let parent = path.parent().ok_or_else(|| gone(path))?;
            let path = ops::create_file(parent, &name_of(path))?;
            Ok(Step::create_file(path))
        }
        Step::Copy { from, to, .. } => {
            if !exists(from) {
                return Err(gone(from));
            }
            let parent = to.parent().ok_or_else(|| gone(to))?;
            let copied = ops::copy_into(std::slice::from_ref(from), parent)?;
            let to = copied.paths.into_iter().next().ok_or_else(|| gone(from))?;
            Ok(Step::copied(from.clone(), to))
        }
        Step::Made { path, .. } => Err(changed(format!(
            "\"{}\" can't be made again; run the tool again",
            name_of(path)
        ))),
        Step::Trash { original, .. } => {
            if !exists(original) {
                return Err(gone(original));
            }
            ops::trash(std::slice::from_ref(original)).map_err(|e| trash_error(&e))?;
            let id = trash_ids(std::slice::from_ref(original), now() - 5, in_use)
                .into_iter()
                .next()
                .flatten();
            Ok(Step::Trash {
                original: original.clone(),
                id,
            })
        }
    }
}

/// A file a tool made can't be made again, and redoing the steps before it (a Recycle Bin
/// move of the original, say) would leave nothing in its place: the entry is dropped whole.
fn drop_unredoable(conn: &Connection, entry: &Entry) -> String {
    save(conn, entry.id, "dropped", &[]);
    format!("\"{}\" can't be redone; run the tool again", entry.label)
}

fn trash_error(e: &trash::Error) -> io::Error {
    io::Error::other(format!("the system couldn't move it to the trash ({e})"))
}

/// `path` as the Recycle Bin / Trash records it: its folder resolved, then compared
/// without regard to a `\\?\` prefix or, on Windows, case.
#[cfg(any(
    target_os = "windows",
    all(
        unix,
        not(target_os = "macos"),
        not(target_os = "ios"),
        not(target_os = "android")
    )
))]
fn as_recorded(path: &Path) -> PathBuf {
    let resolved = match (path.parent(), path.file_name()) {
        (Some(parent), Some(name)) => std::fs::canonicalize(parent)
            .map_or_else(|_| path.to_path_buf(), |real| real.join(name)),
        _ => path.to_path_buf(),
    };
    ops::comparable(&resolved).unwrap_or(resolved)
}

/// The Recycle Bin / Trash ids of items that were just trashed from `originals`, in the
/// same order. Each is the newest entry for its original path deleted since `since`
/// (unix seconds) that is not in `exclude`; `None` where there is none, or where the
/// platform can't restore.
#[cfg(any(
    target_os = "windows",
    all(
        unix,
        not(target_os = "macos"),
        not(target_os = "ios"),
        not(target_os = "android")
    )
))]
pub fn trash_ids(
    originals: &[PathBuf],
    since: i64,
    exclude: &HashSet<String>,
) -> Vec<Option<String>> {
    let Ok(items) = trash::os_limited::list() else {
        return vec![None; originals.len()];
    };
    let mut taken = exclude.clone();
    originals
        .iter()
        .map(|original| {
            let wanted = as_recorded(original);
            let id = items
                .iter()
                .filter(|i| {
                    i.time_deleted >= since
                        && !taken.contains(&*i.id.to_string_lossy())
                        && ops::comparable(&i.original_path()).is_ok_and(|p| p == wanted)
                })
                .max_by_key(|i| i.time_deleted)
                .map(|i| i.id.to_string_lossy().into_owned());
            taken.extend(id.clone());
            id
        })
        .collect()
}

#[cfg(not(any(
    target_os = "windows",
    all(
        unix,
        not(target_os = "macos"),
        not(target_os = "ios"),
        not(target_os = "android")
    )
)))]
pub fn trash_ids(
    originals: &[PathBuf],
    _since: i64,
    _exclude: &HashSet<String>,
) -> Vec<Option<String>> {
    vec![None; originals.len()]
}

/// Puts the item with Recycle Bin / Trash id `id` back where `original` was.
#[cfg(any(
    target_os = "windows",
    all(
        unix,
        not(target_os = "macos"),
        not(target_os = "ios"),
        not(target_os = "android")
    )
))]
fn restore(original: &Path, id: Option<&str>) -> io::Result<()> {
    let id = id.ok_or_else(|| {
        io::Error::new(
            ErrorKind::Unsupported,
            format!(
                "\"{}\" can't be found in the Recycle Bin",
                name_of(original)
            ),
        )
    })?;
    // Listing a big bin takes a while, so it comes before the check that the place is
    // free: the check must be as close to the restore as it can be.
    let items = trash::os_limited::list().map_err(io::Error::other)?;
    let item = items
        .into_iter()
        .find(|i| i.id.to_string_lossy() == id)
        .ok_or_else(|| {
            io::Error::new(
                ErrorKind::NotFound,
                format!("\"{}\" is no longer in the Recycle Bin", name_of(original)),
            )
        })?;
    if exists(original) {
        return Err(in_the_way(original));
    }
    trash::os_limited::restore_all([item]).map_err(io::Error::other)?;
    // The system can report success for a restore it was told to abort.
    if exists(original) {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "\"{}\" is still in the Recycle Bin",
            name_of(original)
        )))
    }
}

#[cfg(not(any(
    target_os = "windows",
    all(
        unix,
        not(target_os = "macos"),
        not(target_os = "ios"),
        not(target_os = "android")
    )
)))]
fn restore(original: &Path, _id: Option<&str>) -> io::Result<()> {
    Err(io::Error::new(
        ErrorKind::Unsupported,
        format!(
            "\"{}\" can't be restored from the Trash by Folio on this system",
            name_of(original)
        ),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn journal() -> Journal {
        Journal::open(None).unwrap()
    }

    fn done(j: &Journal, label: &str, steps: &[Step]) {
        let pending = j.begin(label).unwrap();
        j.finish(pending, steps);
    }

    fn rename_step(from: &Path, to: &Path) -> Step {
        Step::rename(from.to_path_buf(), to.to_path_buf())
    }

    fn tmp() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn nothing_to_undo_or_redo_at_first() {
        let j = journal();
        assert_eq!(j.undo().unwrap_err(), "Nothing to undo");
        assert_eq!(j.redo().unwrap_err(), "Nothing to redo");
        assert_eq!(j.next(), (None, None));
    }

    #[test]
    fn a_rename_is_undone_and_redone() {
        let t = tmp();
        let (a, b) = (t.path().join("a.txt"), t.path().join("b.txt"));
        std::fs::write(&b, "x").unwrap();
        let j = journal();
        done(&j, "Rename \"a.txt\"", &[rename_step(&a, &b)]);
        assert_eq!(j.next().0.as_deref(), Some("Rename \"a.txt\""));

        assert_eq!(j.undo().unwrap(), "Rename \"a.txt\"");
        assert!(a.exists() && !b.exists());
        assert_eq!(j.next(), (None, Some("Rename \"a.txt\"".into())));

        j.redo().unwrap();
        assert!(b.exists() && !a.exists());
    }

    #[test]
    fn a_move_is_undone_to_its_exact_place_and_refuses_to_overwrite() {
        let t = tmp();
        let (from, to) = (t.path().join("a.txt"), t.path().join("dest").join("a.txt"));
        std::fs::create_dir(t.path().join("dest")).unwrap();
        std::fs::write(&to, "moved").unwrap();
        let j = journal();
        done(
            &j,
            "Move \"a.txt\"",
            &[Step::moved(from.clone(), to.clone())],
        );
        // Something took the original place meanwhile.
        std::fs::write(&from, "newcomer").unwrap();

        let e = j.undo().unwrap_err();
        assert!(e.contains("already at"), "{e}");
        assert_eq!(std::fs::read_to_string(&from).unwrap(), "newcomer");
        assert_eq!(std::fs::read_to_string(&to).unwrap(), "moved");

        // Retrying after the way is clear works, and the entry was kept for it.
        std::fs::remove_file(&from).unwrap();
        j.undo().unwrap();
        assert_eq!(std::fs::read_to_string(&from).unwrap(), "moved");
    }

    #[test]
    fn a_missing_item_gives_a_clear_message_and_is_forgotten() {
        let t = tmp();
        let j = journal();
        done(
            &j,
            "Move \"a.txt\"",
            &[Step::moved(
                t.path().join("a.txt"),
                t.path().join("gone.txt"),
            )],
        );
        let e = j.undo().unwrap_err();
        assert!(e.contains("\"gone.txt\" isn't there any more"), "{e}");
        assert!(e.contains("removed from the history"), "{e}");
        assert_eq!(j.next(), (None, None));
    }

    #[test]
    fn an_entry_that_cannot_be_undone_does_not_block_older_ones() {
        let t = tmp();
        let (a, b) = (t.path().join("a"), t.path().join("b"));
        std::fs::write(&b, "x").unwrap();
        let j = journal();
        done(&j, "older", &[rename_step(&a, &b)]);
        done(
            &j,
            "newer",
            &[Step::moved(t.path().join("x"), t.path().join("gone"))],
        );

        assert!(j.undo().is_err());
        assert_eq!(j.undo().unwrap(), "older");
        assert!(a.exists());
    }

    #[test]
    fn a_missing_original_folder_says_which() {
        let t = tmp();
        let to = t.path().join("a.txt");
        std::fs::write(&to, "x").unwrap();
        let j = journal();
        done(
            &j,
            "Move",
            &[Step::moved(t.path().join("old folder").join("a.txt"), to)],
        );
        let e = j.undo().unwrap_err();
        assert!(
            e.contains("the folder \"old folder\" isn't there any more"),
            "{e}"
        );
    }

    #[test]
    fn a_new_operation_drops_what_could_be_redone() {
        let t = tmp();
        let (a, b) = (t.path().join("a"), t.path().join("b"));
        std::fs::write(&b, "x").unwrap();
        let j = journal();
        done(&j, "first", &[rename_step(&a, &b)]);
        j.undo().unwrap();
        assert!(j.next().1.is_some());

        done(&j, "second", &[rename_step(&b, &a)]);

        assert_eq!(j.next(), (Some("second".into()), None));
    }

    #[test]
    fn an_operation_that_did_nothing_keeps_what_could_be_redone() {
        let t = tmp();
        let (a, b) = (t.path().join("a"), t.path().join("b"));
        std::fs::write(&b, "x").unwrap();
        let j = journal();
        done(&j, "first", &[rename_step(&a, &b)]);
        j.undo().unwrap();

        done(&j, "failed rename", &[]);

        assert_eq!(j.next().1.as_deref(), Some("first"));
    }

    #[test]
    fn undo_goes_back_one_operation_at_a_time_and_redo_forward() {
        let t = tmp();
        let p = |n: &str| t.path().join(n);
        std::fs::write(p("c"), "x").unwrap();
        let j = journal();
        done(&j, "a to b", &[rename_step(&p("a"), &p("b"))]);
        done(&j, "b to c", &[rename_step(&p("b"), &p("c"))]);

        assert_eq!(j.undo().unwrap(), "b to c");
        assert_eq!(j.undo().unwrap(), "a to b");
        assert!(p("a").exists());
        assert_eq!(j.redo().unwrap(), "a to b");
        assert_eq!(j.redo().unwrap(), "b to c");
        assert!(p("c").exists());
    }

    #[test]
    fn a_partly_redone_operation_keeps_what_was_redone_undoable() {
        let t = tmp();
        let p = |n: &str| t.path().join(n);
        // a and b were moved into x; c was too, but it never made it there.
        std::fs::create_dir(p("x")).unwrap();
        for n in ["a", "b"] {
            std::fs::write(p(n), n).unwrap();
            std::fs::rename(p(n), p("x").join(n)).unwrap();
        }
        let j = journal();
        let steps: Vec<Step> = ["a", "b", "c"]
            .iter()
            .map(|n| Step::moved(p(n), p("x").join(n)))
            .collect();
        done(&j, "Move 3 items", &steps);
        // Undo moves a and b back and reports c, which is not there.
        assert!(j.undo().is_err());
        assert!(p("a").exists() && p("b").exists());
        // What was undone can be redone: a and b go into x again.
        assert_eq!(j.redo().unwrap(), "Move 3 items");
        assert!(p("x").join("a").exists() && p("x").join("b").exists());

        // And that redo is undoable.
        assert_eq!(j.undo().unwrap(), "Move 3 items");
        assert!(p("a").exists() && p("b").exists());
    }

    #[test]
    fn an_operation_cut_short_cannot_be_undone_after_restart() {
        let dir = tmp();
        let file = dir.path().join("journal.sqlite");
        {
            let j = Journal::open(Some(&file)).unwrap();
            // Begun, never finished: the app stopped in the middle.
            let _ = j.begin("Move 3 items").unwrap();
        }
        let j = Journal::open(Some(&file)).unwrap();
        assert_eq!(j.next(), (None, None));
        let e = j.undo().unwrap_err();
        assert!(e.contains("\"Move 3 items\" didn't finish"), "{e}");
    }

    #[test]
    fn undo_stops_at_an_interrupted_entry_then_goes_on() {
        let t = tmp();
        let (a, b) = (t.path().join("a"), t.path().join("b"));
        std::fs::write(&b, "x").unwrap();
        let file = t.path().join("journal.sqlite");
        {
            let j = Journal::open(Some(&file)).unwrap();
            done(&j, "older", &[rename_step(&a, &b)]);
            let _ = j.begin("cut short").unwrap();
        }
        let j = Journal::open(Some(&file)).unwrap();

        assert!(j.undo().unwrap_err().contains("didn't finish"));
        assert!(b.exists(), "nothing older was touched");
        assert_eq!(j.undo().unwrap(), "older");
    }

    #[test]
    fn undo_refuses_while_an_operation_is_running() {
        let t = tmp();
        let (a, b) = (t.path().join("a"), t.path().join("b"));
        std::fs::write(&b, "x").unwrap();
        let j = journal();
        done(&j, "first", &[rename_step(&a, &b)]);
        let running = j.begin("a long copy").unwrap();

        let why = j.undo().unwrap_err();
        assert!(why.contains("Wait for the current operation"), "{why}");
        assert!(b.exists());

        j.finish(running, &[]);
        assert!(j.undo().is_ok());
    }

    #[test]
    fn the_journal_survives_a_restart() {
        let t = tmp();
        let (a, b) = (t.path().join("a"), t.path().join("b"));
        std::fs::write(&b, "x").unwrap();
        let file = t.path().join("journal.sqlite");
        {
            let j = Journal::open(Some(&file)).unwrap();
            done(&j, "Rename \"a\"", &[rename_step(&a, &b)]);
        }
        let j = Journal::open(Some(&file)).unwrap();
        assert_eq!(j.undo().unwrap(), "Rename \"a\"");
        assert!(a.exists());
    }

    #[test]
    fn only_the_last_entries_are_kept() {
        let j = journal();
        for i in 0..(KEEP + 20) {
            done(
                &j,
                &format!("op {i}"),
                &[Step::CreateFolder {
                    path: "x".into(),
                    id: None,
                }],
            );
        }
        let rows: i64 = j
            .conn()
            .query_row("SELECT COUNT(*) FROM journal", [], |r| r.get(0))
            .unwrap();
        assert!(rows <= KEEP + 1, "{rows}");
    }

    #[test]
    fn a_file_with_content_is_not_taken_back() {
        let t = tmp();
        let file = t.path().join("New file.txt");
        std::fs::write(&file, "").unwrap();
        let j = journal();
        done(&j, "New file", &[Step::create_file(file.clone())]);
        std::fs::write(&file, "I typed something").unwrap();

        let e = j.undo().unwrap_err();

        assert!(e.contains("has content now"), "{e}");
        assert!(file.exists());
    }

    #[test]
    fn a_new_empty_file_can_be_taken_back_and_made_again() {
        let t = tmp();
        let file = ops::create_file(t.path(), "New file.txt").unwrap();
        let j = journal();
        done(&j, "New file", &[Step::create_file(file.clone())]);

        j.undo().unwrap();
        assert!(!file.exists());
        j.redo().unwrap();
        assert!(file.exists());
    }

    #[test]
    fn a_folder_with_files_in_it_is_not_taken_back() {
        let t = tmp();
        let folder = t.path().join("New folder");
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(folder.join("mine.txt"), "x").unwrap();
        let j = journal();
        done(&j, "New folder", &[Step::create_folder(folder.clone())]);

        let e = j.undo().unwrap_err();

        assert!(e.contains("has files in it now"), "{e}");
        assert!(folder.join("mine.txt").exists());
    }

    #[test]
    fn a_folder_replaced_by_another_is_not_taken_back() {
        let t = tmp();
        let folder = t.path().join("New folder");
        std::fs::create_dir(&folder).unwrap();
        let j = journal();
        done(&j, "New folder", &[Step::create_folder(folder.clone())]);
        // Another empty folder takes the name.
        std::fs::remove_dir(&folder).unwrap();
        std::fs::create_dir(&folder).unwrap();

        let e = j.undo().unwrap_err();

        assert!(e.contains("replaced by something else"), "{e}");
        assert!(folder.is_dir());
    }

    #[test]
    fn a_copy_that_was_edited_is_not_taken_back() {
        let t = tmp();
        let (from, to) = (t.path().join("a.txt"), t.path().join("copy.txt"));
        std::fs::write(&from, "original").unwrap();
        std::fs::write(&to, "original").unwrap();
        let j = journal();
        done(&j, "Copy \"a.txt\"", &[Step::copied(from, to.clone())]);
        // Edited after it was copied.
        std::fs::write(&to, "edited, and longer").unwrap();

        let e = j.undo().unwrap_err();

        assert!(e.contains("can't tell that"), "{e}");
        assert_eq!(std::fs::read_to_string(&to).unwrap(), "edited, and longer");
    }

    #[test]
    fn a_copy_replaced_by_another_item_is_not_taken_back() {
        let t = tmp();
        let (from, to) = (t.path().join("a.txt"), t.path().join("copy.txt"));
        std::fs::write(&from, "same").unwrap();
        std::fs::write(&to, "same").unwrap();
        let j = journal();
        done(&j, "Copy", &[Step::copied(from, to.clone())]);
        std::fs::remove_file(&to).unwrap();
        std::fs::write(&to, "same").unwrap();

        let e = j.undo().unwrap_err();

        assert!(e.contains("can't tell that"), "{e}");
        assert!(to.exists());
    }

    #[test]
    fn a_partly_undone_operation_is_redone_in_its_place() {
        let t = tmp();
        let p = |n: &str| t.path().join(n);
        std::fs::create_dir(p("x")).unwrap();
        std::fs::write(p("x").join("a.txt"), "a").unwrap();
        std::fs::write(p("x").join("c.txt"), "c").unwrap();
        let j = journal();
        // First: a.txt and c.txt moved into x (c.txt's origin is blocked below).
        done(
            &j,
            "move in",
            &[
                Step::moved(p("a.txt"), p("x").join("a.txt")),
                Step::moved(p("gone.txt"), p("x").join("gone.txt")),
            ],
        );
        // Then: a.txt renamed inside x.
        std::fs::rename(p("x").join("a.txt"), p("x").join("b.txt")).unwrap();
        done(
            &j,
            "rename",
            &[Step::rename(p("x").join("a.txt"), p("x").join("b.txt"))],
        );

        assert_eq!(j.undo().unwrap(), "rename");
        assert!(j.undo().is_err(), "gone.txt can't be moved back");
        assert!(p("a.txt").exists());

        // Redo goes in the order things happened: the move, then the rename.
        assert_eq!(j.redo().unwrap(), "move in");
        assert_eq!(j.redo().unwrap(), "rename");
        assert!(p("x").join("b.txt").exists());
    }

    #[test]
    fn a_copy_without_a_seal_is_not_taken_back() {
        let t = tmp();
        let (from, to) = (t.path().join("a.txt"), t.path().join("copy.txt"));
        std::fs::write(&from, "x").unwrap();
        std::fs::write(&to, "x").unwrap();
        let j = journal();
        done(
            &j,
            "Copy",
            &[Step::Copy {
                from,
                to: to.clone(),
                seal: None,
            }],
        );

        let why = j.undo().unwrap_err();

        assert!(why.contains("can't tell that"), "{why}");
        assert!(to.exists());
    }

    #[test]
    fn a_copy_is_not_taken_back_when_the_original_is_gone() {
        let t = tmp();
        let (from, to) = (t.path().join("a.txt"), t.path().join("copy.txt"));
        std::fs::write(&from, "x").unwrap();
        std::fs::write(&to, "x").unwrap();
        let j = journal();
        done(&j, "Copy", &[Step::copied(from.clone(), to.clone())]);
        std::fs::remove_file(&from).unwrap();

        let why = j.undo().unwrap_err();

        assert!(why.contains("is all that is left of it"), "{why}");
        assert!(to.exists());
    }

    #[test]
    fn an_edited_copy_does_not_block_older_undos() {
        let t = tmp();
        let (a, b) = (t.path().join("a"), t.path().join("b"));
        std::fs::write(&b, "x").unwrap();
        let (from, to) = (t.path().join("src.txt"), t.path().join("copy.txt"));
        std::fs::write(&from, "orig").unwrap();
        std::fs::write(&to, "orig").unwrap();
        let j = journal();
        done(&j, "older", &[rename_step(&a, &b)]);
        done(&j, "Copy", &[Step::copied(from, to.clone())]);
        std::fs::write(&to, "edited, much longer").unwrap();

        assert!(j.undo().is_err());
        assert!(to.exists(), "the edited copy stays");
        assert_eq!(j.undo().unwrap(), "older");
    }

    #[test]
    fn a_made_file_with_content_is_undone_only_while_it_is_unchanged() {
        let t = tmp();
        let (a, b) = (t.path().join("a"), t.path().join("b"));
        std::fs::write(&b, "x").unwrap();
        let made = t.path().join("Merged.pdf");
        std::fs::write(&made, "%PDF content").unwrap();
        let j = journal();
        done(&j, "older", &[rename_step(&a, &b)]);
        done(&j, "Merge", &[Step::made(made.clone())]);
        // Edited since: refused, and the file stays.
        std::fs::write(&made, "%PDF content, edited later").unwrap();
        assert!(j.undo().is_err());
        assert!(made.exists());
    }

    #[test]
    fn a_made_file_is_not_undone_once_what_it_was_made_from_is_gone() {
        let t = tmp();
        let (a, b) = (t.path().join("a"), t.path().join("b"));
        std::fs::write(&b, "x").unwrap();
        let source = t.path().join("docs");
        std::fs::create_dir(&source).unwrap();
        let made = t.path().join("docs.zip");
        std::fs::write(&made, "zip content").unwrap();
        let j = journal();
        done(&j, "older", &[rename_step(&a, &b)]);
        done(
            &j,
            "Compress",
            &[Step::made_from(made.clone(), vec![source.clone()])],
        );
        // The folder was deleted outside Folio: the zip is all that is left of it.
        std::fs::remove_dir(&source).unwrap();
        let error = j.undo().unwrap_err();
        assert!(error.contains("only copy"), "{error}");
        assert!(made.exists());
    }

    #[test]
    fn an_entry_saved_before_made_steps_knew_their_sources_still_loads() {
        let old = r#"{"step":"made","path":"C:/x/Merged.pdf"}"#;
        let step: Step = serde_json::from_str(old).unwrap();
        assert!(matches!(step, Step::Made { from, .. } if from.is_empty()));
    }

    #[test]
    fn redoing_an_entry_with_a_made_file_touches_nothing() {
        let t = tmp();
        let original = t.path().join("photo.jpg");
        std::fs::write(&original, "original").unwrap();
        let made = t.path().join("edit.jpg");
        std::fs::write(&made, "edited").unwrap();
        let j = journal();
        // As a replace is recorded: the original to the bin, then the edit in its place.
        let pending = j.begin("Rotate").unwrap();
        let steps = [
            Step::Trash {
                original: original.clone(),
                id: Some("bin-1".into()),
            },
            Step::made(made.clone()),
        ];
        j.finish(pending, &steps);
        {
            let conn = j.conn();
            conn.execute("UPDATE journal SET state = 'undone'", [])
                .unwrap();
        }
        assert!(j.redo().is_err());
        assert!(original.exists(), "the original is not trashed by a redo");
        assert!(made.exists());
    }

    #[test]
    fn redoing_a_made_file_never_creates_an_empty_one() {
        let t = tmp();
        let made = t.path().join("Merged.pdf");
        let step = Step::made(made.clone());
        assert!(redo_step(&step, &HashSet::new()).is_err());
        assert!(!made.exists());
    }

    #[test]
    fn recorded_trash_ids_are_not_given_to_another_trash() {
        let j = journal();
        done(
            &j,
            "Trash",
            &[Step::Trash {
                original: "C:\\a".into(),
                id: Some("bin-1".into()),
            }],
        );
        assert!(j.trash_ids_in_use().contains("bin-1"));
    }

    /// Trashes a real file and brings it back through the real Recycle Bin / Trash, so it
    /// is run by hand: `cargo test journal::tests::trash_round_trip -- --ignored`.
    #[test]
    #[ignore = "uses the real Recycle Bin"]
    fn trash_round_trip() {
        let t = tmp();
        let file = t.path().join("folio-journal-test.txt");
        std::fs::write(&file, "restore me").unwrap();
        let before = now() - 2;
        ops::trash(std::slice::from_ref(&file)).unwrap();
        let ids = trash_ids(std::slice::from_ref(&file), before, &HashSet::new());
        let j = journal();
        done(
            &j,
            "Move to Recycle Bin",
            &[Step::Trash {
                original: file.clone(),
                id: ids[0].clone(),
            }],
        );

        j.undo().unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "restore me");

        j.redo().unwrap();
        assert!(!file.exists());
        // Leave the Recycle Bin as it was.
        j.undo().unwrap();
        assert!(file.exists());
    }
}
