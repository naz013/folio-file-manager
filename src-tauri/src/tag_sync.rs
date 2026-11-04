//! Two-way sync between the tag index and the tags the system keeps on files (Finder tags on
//! macOS, `user.xdg.tags` on Linux; see [`crate::native_tags`]).
//!
//! The two sides can both change between looks, so a file is not simply copied one way. For each
//! file the sync keeps a **base**: the names both sides agreed on the last time. A name is then
//! - added if either side has it and the base did not,
//! - removed if either side lacks it and the base had it,
//! - otherwise as it was.
//!
//! One side can only add what the base lacked and only remove what the base had, so the two
//! can never disagree about a name: there is no conflict to settle, and running the sync twice
//! changes nothing the second time. The first time there is no base, which makes it a union:
//! nothing on either side is lost.
//!
//! Only names both worlds can hold take part. A tag Folio has that the system can't store
//! (a comma on Linux, a name over Folio's limit) is left where it is, and so is a native name
//! Folio can't take.

use crate::native_tags::{self, Item, Store};
use crate::ops;
use crate::tags::{MAX_NAME, Tags, key};
use rusqlite::params;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

/// The most items of one folder looked at in a pass; a bigger folder is synced as it is used.
const MAX_DIR: usize = 20_000;

/// What a sync did, in files.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SyncReport {
    /// Files whose tags in the index changed to match the system.
    pub imported: usize,
    /// Files whose tags on the system changed to match the index.
    pub exported: usize,
    /// Files left out: no identity, tags that couldn't be read, or a write that failed.
    pub failed: usize,
}

impl SyncReport {
    fn add(&mut self, done: Result<(bool, bool), ()>) {
        match done {
            Ok((imported, exported)) => {
                self.imported += usize::from(imported);
                self.exported += usize::from(exported);
            }
            Err(()) => self.failed += 1,
        }
    }
}

/// Names as a map from their lower-case form to a spelling, so `Work` and `work` are one tag
/// (as in the index).
type Names = BTreeMap<String, String>;

fn names(list: impl IntoIterator<Item = String>) -> Names {
    let mut out = Names::new();
    for n in list {
        out.entry(n.to_lowercase()).or_insert(n);
    }
    out
}

/// Whether both sides can hold a tag with this name.
fn syncable(name: &str) -> bool {
    let name = name.trim();
    !name.is_empty() && name.chars().count() <= MAX_NAME && native_tags::can_store(name)
}

/// The names after a sync, from the base and both sides (see the module docs). The spelling
/// of a name is the index's if it has it, else the system's, else the base's.
pub(crate) fn merge(base: &Names, local: &Names, native: &Names) -> Names {
    let mut out = Names::new();
    let all = base.keys().chain(local.keys()).chain(native.keys());
    for k in all {
        let (b, l, n) = (
            base.contains_key(k),
            local.contains_key(k),
            native.contains_key(k),
        );
        let added = !b && (l || n);
        let removed = b && (!l || !n);
        if (b || added) && !removed {
            let spelling = local
                .get(k)
                .or_else(|| native.get(k))
                .or_else(|| base.get(k))
                .cloned()
                .unwrap_or_default();
            out.insert(k.clone(), spelling);
        }
    }
    out
}

impl Tags {
    /// Brings the tags of `paths` and the system's into line. Without a system store (Windows)
    /// it does nothing.
    pub fn sync_with_system(&self, paths: &[PathBuf]) -> SyncReport {
        match native_tags::system() {
            Some(store) => self.sync(store, paths),
            None => SyncReport::default(),
        }
    }

    /// The items directly in `dir`, brought into line with the system's tags: for a folder
    /// someone tagged in Finder, or with another program, since Folio last looked.
    pub fn sync_dir_with_system(&self, dir: &Path) -> SyncReport {
        let Some(store) = native_tags::system() else {
            return SyncReport::default();
        };
        self.sync_dir(store, dir)
    }

    /// A folder is read once and each item's tags once; only an item that has tags on the
    /// system, or that the index or the last sync knew, costs more than reading its attribute.
    pub(crate) fn sync_dir(&self, store: &dyn Store, dir: &Path) -> SyncReport {
        let Ok(read) = std::fs::read_dir(dir) else {
            return SyncReport::default();
        };
        let paths: Vec<PathBuf> = read
            .filter_map(|e| e.ok().map(|e| e.path()))
            .take(MAX_DIR)
            .collect();
        let known = self.known_in(dir);
        let mut report = SyncReport::default();
        for path in &paths {
            let Ok(native) = store.read(path) else {
                report.failed += 1;
                continue;
            };
            if native.is_empty() && !known.contains(&key(path)) {
                continue;
            }
            report.add(self.sync_one(store, path, native));
        }
        report
    }

    /// Brings the tags of each of `paths` and the store's into line.
    pub(crate) fn sync(&self, store: &dyn Store, paths: &[PathBuf]) -> SyncReport {
        let mut report = SyncReport::default();
        for path in paths {
            match store.read(path) {
                Ok(native) => report.add(self.sync_one(store, path, native)),
                Err(_) => report.failed += 1,
            }
        }
        report
    }

    /// Returns whether the index changed and whether the system's tags changed.
    fn sync_one(
        &self,
        store: &dyn Store,
        path: &Path,
        native_all: Vec<Item>,
    ) -> Result<(bool, bool), ()> {
        let identity = ops::identity(path).ok_or(())?;
        let at = key(path);
        let (local_all, base) = self.sync_state(&identity, &at);
        // The names both can hold take part; the rest stay where they are.
        let (native, native_rest): (Vec<_>, Vec<_>) = native_all
            .into_iter()
            .partition(|item| syncable(&item.name));
        let local_names = names(local_all.into_iter().filter(|n| syncable(n)));
        let native_names = names(native.iter().map(|i| i.name.clone()));
        if local_names.is_empty() && native_names.is_empty() && base.is_empty() {
            return Ok((false, false));
        }
        let merged = merge(&base, &local_names, &native_names);

        let imported = !same(&merged, &local_names);
        if imported {
            self.apply_to_index(&identity, path, &local_names, &merged)
                .map_err(|_| ())?;
        }
        let exported = !same(&merged, &native_names);
        if exported {
            // A tag that stays is written as it was stored, so a colour Finder keeps beside it
            // survives; only a tag new to the file is written afresh.
            let stored: HashMap<String, &str> = native
                .iter()
                .map(|i| (i.name.to_lowercase(), i.raw.as_str()))
                .collect();
            let mut out: Vec<String> = native_rest.into_iter().map(|i| i.raw).collect();
            out.extend(merged.iter().map(|(k, name)| {
                stored
                    .get(k)
                    .map_or_else(|| store.item(name), |raw| (*raw).to_owned())
            }));
            store.write(path, &out).map_err(|_| ())?;
        }
        // Remembered only once both sides have it, so a failed write is tried again.
        if imported || exported || !same(&merged, &base) {
            self.save_base(&identity, path, &merged).map_err(|_| ())?;
        }
        Ok((imported, exported))
    }

    /// The places in `dir` the index or the last sync knows a file at.
    fn known_in(&self, dir: &Path) -> HashSet<String> {
        let conn = self.conn();
        let dir = key(dir);
        conn.prepare(
            "SELECT norm FROM files WHERE dir = ?1 UNION SELECT norm FROM native_base WHERE dir = ?1",
        )
        .and_then(|mut s| {
            s.query_map(params![dir], |r| r.get(0))
                .map(|rows| rows.filter_map(Result::ok).collect())
        })
        .unwrap_or_default()
    }

    /// The tag names the index has on the file, and the base the last sync left. A base made
    /// for another place is not this file's: the system hands out a number again after a file
    /// is gone, and a stale base would read a new file's tags as taken away.
    fn sync_state(&self, identity: &str, at: &str) -> (Vec<String>, Names) {
        let conn = self.conn();
        let local: Vec<String> = conn
            .prepare_cached(
                "SELECT t.name FROM file_tags f JOIN tags t ON t.id = f.tag_id
                 WHERE f.identity = ?1",
            )
            .and_then(|mut s| {
                s.query_map(params![identity], |r| r.get(0))
                    .map(|rows| rows.filter_map(Result::ok).collect())
            })
            .unwrap_or_default();
        let base: Vec<String> = conn
            .query_row(
                "SELECT names FROM native_base WHERE identity = ?1 AND norm = ?2",
                params![identity, at],
                |r| r.get::<_, String>(0),
            )
            .ok()
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default();
        (local, names(base))
    }

    /// Makes the file's tags in the index `merged`, for the names that take part in the sync.
    fn apply_to_index(
        &self,
        identity: &str,
        path: &Path,
        before: &Names,
        merged: &Names,
    ) -> rusqlite::Result<()> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        if !merged.is_empty() {
            let dir = path.parent().map(key).unwrap_or_default();
            tx.execute(
                "INSERT INTO files (identity, path, norm, dir) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(identity) DO UPDATE SET path = ?2, norm = ?3, dir = ?4",
                params![identity, path.to_string_lossy(), key(path), dir],
            )?;
        }
        for (k, name) in merged {
            if before.contains_key(k) {
                continue;
            }
            // A tag the index doesn't have yet is made, without a colour.
            tx.execute(
                "INSERT OR IGNORE INTO tags (name) VALUES (?1)",
                params![name],
            )?;
            tx.execute(
                "INSERT OR IGNORE INTO file_tags (identity, tag_id)
                 SELECT ?1, id FROM tags WHERE name = ?2",
                params![identity, name],
            )?;
        }
        for (k, name) in before {
            if !merged.contains_key(k) {
                tx.execute(
                    "DELETE FROM file_tags WHERE identity = ?1
                     AND tag_id IN (SELECT id FROM tags WHERE name = ?2)",
                    params![identity, name],
                )?;
            }
        }
        // A file with no tag needs no row.
        tx.execute(
            "DELETE FROM files WHERE identity = ?1
             AND NOT EXISTS (SELECT 1 FROM file_tags WHERE identity = ?1)",
            params![identity],
        )?;
        tx.commit()
    }

    fn save_base(&self, identity: &str, path: &Path, merged: &Names) -> rusqlite::Result<()> {
        let conn = self.conn();
        if merged.is_empty() {
            conn.execute(
                "DELETE FROM native_base WHERE identity = ?1",
                params![identity],
            )?;
        } else {
            let list: Vec<&String> = merged.values().collect();
            let json = serde_json::to_string(&list).unwrap_or_default();
            let dir = path.parent().map(key).unwrap_or_default();
            conn.execute(
                "INSERT INTO native_base (identity, names, norm, dir) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(identity) DO UPDATE SET names = ?2, norm = ?3, dir = ?4",
                params![identity, json, key(path), dir],
            )?;
        }
        Ok(())
    }
}

fn same(a: &Names, b: &Names) -> bool {
    a.keys().eq(b.keys())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;

    /// A store in memory, standing in for the system's attributes.
    #[derive(Default)]
    struct Fake {
        tags: RefCell<HashMap<PathBuf, Vec<String>>>,
        writes: RefCell<usize>,
        fail_writes: bool,
    }

    impl Fake {
        fn set(&self, path: &Path, list: &[&str]) {
            self.tags.borrow_mut().insert(
                path.to_path_buf(),
                list.iter().map(|s| (*s).to_owned()).collect(),
            );
        }
        fn get(&self, path: &Path) -> Vec<String> {
            let mut v = self.tags.borrow().get(path).cloned().unwrap_or_default();
            v.sort();
            v
        }
    }

    impl Store for Fake {
        fn read(&self, path: &Path) -> Result<Vec<Item>, String> {
            let list = self.tags.borrow().get(path).cloned().unwrap_or_default();
            Ok(list.iter().map(|n| Item::plain(n)).collect())
        }
        fn item(&self, name: &str) -> String {
            name.to_owned()
        }
        fn write(&self, path: &Path, list: &[String]) -> Result<(), String> {
            if self.fail_writes {
                return Err("read-only".into());
            }
            *self.writes.borrow_mut() += 1;
            self.tags
                .borrow_mut()
                .insert(path.to_path_buf(), list.to_vec());
            Ok(())
        }
    }

    fn set(list: &[&str]) -> Names {
        names(list.iter().map(|s| (*s).to_owned()))
    }

    fn keys(n: &Names) -> Vec<&str> {
        n.keys().map(String::as_str).collect()
    }

    fn file(dir: &Path, name: &str) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, name).unwrap();
        p
    }

    fn index() -> Tags {
        Tags::open(None).unwrap()
    }

    /// The names of the tags the index has on `path`.
    fn local(t: &Tags, path: &Path) -> Vec<String> {
        let id = ops::identity(path).unwrap();
        let mut v: Vec<String> = t
            .tag_ids(&id)
            .into_iter()
            .filter_map(|i| t.list().into_iter().find(|x| x.id == i).map(|x| x.name))
            .collect();
        v.sort();
        v
    }

    #[test]
    fn a_name_one_side_added_is_kept_and_a_name_one_side_removed_is_dropped() {
        let base = set(&["a", "b"]);
        // Index added c; the system removed b.
        let merged = merge(&base, &set(&["a", "b", "c"]), &set(&["a"]));
        assert_eq!(keys(&merged), ["a", "c"]);
    }

    #[test]
    fn with_no_base_the_two_sides_are_joined() {
        let merged = merge(&set(&[]), &set(&["a"]), &set(&["b"]));
        assert_eq!(keys(&merged), ["a", "b"]);
    }

    #[test]
    fn a_name_both_sides_dropped_stays_dropped() {
        assert!(merge(&set(&["a"]), &set(&[]), &set(&[])).is_empty());
    }

    #[test]
    fn merging_is_the_same_whichever_side_is_which_and_when_repeated() {
        let (base, l, n) = (set(&["a", "b"]), set(&["b", "c"]), set(&["a", "b", "d"]));
        let once = merge(&base, &l, &n);
        assert_eq!(keys(&once), ["b", "c", "d"]);
        assert_eq!(merge(&base, &n, &l), once, "sides swapped");
        assert_eq!(merge(&once, &once, &once), once, "nothing left to do");
    }

    #[test]
    fn letter_case_does_not_make_another_tag() {
        let merged = merge(&set(&["Work"]), &set(&["Work"]), &set(&["work"]));
        assert_eq!(keys(&merged), ["work"]);
        assert_eq!(merged["work"], "Work", "the index's spelling wins");
    }

    #[test]
    fn a_tag_added_on_the_system_arrives_in_the_index_and_stays_there() {
        let dir = tempfile::tempdir().unwrap();
        let (t, sys) = (index(), Fake::default());
        let f = file(dir.path(), "a.txt");
        sys.set(&f, &["Red", "Taxes"]);

        let r = t.sync(&sys, std::slice::from_ref(&f));
        assert_eq!((r.imported, r.exported, r.failed), (1, 0, 0));
        assert_eq!(local(&t, &f), ["Red", "Taxes"]);
        assert_eq!(t.in_dir(dir.path()).len(), 1);
        // A second look finds nothing to do.
        assert_eq!(t.sync(&sys, &[f]), SyncReport::default());
    }

    #[test]
    fn a_tag_given_in_folio_is_written_to_the_system() {
        let dir = tempfile::tempdir().unwrap();
        let (t, sys) = (index(), Fake::default());
        let f = file(dir.path(), "a.txt");
        t.toggle(std::slice::from_ref(&f), 1).unwrap();

        let r = t.sync(&sys, std::slice::from_ref(&f));
        assert_eq!((r.imported, r.exported), (0, 1));
        assert_eq!(sys.get(&f), ["Red"]);
    }

    #[test]
    fn a_tag_taken_off_in_folio_leaves_the_system_too() {
        let dir = tempfile::tempdir().unwrap();
        let (t, sys) = (index(), Fake::default());
        let f = file(dir.path(), "a.txt");
        t.toggle(std::slice::from_ref(&f), 1).unwrap();
        t.sync(&sys, std::slice::from_ref(&f));

        // Toggling again takes it off, and the index drops the file's row altogether.
        t.toggle(std::slice::from_ref(&f), 1).unwrap();
        t.sync(&sys, std::slice::from_ref(&f));
        assert!(
            sys.get(&f).is_empty(),
            "it must not come back from the system"
        );
        assert_eq!(local(&t, &f), Vec::<String>::new());
    }

    #[test]
    fn a_tag_taken_off_on_the_system_leaves_the_index() {
        let dir = tempfile::tempdir().unwrap();
        let (t, sys) = (index(), Fake::default());
        let f = file(dir.path(), "a.txt");
        sys.set(&f, &["Red", "Blue"]);
        t.sync(&sys, std::slice::from_ref(&f));

        sys.set(&f, &["Blue"]);
        let r = t.sync(&sys, std::slice::from_ref(&f));
        assert_eq!((r.imported, r.exported), (1, 0));
        assert_eq!(local(&t, &f), ["Blue"]);
    }

    #[test]
    fn changes_on_both_sides_between_looks_are_both_kept() {
        let dir = tempfile::tempdir().unwrap();
        let (t, sys) = (index(), Fake::default());
        let f = file(dir.path(), "a.txt");
        sys.set(&f, &["Red"]);
        t.sync(&sys, std::slice::from_ref(&f));

        // Folio adds Blue; meanwhile Finder adds Home and removes Red.
        t.toggle(std::slice::from_ref(&f), 5).unwrap();
        sys.set(&f, &["Home"]);
        t.sync(&sys, std::slice::from_ref(&f));

        assert_eq!(local(&t, &f), ["Blue", "Home"]);
        assert_eq!(sys.get(&f), ["Blue", "Home"]);
    }

    #[test]
    fn a_folio_rename_follows_to_the_system() {
        let dir = tempfile::tempdir().unwrap();
        let (t, sys) = (index(), Fake::default());
        let f = file(dir.path(), "a.txt");
        sys.set(&f, &["Taxes"]);
        t.sync(&sys, std::slice::from_ref(&f));
        let id = t.list().into_iter().find(|x| x.name == "Taxes").unwrap().id;
        t.rename(id, "Tax 2026").unwrap();

        t.sync(&sys, std::slice::from_ref(&f));
        assert_eq!(sys.get(&f), ["Tax 2026"]);
        assert_eq!(local(&t, &f), ["Tax 2026"]);
    }

    #[test]
    fn what_the_other_side_cannot_hold_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let (t, sys) = (index(), Fake::default());
        let f = file(dir.path(), "a.txt");
        let long = "x".repeat(MAX_NAME + 1);
        sys.set(&f, &["Red", &long]);
        t.sync(&sys, std::slice::from_ref(&f));
        assert_eq!(local(&t, &f), ["Red"], "a name Folio refuses is not taken");

        // Giving the file another tag in Folio keeps the system's own name in its list.
        t.toggle(std::slice::from_ref(&f), 5).unwrap();
        t.sync(&sys, std::slice::from_ref(&f));
        let mut expect = vec!["Blue".to_owned(), "Red".to_owned(), long];
        expect.sort();
        assert_eq!(sys.get(&f), expect);
    }

    #[test]
    fn a_failed_write_is_tried_again_and_loses_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let (t, sys) = (index(), Fake::default());
        let f = file(dir.path(), "a.txt");
        t.toggle(std::slice::from_ref(&f), 1).unwrap();

        let broken = Fake {
            fail_writes: true,
            ..Fake::default()
        };
        let r = t.sync(&broken, std::slice::from_ref(&f));
        assert_eq!(r.failed, 1);
        assert_eq!(local(&t, &f), ["Red"], "the index keeps it");

        let r = t.sync(&sys, std::slice::from_ref(&f));
        assert_eq!((r.exported, r.failed), (1, 0));
        assert_eq!(sys.get(&f), ["Red"]);
    }

    #[test]
    fn a_folder_pass_picks_up_tags_from_all_its_files_and_skips_untagged_ones() {
        let dir = tempfile::tempdir().unwrap();
        let (t, sys) = (index(), Fake::default());
        let (a, b) = (file(dir.path(), "a.txt"), file(dir.path(), "b.txt"));
        file(dir.path(), "c.txt");
        sys.set(&a, &["Red"]);
        sys.set(&b, &["Green"]);

        let r = t.sync_dir(&sys, dir.path());
        assert_eq!((r.imported, r.exported, r.failed), (2, 0, 0));
        assert_eq!(*sys.writes.borrow(), 0);
        let found = t.in_dir(dir.path());
        assert_eq!(found.len(), 2);
    }

    #[test]
    fn merge_over_every_subset_never_loses_a_shared_name_and_is_idempotent() {
        let pool = ["a", "b", "c"];
        let subset = |mask: usize| -> Names {
            names(
                pool.iter()
                    .enumerate()
                    .filter(|(i, _)| mask & (1 << i) != 0)
                    .map(|(_, s)| (*s).to_owned()),
            )
        };
        for (bm, lm, nm) in
            (0..8).flat_map(|b| (0..8).flat_map(move |l| (0..8).map(move |n| (b, l, n))))
        {
            let (base, l, n) = (subset(bm), subset(lm), subset(nm));
            let out = merge(&base, &l, &n);
            let ctx = format!("base={bm:03b} local={lm:03b} native={nm:03b}");
            for k in l.keys().filter(|k| n.contains_key(*k)) {
                assert!(out.contains_key(k), "shared name {k} lost: {ctx}");
            }
            for k in out.keys() {
                assert!(
                    l.contains_key(k) || n.contains_key(k),
                    "invented {k}: {ctx}"
                );
            }
            assert!(
                merge(&base, &n, &l).keys().eq(out.keys()),
                "symmetry: {ctx}"
            );
            // After a sync both sides and the base equal the result; syncing again is a no-op.
            assert_eq!(merge(&out, &out, &out), out, "idempotent: {ctx}");
        }
    }

    #[test]
    fn a_name_the_index_cannot_take_is_kept_on_the_system_when_the_file_also_has_a_folio_tag() {
        let dir = tempfile::tempdir().unwrap();
        let (t, sys) = (index(), Fake::default());
        let f = file(dir.path(), "a.txt");
        let long = "y".repeat(MAX_NAME + 1);
        sys.set(&f, &[&long]);
        t.toggle(std::slice::from_ref(&f), 1).unwrap();

        let r = t.sync(&sys, std::slice::from_ref(&f));
        assert_eq!((r.imported, r.exported, r.failed), (0, 1, 0));
        let mut expect = vec!["Red".to_owned(), long];
        expect.sort();
        assert_eq!(sys.get(&f), expect);
        assert_eq!(local(&t, &f), ["Red"]);
        assert_eq!(t.sync(&sys, &[f]), SyncReport::default(), "settled");
    }

    #[test]
    fn a_tag_deleted_in_folio_leaves_the_system_and_does_not_come_back() {
        let dir = tempfile::tempdir().unwrap();
        let (t, sys) = (index(), Fake::default());
        let f = file(dir.path(), "a.txt");
        sys.set(&f, &["Taxes", "Red"]);
        t.sync(&sys, std::slice::from_ref(&f));
        let id = t.list().into_iter().find(|x| x.name == "Taxes").unwrap().id;
        t.delete(id).unwrap();

        let r = t.sync(&sys, std::slice::from_ref(&f));
        assert_eq!((r.imported, r.exported, r.failed), (0, 1, 0));
        assert_eq!(sys.get(&f), ["Red"]);
        assert_eq!(local(&t, &f), ["Red"]);
        assert_eq!(t.sync(&sys, &[f]), SyncReport::default());
    }

    #[test]
    fn a_renamed_file_keeps_its_tags_and_needs_no_work() {
        let dir = tempfile::tempdir().unwrap();
        let (t, sys) = (index(), Fake::default());
        let f = file(dir.path(), "a.txt");
        t.toggle(std::slice::from_ref(&f), 1).unwrap();
        t.sync(&sys, std::slice::from_ref(&f));

        let moved = dir.path().join("b.txt");
        std::fs::rename(&f, &moved).unwrap();
        // The system carries the attribute with the file.
        let carried = sys.get(&f);
        sys.set(
            &moved,
            &carried.iter().map(String::as_str).collect::<Vec<_>>(),
        );

        assert_eq!(
            t.sync(&sys, std::slice::from_ref(&moved)),
            SyncReport::default()
        );
        assert_eq!(local(&t, &moved), ["Red"]);
    }

    #[test]
    fn interleaved_edits_in_folio_and_on_the_system_converge() {
        let dir = tempfile::tempdir().unwrap();
        let (t, sys) = (index(), Fake::default());
        let f = file(dir.path(), "a.txt");
        let one = std::slice::from_ref(&f);

        t.toggle(one, 1).unwrap(); // Folio: Red
        sys.set(&f, &["Taxes"]); // system: Taxes
        t.sync(&sys, one);
        assert_eq!(local(&t, &f), ["Red", "Taxes"]);
        assert_eq!(sys.get(&f), ["Red", "Taxes"]);

        t.toggle(one, 1).unwrap(); // Folio drops Red
        let mut now = sys.get(&f);
        now.push("Home".into()); // system adds Home
        sys.set(&f, &now.iter().map(String::as_str).collect::<Vec<_>>());
        t.sync(&sys, one);
        assert_eq!(local(&t, &f), ["Home", "Taxes"]);
        assert_eq!(sys.get(&f), ["Home", "Taxes"]);

        sys.set(&f, &[]); // system clears everything
        t.sync(&sys, one);
        assert_eq!(local(&t, &f), Vec::<String>::new());
        assert_eq!(sys.get(&f), Vec::<String>::new());
        assert_eq!(t.sync(&sys, one), SyncReport::default());
    }

    #[test]
    fn a_file_with_no_identity_is_counted_and_left_out() {
        let sys = Fake::default();
        let r = index().sync(&sys, &[PathBuf::from("/no/such/file")]);
        assert_eq!(r.failed, 1);
    }
}
