//! Smart folders: saved searches. A smart folder keeps what to look for, not what was found:
//! it is worked out again each time it is opened, so it is always up to date. Tags are looked
//! up in the tag index; a search with no tag walks a folder.

use crate::fs::Entry;
use crate::search::{self, Matcher, Query};
use crate::tags::{Tags, key};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

const MAX_NAME: usize = 60;
const DAY_MS: u64 = 86_400_000;
/// The most days "changed in the last" can reach: a hundred years.
const MAX_DAYS: u32 = 36_500;

/// What a smart folder looks for. Everything given must hold.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Criteria {
    /// Items must have all of these tags.
    #[serde(default)]
    pub tags: Vec<i64>,
    /// Part of the name, or a pattern with `*` and `?`, as in a search.
    #[serde(default)]
    pub text: String,
    /// Changed within this many days up to now.
    #[serde(default)]
    pub modified_days: Option<u32>,
    /// Only items in this folder or below. Needed when no tag is given: that is where the
    /// search walks.
    #[serde(default)]
    pub folder: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SmartFolder {
    pub id: i64,
    pub name: String,
    pub criteria: Criteria,
}

/// What a walk that was stopped says. The window knows it and ignores it: a newer look at the
/// same smart folder took over, and its answer is the one that counts.
pub const SUPERSEDED: &str = "A newer look at this smart folder took over";

/// What a deleted tag's id is replaced with in the smart folders that used it. No tag has it,
/// so such a folder keeps failing, even once a new tag is given the old tag's number.
const GONE: i64 = 0;

/// Marks `tag` as deleted in every smart folder that names it.
pub(crate) fn forget_tag(tx: &rusqlite::Transaction, tag: i64) -> rusqlite::Result<()> {
    let rows: Vec<(i64, String)> = tx
        .prepare("SELECT id, criteria FROM smart_folders")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    for (id, json) in rows {
        let Ok(mut criteria) = serde_json::from_str::<Criteria>(&json) else {
            continue;
        };
        if !criteria.tags.contains(&tag) {
            continue;
        }
        for t in &mut criteria.tags {
            if *t == tag {
                *t = GONE;
            }
        }
        criteria.tags.sort_unstable();
        criteria.tags.dedup();
        if let Ok(json) = serde_json::to_string(&criteria) {
            tx.execute(
                "UPDATE smart_folders SET criteria = ?2 WHERE id = ?1",
                params![id, json],
            )?;
        }
    }
    Ok(())
}

/// An item found: its path, its entry, and its tag ids.
pub type Found = (PathBuf, Entry, Vec<i64>);

impl Criteria {
    /// The criteria without blanks and repeats, or why they can't be used.
    fn checked(&self) -> Result<Self, String> {
        let mut tags = self.tags.clone();
        tags.sort_unstable();
        tags.dedup();
        let text = self.text.trim().to_owned();
        let folder = self
            .folder
            .as_deref()
            .map(str::trim)
            .filter(|f| !f.is_empty())
            .map(str::to_owned);
        if tags.is_empty() && text.is_empty() && self.modified_days.is_none() {
            return Err("Choose at least a tag, part of a name, or a time".into());
        }
        if tags.is_empty() && folder.is_none() {
            return Err("Choose a folder to look in, or a tag".into());
        }
        // A relative path would mean whatever folder the program was started from.
        if folder
            .as_deref()
            .is_some_and(|f| !Path::new(f).is_absolute())
        {
            return Err(
                "The folder to look in must be a full path, such as C:\\Users\\me\\Documents"
                    .into(),
            );
        }
        if self.modified_days.is_some_and(|d| d == 0 || d > MAX_DAYS) {
            return Err(format!("Days must be between 1 and {MAX_DAYS}"));
        }
        Ok(Self {
            tags,
            text,
            modified_days: self.modified_days,
            folder,
        })
    }

    /// The oldest change time (milliseconds since 1970) that still counts, at `now`.
    fn since(&self, now: u64) -> Option<u64> {
        self.modified_days
            .map(|d| now.saturating_sub(u64::from(d) * DAY_MS))
    }

    fn query(&self, now: u64) -> Query {
        Query {
            text: self.text.clone(),
            modified_since: self.since(now),
            ..Query::default()
        }
    }
}

/// Whether `path` is `folder` or inside it.
fn under(folder: &Path, path: &Path) -> bool {
    Path::new(&key(path)).starts_with(Path::new(&key(folder)))
}

/// Items found by walking `criteria.folder`: for a smart folder with no tag. Stops early if
/// `cancel` is set; what was found so far is returned.
pub fn walk(criteria: &Criteria, now: u64, cancel: &AtomicBool) -> Result<Vec<Found>, String> {
    let criteria = criteria.checked()?;
    let Some(folder) = criteria.folder.as_deref() else {
        return Err("Choose a folder to look in".into());
    };
    let mut found: Vec<Found> = Vec::new();
    // Hits are collected as they come, so a stopped walk has a part: it must not pass for all.
    search::search(
        Path::new(folder),
        &criteria.query(now),
        cancel,
        &mut |hits| {
            found.extend(hits.into_iter().map(|h| {
                (
                    PathBuf::from(h.path),
                    Entry {
                        name: h.name,
                        is_dir: h.is_dir,
                        size: h.size,
                        modified: h.modified,
                        hidden: h.hidden,
                    },
                    // Like a search, the matches show no tags: who carries one is not looked up.
                    Vec::new(),
                )
            }));
            true
        },
    );
    if cancel.load(Ordering::Relaxed) {
        return Err(SUPERSEDED.into());
    }
    sort(&mut found);
    Ok(found)
}

fn sort(found: &mut [Found]) {
    found.sort_by_cached_key(|(_, e, _)| (!e.is_dir, e.name.to_lowercase()));
}

impl Tags {
    /// Every smart folder, by name.
    pub fn smart_list(&self) -> Vec<SmartFolder> {
        let conn = self.conn();
        let Ok(mut stmt) = conn.prepare("SELECT id, name, criteria FROM smart_folders") else {
            return Vec::new();
        };
        let mut all: Vec<SmartFolder> = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })
            .map(|rows| {
                rows.filter_map(Result::ok)
                    .filter_map(|(id, name, json)| {
                        // A row that can't be read is left out, not shown broken.
                        let criteria = serde_json::from_str(&json).ok()?;
                        Some(SmartFolder { id, name, criteria })
                    })
                    .collect()
            })
            .unwrap_or_default();
        all.sort_by_cached_key(|s| s.name.to_lowercase());
        all
    }

    pub fn smart_get(&self, id: i64) -> Result<SmartFolder, String> {
        self.smart_list()
            .into_iter()
            .find(|s| s.id == id)
            .ok_or_else(|| "This smart folder no longer exists".into())
    }

    /// Adds a smart folder, or changes the one with `id`.
    pub fn smart_save(
        &self,
        id: Option<i64>,
        name: &str,
        criteria: &Criteria,
    ) -> Result<SmartFolder, String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("Give the smart folder a name".into());
        }
        if name.chars().count() > MAX_NAME {
            return Err(format!("A name can have at most {MAX_NAME} characters"));
        }
        let criteria = criteria.checked()?;
        let json = serde_json::to_string(&criteria).map_err(|e| e.to_string())?;
        let conn = self.conn();
        let known: std::collections::HashSet<i64> = conn
            .prepare("SELECT id FROM tags")
            .and_then(|mut s| s.query_map([], |r| r.get(0))?.collect::<Result<_, _>>())
            .map_err(|e| e.to_string())?;
        if criteria.tags.iter().any(|t| !known.contains(t)) {
            return Err("A tag chosen here has been deleted. Choose another.".into());
        }
        let taken = |e: rusqlite::Error| match e {
            rusqlite::Error::SqliteFailure(f, _)
                if f.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                format!("There is already a smart folder called \"{name}\"")
            }
            other => other.to_string(),
        };
        let id = if let Some(id) = id {
            let changed = conn
                .execute(
                    "UPDATE smart_folders SET name = ?2, criteria = ?3 WHERE id = ?1",
                    params![id, name, json],
                )
                .map_err(taken)?;
            if changed == 0 {
                return Err("This smart folder no longer exists".into());
            }
            id
        } else {
            conn.execute(
                "INSERT INTO smart_folders (name, criteria) VALUES (?1, ?2)",
                params![name, json],
            )
            .map_err(taken)?;
            conn.last_insert_rowid()
        };
        Ok(SmartFolder {
            id,
            name: name.to_owned(),
            criteria,
        })
    }

    pub fn smart_delete(&self, id: i64) -> Result<(), String> {
        self.conn()
            .execute("DELETE FROM smart_folders WHERE id = ?1", params![id])
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    /// The items a smart folder with tags matches, from the tag index: every item that has all
    /// its tags and fits the rest. An item that is gone or replaced by another file is left out.
    /// Fails if a tag it names has been deleted: dropping it would widen the folder.
    pub fn smart_tagged(&self, criteria: &Criteria, now: u64) -> Result<Vec<Found>, String> {
        let criteria = criteria.checked()?;
        let Some(&first) = criteria.tags.first() else {
            return Err("This smart folder names no tag".into());
        };
        let exists = |id: i64| {
            self.conn()
                .query_row("SELECT 1 FROM tags WHERE id = ?1", params![id], |_| Ok(()))
                .optional()
                .is_ok_and(|row| row.is_some())
        };
        if !criteria.tags.iter().all(|&t| exists(t)) {
            return Err(
                "A tag this smart folder uses has been deleted. Edit it to choose another.".into(),
            );
        }
        let matcher = Matcher::new(&criteria.query(now));
        let folder = criteria.folder.as_deref().map(PathBuf::from);
        let mut found = self.files_with(first);
        found.retain(|(path, entry, ids)| {
            criteria.tags.iter().all(|t| ids.contains(t))
                && matcher.name_fits(&entry.name)
                && matcher.date_fits(entry)
                && folder.as_deref().is_none_or(|f| under(f, path))
        });
        sort(&mut found);
        Ok(found)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{Duration, SystemTime};

    fn now_ms() -> u64 {
        u64::try_from(
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_millis(),
        )
        .unwrap()
    }

    fn age(path: &Path, days: u64) {
        let file = fs::File::options().write(true).open(path).unwrap();
        file.set_modified(SystemTime::now() - Duration::from_secs(days * 86_400))
            .unwrap();
    }

    fn file(dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, "x").unwrap();
        path
    }

    fn crit(tags: &[i64], text: &str, days: Option<u32>) -> Criteria {
        Criteria {
            tags: tags.to_vec(),
            text: text.into(),
            modified_days: days,
            folder: None,
        }
    }

    fn names(found: &[Found]) -> Vec<String> {
        let mut n: Vec<String> = found.iter().map(|(_, e, _)| e.name.clone()).collect();
        n.sort();
        n
    }

    #[test]
    fn saves_lists_renames_and_deletes() {
        let tags = Tags::open(None).unwrap();
        let a = tags
            .smart_save(None, " Work this week ", &crit(&[1], "", Some(7)))
            .unwrap();
        assert_eq!(a.name, "Work this week");
        let b = tags.smart_save(None, "Big", &crit(&[2], "", None)).unwrap();
        let listed: Vec<String> = tags.smart_list().into_iter().map(|s| s.name).collect();
        assert_eq!(listed, ["Big", "Work this week"]);

        tags.smart_save(Some(a.id), "Work lately", &a.criteria)
            .unwrap();
        assert_eq!(tags.smart_get(a.id).unwrap().name, "Work lately");
        tags.smart_delete(b.id).unwrap();
        assert_eq!(tags.smart_list().len(), 1);
        assert!(tags.smart_get(b.id).is_err());
    }

    #[test]
    fn refuses_bad_names_and_criteria() {
        let tags = Tags::open(None).unwrap();
        let ok = crit(&[1], "", None);
        assert!(tags.smart_save(None, "  ", &ok).is_err());
        assert!(
            tags.smart_save(None, &"x".repeat(MAX_NAME + 1), &ok)
                .is_err()
        );
        assert!(
            tags.smart_save(None, "a", &Criteria::default()).is_err(),
            "nothing to look for"
        );
        assert!(
            tags.smart_save(None, "a", &crit(&[], "x", None)).is_err(),
            "a walk needs a folder"
        );
        assert!(
            tags.smart_save(None, "a", &crit(&[1], "", Some(0)))
                .is_err()
        );
        tags.smart_save(None, "Dup", &ok).unwrap();
        let err = tags.smart_save(None, "dup", &ok).unwrap_err();
        assert!(err.contains("already"), "{err}");
        assert!(
            tags.smart_save(Some(999), "Other", &ok).is_err(),
            "no such folder to change"
        );
    }

    #[test]
    fn needs_every_tag_and_follows_changes() {
        let tmp = tempfile::tempdir().unwrap();
        let both = file(tmp.path(), "both.txt");
        let one = file(tmp.path(), "one.txt");
        let tags = Tags::open(None).unwrap();
        tags.toggle(&[both.clone(), one.clone()], 1).unwrap();
        tags.toggle(std::slice::from_ref(&both), 2).unwrap();

        let c = crit(&[1, 2], "", None);
        assert_eq!(
            names(&tags.smart_tagged(&c, now_ms()).unwrap()),
            ["both.txt"]
        );
        assert_eq!(
            names(&tags.smart_tagged(&crit(&[1], "", None), now_ms()).unwrap()),
            ["both.txt", "one.txt"]
        );

        // Live: the answer is worked out at each opening, so a change shows at once.
        tags.toggle(std::slice::from_ref(&one), 2).unwrap();
        assert_eq!(
            names(&tags.smart_tagged(&c, now_ms()).unwrap()),
            ["both.txt", "one.txt"]
        );
        tags.toggle(std::slice::from_ref(&both), 1).unwrap();
        assert_eq!(
            names(&tags.smart_tagged(&c, now_ms()).unwrap()),
            ["one.txt"]
        );
        fs::remove_file(&one).unwrap();
        assert_eq!(
            names(&tags.smart_tagged(&c, now_ms()).unwrap()),
            <Vec<String>>::new()
        );
    }

    #[test]
    fn a_time_window_moves_with_now() {
        let tmp = tempfile::tempdir().unwrap();
        let recent = file(tmp.path(), "recent.txt");
        let old = file(tmp.path(), "old.txt");
        age(&old, 40);
        let tags = Tags::open(None).unwrap();
        tags.toggle(&[recent, old.clone()], 1).unwrap();

        let week = crit(&[1], "", Some(7));
        assert_eq!(
            names(&tags.smart_tagged(&week, now_ms()).unwrap()),
            ["recent.txt"]
        );
        let year = crit(&[1], "", Some(365));
        assert_eq!(
            names(&tags.smart_tagged(&year, now_ms()).unwrap()),
            ["old.txt", "recent.txt"]
        );
        // The same folder, opened 60 days later, no longer has the recent file.
        let later = now_ms() + 60 * DAY_MS;
        assert_eq!(
            names(&tags.smart_tagged(&week, later).unwrap()),
            <Vec<String>>::new()
        );
        age(&old, 1);
        assert_eq!(
            names(&tags.smart_tagged(&week, now_ms()).unwrap()),
            ["old.txt", "recent.txt"]
        );
    }

    #[test]
    fn narrows_by_name_and_folder() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir(tmp.path().join("a")).unwrap();
        fs::create_dir(tmp.path().join("ab")).unwrap();
        let in_a = file(&tmp.path().join("a"), "report.txt");
        let in_ab = file(&tmp.path().join("ab"), "report.txt");
        let other = file(&tmp.path().join("a"), "notes.md");
        let tags = Tags::open(None).unwrap();
        tags.toggle(&[in_a.clone(), in_ab, other], 1).unwrap();

        let by_name = crit(&[1], "*.md", None);
        assert_eq!(
            names(&tags.smart_tagged(&by_name, now_ms()).unwrap()),
            ["notes.md"]
        );

        let mut in_folder = crit(&[1], "report", None);
        in_folder.folder = Some(tmp.path().join("a").to_string_lossy().into_owned());
        let found = tags.smart_tagged(&in_folder, now_ms()).unwrap();
        assert_eq!(found.len(), 1, "a folder called `ab` is not inside `a`");
        assert_eq!(found[0].0, in_a);
    }

    #[test]
    fn a_deleted_tag_fails_instead_of_widening() {
        let tmp = tempfile::tempdir().unwrap();
        let f = file(tmp.path(), "x.txt");
        let tags = Tags::open(None).unwrap();
        let named = tags.create("Project", None).unwrap();
        tags.toggle(std::slice::from_ref(&f), named.id).unwrap();
        let c = crit(&[named.id], "", None);
        assert_eq!(tags.smart_tagged(&c, now_ms()).unwrap().len(), 1);
        tags.delete(named.id).unwrap();
        let err = tags.smart_tagged(&c, now_ms()).unwrap_err();
        assert!(err.contains("deleted"), "{err}");
    }

    #[test]
    fn a_new_tag_taking_a_deleted_tags_number_is_not_matched() {
        let tmp = tempfile::tempdir().unwrap();
        let f = file(tmp.path(), "x.txt");
        let tags = Tags::open(None).unwrap();
        let old = tags.create("Old", None).unwrap();
        let saved = tags
            .smart_save(None, "Uses old", &crit(&[old.id], "", None))
            .unwrap();
        tags.delete(old.id).unwrap();
        // A table that gives out the highest number again hands the new tag the old one's.
        let new = tags.create("New", None).unwrap();
        assert_eq!(new.id, old.id, "the number is reused");
        tags.toggle(std::slice::from_ref(&f), new.id).unwrap();
        let criteria = tags.smart_get(saved.id).unwrap().criteria;
        let err = tags.smart_tagged(&criteria, now_ms()).unwrap_err();
        assert!(err.contains("deleted"), "{err}");
        // And a folder can't be saved with a tag that is not there.
        assert!(
            tags.smart_save(Some(saved.id), "Uses old", &criteria)
                .is_err()
        );
    }

    #[test]
    fn refuses_a_tag_that_is_not_there_and_a_folder_that_is_not_absolute() {
        let tags = Tags::open(None).unwrap();
        assert!(
            tags.smart_save(None, "a", &crit(&[9999], "", None))
                .is_err()
        );
        let with_folder = |folder: &str| Criteria {
            folder: Some(folder.into()),
            ..crit(&[1], "", None)
        };
        for bad in ["Documents", "C:foo", "\\foo", "folio:recycle-bin", "../x"] {
            assert!(
                tags.smart_save(None, "a", &with_folder(bad)).is_err(),
                "{bad} is not a full path"
            );
        }
        let full = std::env::temp_dir().to_string_lossy().into_owned();
        assert!(tags.smart_save(None, "a", &with_folder(&full)).is_ok());
    }

    #[test]
    fn a_stopped_walk_says_so_instead_of_passing_for_complete() {
        let tmp = tempfile::tempdir().unwrap();
        file(tmp.path(), "a.psd");
        let mut c = crit(&[], "*.psd", None);
        c.folder = Some(tmp.path().to_string_lossy().into_owned());
        let err = walk(&c, now_ms(), &AtomicBool::new(true)).unwrap_err();
        assert_eq!(err, SUPERSEDED);
    }

    #[test]
    fn walks_a_folder_when_no_tag_is_named() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir(tmp.path().join("sub")).unwrap();
        file(tmp.path(), "a.psd");
        let old = file(&tmp.path().join("sub"), "b.psd");
        file(&tmp.path().join("sub"), "c.txt");
        age(&old, 30);

        let mut c = crit(&[], "*.psd", Some(7));
        c.folder = Some(tmp.path().to_string_lossy().into_owned());
        let found = walk(&c, now_ms(), &AtomicBool::new(false)).unwrap();
        assert_eq!(names(&found), ["a.psd"]);
        c.modified_days = Some(90);
        let found = walk(&c, now_ms(), &AtomicBool::new(false)).unwrap();
        assert_eq!(names(&found), ["a.psd", "b.psd"]);
        assert!(walk(&crit(&[], "x", None), now_ms(), &AtomicBool::new(false)).is_err());
    }
}
