//! The tag index (ADR 0002): tags and which files carry them, in SQLite, keyed by file
//! identity (volume and file id) so a tag survives a rename or move on its drive. Each
//! file's last known place is kept too, so a folder can ask which of its items are tagged
//! without looking up every one.

use crate::fs::{self as listing, Entry};
use crate::ops;
use rusqlite::{Connection, params};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

/// The seven colour tags every index starts with, in the order of keys 1 to 7.
const COLORS: [(&str, &str); 7] = [
    ("Red", "red"),
    ("Orange", "orange"),
    ("Yellow", "yellow"),
    ("Green", "green"),
    ("Blue", "blue"),
    ("Purple", "purple"),
    ("Gray", "gray"),
];

/// Ids 1 to 7 are the colour tags; named tags come after.
const COLOR_TAGS: i64 = 7;
const MAX_NAME: usize = 40;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Tag {
    pub id: i64,
    pub name: String,
    /// One of the seven colour names, or `None` for a plain named tag.
    pub color: Option<String>,
}

/// What `toggle` did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Toggled {
    /// Whether the items that could be tagged have the tag now.
    pub on: bool,
    /// Items left out because their identity can't be read (some network shares, files
    /// that vanished).
    pub skipped: usize,
}

pub struct Tags {
    conn: Mutex<Connection>,
}

/// `path` as text that two spellings of one path share: how folders and files are matched.
fn key(path: &Path) -> String {
    ops::comparable(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

impl Tags {
    /// Opens the index at `file`, or an in-memory one.
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
            "PRAGMA foreign_keys = ON;
             CREATE TABLE IF NOT EXISTS tags (
                 id    INTEGER PRIMARY KEY,
                 name  TEXT NOT NULL UNIQUE COLLATE NOCASE,
                 color TEXT
             );
             CREATE TABLE IF NOT EXISTS files (
                 identity TEXT PRIMARY KEY,
                 path     TEXT NOT NULL,
                 norm     TEXT NOT NULL,
                 dir      TEXT NOT NULL
             );
             CREATE INDEX IF NOT EXISTS files_dir ON files (dir);
             CREATE INDEX IF NOT EXISTS files_norm ON files (norm);
             CREATE TABLE IF NOT EXISTS file_tags (
                 identity TEXT NOT NULL REFERENCES files(identity) ON DELETE CASCADE,
                 tag_id   INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
                 PRIMARY KEY (identity, tag_id)
             );",
        )?;
        for (id, (name, color)) in (1i64..).zip(COLORS) {
            conn.execute(
                "INSERT OR IGNORE INTO tags (id, name, color) VALUES (?1, ?2, ?3)",
                params![id, name, color],
            )?;
        }
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn conn(&self) -> MutexGuard<'_, Connection> {
        self.conn
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Every tag: the colour tags first, then the named ones by name.
    pub fn list(&self) -> Vec<Tag> {
        let conn = self.conn();
        let Ok(mut stmt) = conn.prepare(
            "SELECT id, name, color FROM tags
             ORDER BY CASE WHEN id <= 7 THEN 0 ELSE 1 END,
                      CASE WHEN id <= 7 THEN id ELSE 0 END, name",
        ) else {
            return Vec::new();
        };
        stmt.query_map([], |r| {
            Ok(Tag {
                id: r.get(0)?,
                name: r.get(1)?,
                color: r.get(2)?,
            })
        })
        .map(|rows| rows.filter_map(Result::ok).collect())
        .unwrap_or_default()
    }

    /// Adds a named tag, optionally with one of the seven colours.
    pub fn create(&self, name: &str, color: Option<&str>) -> Result<Tag, String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("A tag needs a name".into());
        }
        if name.chars().count() > MAX_NAME {
            return Err(format!("A tag name can have at most {MAX_NAME} characters"));
        }
        if let Some(c) = color
            && !COLORS.iter().any(|(_, known)| *known == c)
        {
            return Err(format!("\"{c}\" is not one of the tag colours"));
        }
        let conn = self.conn();
        let made = conn.execute(
            "INSERT INTO tags (name, color) VALUES (?1, ?2)",
            params![name, color],
        );
        match made {
            // A named tag's id is always past the colour tags.
            Ok(_) => Ok(Tag {
                id: conn.last_insert_rowid(),
                name: name.to_owned(),
                color: color.map(str::to_owned),
            }),
            Err(rusqlite::Error::SqliteFailure(e, _))
                if e.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                Err(format!("There is already a tag named \"{name}\""))
            }
            Err(e) => Err(e.to_string()),
        }
    }

    /// Removes a named tag from every file. The seven colour tags stay.
    pub fn delete(&self, id: i64) -> Result<(), String> {
        if id <= COLOR_TAGS {
            return Err("The colour tags can't be removed".into());
        }
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        tx.execute("DELETE FROM tags WHERE id = ?1", params![id])
            .map_err(|e| e.to_string())?;
        // Files left with no tag need no row.
        tx.execute(
            "DELETE FROM files WHERE identity NOT IN (SELECT identity FROM file_tags)",
            [],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())
    }

    /// Gives `tag` to every one of `paths`, or takes it from them if they all have it.
    /// Returns whether they have it now.
    pub fn toggle(&self, paths: &[PathBuf], tag: i64) -> Result<Toggled, String> {
        let items: Vec<(String, &PathBuf)> = paths
            .iter()
            .filter_map(|p| ops::identity(p).map(|id| (id, p)))
            .collect();
        if items.is_empty() {
            return Err("These items can't be tagged".into());
        }
        let skipped = paths.len() - items.len();
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        let exists: bool = tx
            .query_row("SELECT 1 FROM tags WHERE id = ?1", params![tag], |_| {
                Ok(true)
            })
            .unwrap_or(false);
        if !exists {
            return Err("That tag no longer exists".into());
        }
        let has = |identity: &str| -> bool {
            tx.query_row(
                "SELECT 1 FROM file_tags WHERE identity = ?1 AND tag_id = ?2",
                params![identity, tag],
                |_| Ok(true),
            )
            .unwrap_or(false)
        };
        let all_have = items.iter().all(|(id, _)| has(id));
        for (identity, path) in &items {
            if all_have {
                tx.execute(
                    "DELETE FROM file_tags WHERE identity = ?1 AND tag_id = ?2",
                    params![identity, tag],
                )
                .map_err(|e| e.to_string())?;
                tx.execute(
                    "DELETE FROM files WHERE identity = ?1
                     AND NOT EXISTS (SELECT 1 FROM file_tags WHERE identity = ?1)",
                    params![identity],
                )
                .map_err(|e| e.to_string())?;
            } else {
                let dir = path.parent().map(key).unwrap_or_default();
                tx.execute(
                    "INSERT INTO files (identity, path, norm, dir) VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT(identity) DO UPDATE SET path = ?2, norm = ?3, dir = ?4",
                    params![identity, path.to_string_lossy(), key(path), dir],
                )
                .map_err(|e| e.to_string())?;
                tx.execute(
                    "INSERT OR IGNORE INTO file_tags (identity, tag_id) VALUES (?1, ?2)",
                    params![identity, tag],
                )
                .map_err(|e| e.to_string())?;
            }
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok(Toggled {
            on: !all_have,
            skipped,
        })
    }

    /// The tagged items directly in `dir`, as (name, tag ids). An entry counts only if the
    /// file at its place is still the one that was tagged.
    pub fn in_dir(&self, dir: &Path) -> Vec<(String, Vec<i64>)> {
        let rows: Vec<(String, String)> = {
            let conn = self.conn();
            let Ok(mut stmt) = conn.prepare("SELECT identity, path FROM files WHERE dir = ?1")
            else {
                return Vec::new();
            };
            stmt.query_map(params![key(dir)], |r| Ok((r.get(0)?, r.get(1)?)))
                .map(|rows| rows.filter_map(Result::ok).collect())
                .unwrap_or_default()
        };
        let mut out: BTreeMap<String, Vec<i64>> = BTreeMap::new();
        for (identity, path) in rows {
            let path = PathBuf::from(path);
            if ops::identity(&path).as_deref() != Some(&identity) {
                continue;
            }
            let ids = self.tag_ids(&identity);
            if let (Some(name), false) = (path.file_name(), ids.is_empty()) {
                out.insert(name.to_string_lossy().into_owned(), ids);
            }
        }
        out.into_iter().collect()
    }

    /// Every item that carries `tag`, wherever it is, with its entry and all its tags. Items
    /// that are gone, or replaced by another file, are left out.
    pub fn files_with(&self, tag: i64) -> Vec<(PathBuf, Entry, Vec<i64>)> {
        let rows: Vec<(String, String)> = {
            let conn = self.conn();
            let Ok(mut stmt) = conn.prepare(
                "SELECT f.identity, f.path FROM files f
                 JOIN file_tags t ON t.identity = f.identity WHERE t.tag_id = ?1",
            ) else {
                return Vec::new();
            };
            stmt.query_map(params![tag], |r| Ok((r.get(0)?, r.get(1)?)))
                .map(|rows| rows.filter_map(Result::ok).collect())
                .unwrap_or_default()
        };
        let mut found: Vec<(PathBuf, Entry, Vec<i64>)> = rows
            .into_iter()
            .filter_map(|(identity, path)| {
                let path = PathBuf::from(path);
                if ops::identity(&path).as_deref() != Some(&identity) {
                    return None;
                }
                let meta = std::fs::symlink_metadata(&path).ok()?;
                let name = path.file_name()?.to_string_lossy().into_owned();
                let tags = self.tag_ids(&identity);
                Some((path, listing::entry_from(name, &meta), tags))
            })
            .collect();
        found.sort_by_cached_key(|(_, e, _)| (!e.is_dir, e.name.to_lowercase()));
        found
    }

    fn tag_ids(&self, identity: &str) -> Vec<i64> {
        let conn = self.conn();
        let Ok(mut stmt) =
            conn.prepare("SELECT tag_id FROM file_tags WHERE identity = ?1 ORDER BY tag_id")
        else {
            return Vec::new();
        };
        stmt.query_map(params![identity], |r| r.get(0))
            .map(|rows| rows.filter_map(Result::ok).collect())
            .unwrap_or_default()
    }

    /// `from` was renamed or moved to `to`: what was tagged there, or inside it, is now at
    /// the new place. The identity, and so the tags, don't change.
    pub fn relocated(&self, from: &Path, to: &Path) {
        self.relocated_all(&[(from.to_path_buf(), to.to_path_buf())]);
    }

    /// Several renames or moves at once, in the order they happened, in one transaction.
    pub fn relocated_all(&self, moves: &[(PathBuf, PathBuf)]) {
        let mut conn = self.conn();
        // Most people have no tags to follow.
        let any = conn
            .query_row("SELECT 1 FROM files LIMIT 1", [], |_| Ok(true))
            .unwrap_or(false);
        if !any {
            return;
        }
        let Ok(tx) = conn.transaction() else {
            return;
        };
        for (from, to) in moves {
            relocate(&tx, from, to);
        }
        let _ = tx.commit();
    }
}

fn relocate(tx: &rusqlite::Transaction<'_>, from: &Path, to: &Path) {
    let (from_key, to_key) = (key(from), key(to));
    let to_text = to.to_string_lossy();
    if from_key == to_key {
        // Only the case changed: the tags stay, and the name is spelled the new way.
        let _ = tx.execute(
            "UPDATE files SET path = ?2 WHERE norm = ?1",
            params![from_key, to_text],
        );
        return;
    }
    let sep = if from_key.contains('\\') { '\\' } else { '/' };
    let inside = format!("{from_key}{sep}");
    // Everything below `from_key` sorts between `inside` and the next character after the
    // separator, which an index can answer without scanning.
    let above = format!("{from_key}{}", char::from(sep as u8 + 1));
    let Ok(mut stmt) = tx.prepare(
        "SELECT identity, path, norm FROM files WHERE norm = ?1 OR (norm >= ?2 AND norm < ?3)",
    ) else {
        return;
    };
    let rows: Vec<(String, String, String)> = stmt
        .query_map(params![from_key, inside, above], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .map(|rows| rows.filter_map(Result::ok).collect())
        .unwrap_or_default();
    drop(stmt);
    for (identity, old, old_norm) in rows {
        let new = if old_norm == from_key {
            to_text.clone().into_owned()
        } else {
            // What lies below the moved folder keeps its own spelling.
            let rest: String = old.chars().skip(inside.chars().count()).collect();
            format!("{}{sep}{rest}", to_text.trim_end_matches(['\\', '/']))
        };
        let new_path = PathBuf::from(&new);
        let dir = new_path.parent().map(key).unwrap_or_default();
        let _ = tx.execute(
            "UPDATE files SET path = ?2, norm = ?3, dir = ?4 WHERE identity = ?1",
            params![identity, new, key(&new_path), dir],
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn index() -> Tags {
        Tags::open(None).unwrap()
    }

    fn file(path: &Path) -> PathBuf {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "x").unwrap();
        path.to_path_buf()
    }

    #[test]
    fn it_starts_with_the_seven_colour_tags() {
        let tags = index().list();
        let names: Vec<_> = tags.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(
            names,
            ["Red", "Orange", "Yellow", "Green", "Blue", "Purple", "Gray"]
        );
        assert_eq!(tags[0].id, 1);
        assert_eq!(tags[6].color.as_deref(), Some("gray"));
    }

    #[test]
    fn named_tags_are_added_checked_and_removed() {
        let t = index();
        let work = t.create("  Work  ", Some("red")).unwrap();
        assert_eq!(
            (work.name.as_str(), work.color.as_deref()),
            ("Work", Some("red"))
        );
        assert!(work.id > 7);
        assert!(
            t.create("work", None).is_err(),
            "names are unique, ignoring case"
        );
        assert!(t.create("   ", None).is_err());
        assert!(t.create("x", Some("pink")).is_err());
        assert!(t.delete(3).is_err(), "colour tags stay");
        t.delete(work.id).unwrap();
        assert_eq!(t.list().len(), 7);
    }

    #[test]
    fn toggle_tags_files_and_the_folder_lists_them() {
        let dir = tempfile::tempdir().unwrap();
        let a = file(&dir.path().join("a.txt"));
        let b = file(&dir.path().join("b.txt"));
        file(&dir.path().join("c.txt"));
        let t = index();

        assert!(t.toggle(&[a.clone(), b.clone()], 1).unwrap().on);
        assert!(t.toggle(std::slice::from_ref(&a), 4).unwrap().on);
        let listed = t.in_dir(dir.path());
        assert_eq!(
            listed,
            [
                ("a.txt".to_owned(), vec![1, 4]),
                ("b.txt".to_owned(), vec![1])
            ]
        );

        // All of them have it, so it comes off all of them.
        assert!(!t.toggle(&[a, b], 1).unwrap().on);
        assert_eq!(t.in_dir(dir.path()), [("a.txt".to_owned(), vec![4])]);
    }

    #[test]
    fn a_mix_gets_the_tag_instead_of_losing_it() {
        let dir = tempfile::tempdir().unwrap();
        let a = file(&dir.path().join("a.txt"));
        let b = file(&dir.path().join("b.txt"));
        let t = index();
        t.toggle(std::slice::from_ref(&a), 2).unwrap();
        assert!(
            t.toggle(&[a, b], 2).unwrap().on,
            "one lacked it, so both have it"
        );
        assert_eq!(t.in_dir(dir.path()).len(), 2);
    }

    #[test]
    fn tags_follow_a_file_that_was_renamed_or_moved() {
        let dir = tempfile::tempdir().unwrap();
        let a = file(&dir.path().join("a.txt"));
        let t = index();
        t.toggle(std::slice::from_ref(&a), 1).unwrap();

        let renamed = dir.path().join("renamed.txt");
        fs::rename(&a, &renamed).unwrap();
        t.relocated(&a, &renamed);
        assert_eq!(t.in_dir(dir.path()), [("renamed.txt".to_owned(), vec![1])]);

        let moved = dir.path().join("sub").join("renamed.txt");
        fs::create_dir_all(moved.parent().unwrap()).unwrap();
        fs::rename(&renamed, &moved).unwrap();
        t.relocated(&renamed, &moved);
        assert!(matches!(t.in_dir(dir.path()).as_slice(), []));
        assert_eq!(
            t.in_dir(&dir.path().join("sub")),
            [("renamed.txt".to_owned(), vec![1])]
        );
    }

    #[test]
    fn tags_follow_the_files_inside_a_folder_that_moved() {
        let dir = tempfile::tempdir().unwrap();
        let inner = file(&dir.path().join("old").join("deep").join("f.txt"));
        let t = index();
        t.toggle(std::slice::from_ref(&inner), 5).unwrap();

        let new = dir.path().join("new");
        fs::rename(dir.path().join("old"), &new).unwrap();
        t.relocated(&dir.path().join("old"), &new);
        assert_eq!(t.in_dir(&new.join("deep")), [("f.txt".to_owned(), vec![5])]);
    }

    #[test]
    fn another_file_in_the_place_of_a_tagged_one_is_not_tagged() {
        let dir = tempfile::tempdir().unwrap();
        let a = file(&dir.path().join("a.txt"));
        let t = index();
        t.toggle(std::slice::from_ref(&a), 1).unwrap();

        // Deleted and recreated: a different file now has the name.
        fs::remove_file(&a).unwrap();
        file(&a);
        assert!(matches!(t.in_dir(dir.path()).as_slice(), []));
    }

    #[test]
    fn a_copy_does_not_carry_the_tags() {
        let dir = tempfile::tempdir().unwrap();
        let a = file(&dir.path().join("a.txt"));
        let t = index();
        t.toggle(std::slice::from_ref(&a), 1).unwrap();
        fs::copy(&a, dir.path().join("copy.txt")).unwrap();
        let listed = t.in_dir(dir.path());
        assert_eq!(listed, [("a.txt".to_owned(), vec![1])]);
    }

    #[test]
    fn files_with_a_tag_come_from_every_folder_and_skip_what_is_gone() {
        let dir = tempfile::tempdir().unwrap();
        let a = file(&dir.path().join("x").join("a.txt"));
        let b = file(&dir.path().join("y").join("B.txt"));
        let gone = file(&dir.path().join("z").join("gone.txt"));
        let other = file(&dir.path().join("z").join("other.txt"));
        let t = index();
        t.toggle(&[a.clone(), b.clone(), gone.clone()], 3).unwrap();
        t.toggle(std::slice::from_ref(&other), 4).unwrap();
        fs::remove_file(&gone).unwrap();

        let found = t.files_with(3);
        let names: Vec<_> = found.iter().map(|(_, e, _)| e.name.as_str()).collect();
        assert_eq!(names, ["a.txt", "B.txt"]);
        assert_eq!(found[0].0, a);
        assert_eq!(found[0].2, [3]);
    }

    #[test]
    fn a_case_only_rename_keeps_the_tag_and_shows_the_new_spelling() {
        let dir = tempfile::tempdir().unwrap();
        let a = file(&dir.path().join("a.txt"));
        let t = index();
        t.toggle(std::slice::from_ref(&a), 1).unwrap();
        let upper = dir.path().join("A.txt");
        fs::rename(&a, &upper).unwrap();
        t.relocated(&a, &upper);
        assert_eq!(t.in_dir(dir.path()), [("A.txt".to_owned(), vec![1])]);
    }

    #[test]
    fn folders_can_be_tagged_and_followed() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("docs");
        fs::create_dir_all(&folder).unwrap();
        let t = index();
        let done = t.toggle(std::slice::from_ref(&folder), 6).unwrap();
        assert_eq!((done.on, done.skipped), (true, 0));
        assert_eq!(t.in_dir(dir.path()), [("docs".to_owned(), vec![6])]);
    }

    #[test]
    fn items_that_cannot_be_tagged_are_counted() {
        let dir = tempfile::tempdir().unwrap();
        let a = file(&dir.path().join("a.txt"));
        let t = index();
        let done = t.toggle(&[a, dir.path().join("missing.txt")], 1).unwrap();
        assert_eq!((done.on, done.skipped), (true, 1));
    }

    #[test]
    fn many_moves_are_followed_in_one_go() {
        let dir = tempfile::tempdir().unwrap();
        let a = file(&dir.path().join("a.txt"));
        let t = index();
        t.toggle(std::slice::from_ref(&a), 1).unwrap();
        let b = dir.path().join("b.txt");
        let c = dir.path().join("c.txt");
        fs::rename(&a, &c).unwrap();
        // a -> b, then b -> c, as two steps one after the other.
        t.relocated_all(&[(a, b.clone()), (b, c)]);
        assert_eq!(t.in_dir(dir.path()), [("c.txt".to_owned(), vec![1])]);
    }
}
