//! The clipboard shelf: items marked with cut or copy, kept in SQLite so they survive a
//! restart. Nothing here touches the files themselves (ADR 0003): the shelf only remembers
//! what to move or copy when the user pastes.

use crate::ops;
use rusqlite::{Connection, params};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShelfItem {
    pub path: String,
    /// "copy" or "cut".
    pub mode: String,
    /// Whether the item is still there; one that went missing since it was shelved is shown
    /// greyed out.
    pub exists: bool,
    pub is_dir: bool,
    pub size: u64,
}

pub struct Shelf {
    conn: Mutex<Connection>,
}

fn key(path: &Path) -> String {
    ops::comparable(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

impl Shelf {
    /// Opens the shelf in `file`, or an in-memory one.
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
            "CREATE TABLE IF NOT EXISTS shelf (
                 id   INTEGER PRIMARY KEY AUTOINCREMENT,
                 norm TEXT NOT NULL UNIQUE,
                 path TEXT NOT NULL,
                 mode TEXT NOT NULL CHECK (mode IN ('copy', 'cut'))
             );",
        )?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn conn(&self) -> MutexGuard<'_, Connection> {
        self.conn
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The shelved items in the order they were added, each checked against the disk.
    pub fn list(&self) -> Vec<ShelfItem> {
        let rows: Vec<(String, String)> = {
            let conn = self.conn();
            let Ok(mut stmt) = conn.prepare("SELECT path, mode FROM shelf ORDER BY id") else {
                return Vec::new();
            };
            stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                .map(|rows| rows.filter_map(Result::ok).collect())
                .unwrap_or_default()
        };
        rows.into_iter()
            .map(|(path, mode)| {
                let meta = std::fs::symlink_metadata(&path).ok();
                ShelfItem {
                    exists: meta.is_some(),
                    is_dir: meta.as_ref().is_some_and(std::fs::Metadata::is_dir),
                    size: meta
                        .as_ref()
                        .filter(|m| m.is_file())
                        .map_or(0, std::fs::Metadata::len),
                    path,
                    mode,
                }
            })
            .collect()
    }

    /// Adds `paths` with `mode` ("copy" or "cut"). An item already there keeps its place and
    /// takes the new mode.
    pub fn add(&self, paths: &[PathBuf], mode: &str) -> Result<(), String> {
        if mode != "copy" && mode != "cut" {
            return Err(format!("\"{mode}\" is not a shelf mode"));
        }
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        for path in paths {
            tx.execute(
                "INSERT INTO shelf (norm, path, mode) VALUES (?1, ?2, ?3)
                 ON CONFLICT(norm) DO UPDATE SET mode = ?3, path = ?2",
                params![key(path), path.to_string_lossy(), mode],
            )
            .map_err(|e| e.to_string())?;
        }
        tx.commit().map_err(|e| e.to_string())
    }

    /// Changes one item's mode.
    pub fn set_mode(&self, path: &Path, mode: &str) -> Result<(), String> {
        if mode != "copy" && mode != "cut" {
            return Err(format!("\"{mode}\" is not a shelf mode"));
        }
        self.conn()
            .execute(
                "UPDATE shelf SET mode = ?2 WHERE norm = ?1",
                params![key(path), mode],
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    /// Takes items off the shelf. The files are not touched.
    pub fn remove(&self, paths: &[PathBuf]) -> Result<(), String> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        for path in paths {
            tx.execute("DELETE FROM shelf WHERE norm = ?1", params![key(path)])
                .map_err(|e| e.to_string())?;
        }
        tx.commit().map_err(|e| e.to_string())
    }

    pub fn clear(&self) -> Result<(), String> {
        self.conn()
            .execute("DELETE FROM shelf", [])
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn file(path: &Path) -> PathBuf {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "hello").unwrap();
        path.to_path_buf()
    }

    #[test]
    fn items_stack_up_in_order_across_folders() {
        let dir = tempfile::tempdir().unwrap();
        let a = file(&dir.path().join("x").join("a.txt"));
        let b = file(&dir.path().join("y").join("b.txt"));
        let shelf = Shelf::open(None).unwrap();
        shelf.add(std::slice::from_ref(&a), "copy").unwrap();
        shelf.add(std::slice::from_ref(&b), "cut").unwrap();

        let items = shelf.list();
        let modes: Vec<_> = items.iter().map(|i| (i.mode.as_str(), i.size)).collect();
        assert_eq!(modes, [("copy", 5), ("cut", 5)]);
        assert!(items.iter().all(|i| i.exists && !i.is_dir));
    }

    #[test]
    fn adding_an_item_again_updates_its_mode_and_keeps_its_place() {
        let dir = tempfile::tempdir().unwrap();
        let a = file(&dir.path().join("a.txt"));
        let b = file(&dir.path().join("b.txt"));
        let shelf = Shelf::open(None).unwrap();
        shelf.add(&[a.clone(), b], "copy").unwrap();
        shelf.add(std::slice::from_ref(&a), "cut").unwrap();

        let items = shelf.list();
        assert_eq!(items.len(), 2, "no duplicate");
        assert_eq!(items[0].path, a.to_string_lossy());
        assert_eq!(items[0].mode, "cut");
    }

    #[test]
    fn items_can_change_mode_and_be_removed_without_touching_the_files() {
        let dir = tempfile::tempdir().unwrap();
        let a = file(&dir.path().join("a.txt"));
        let b = file(&dir.path().join("b.txt"));
        let shelf = Shelf::open(None).unwrap();
        shelf.add(&[a.clone(), b.clone()], "copy").unwrap();

        shelf.set_mode(&a, "cut").unwrap();
        assert_eq!(shelf.list()[0].mode, "cut");
        shelf.remove(std::slice::from_ref(&a)).unwrap();
        assert_eq!(shelf.list().len(), 1);
        shelf.clear().unwrap();
        assert!(matches!(shelf.list().as_slice(), []));
        assert!(a.exists() && b.exists(), "the shelf never touches files");
    }

    #[test]
    fn an_item_that_went_missing_is_flagged_not_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let a = file(&dir.path().join("a.txt"));
        let shelf = Shelf::open(None).unwrap();
        shelf.add(std::slice::from_ref(&a), "copy").unwrap();
        fs::remove_file(&a).unwrap();
        let items = shelf.list();
        assert_eq!(items.len(), 1);
        assert!(!items[0].exists);
    }

    #[test]
    fn a_bad_mode_is_refused() {
        let shelf = Shelf::open(None).unwrap();
        assert!(shelf.add(&[PathBuf::from("a")], "delete").is_err());
        assert!(shelf.set_mode(Path::new("a"), "delete").is_err());
    }

    #[test]
    fn the_shelf_survives_reopening() {
        let dir = tempfile::tempdir().unwrap();
        let a = file(&dir.path().join("a.txt"));
        let db = dir.path().join("shelf.sqlite");
        Shelf::open(Some(&db))
            .unwrap()
            .add(std::slice::from_ref(&a), "cut")
            .unwrap();
        let again = Shelf::open(Some(&db)).unwrap().list();
        assert_eq!(again.len(), 1);
        assert_eq!(again[0].mode, "cut");
    }
}
