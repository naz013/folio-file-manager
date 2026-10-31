//! Name collisions at the top level of a copy or move: an item in `sources` whose name is
//! already taken in the destination folder. The UI asks what to do about each one before
//! the operation starts; "replace" sends the existing item to the Recycle Bin (ADR 0003)
//! and never overwrites it.

use serde::Serialize;
use std::fs;
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};

use super::{comparable, noclobber};

/// An item and the existing one with its name, for the UI to compare.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Conflict {
    pub source: PathBuf,
    pub existing: PathBuf,
    pub name: String,
    pub source_is_dir: bool,
    pub existing_is_dir: bool,
    /// Size in bytes; 0 for a folder.
    pub source_size: u64,
    pub existing_size: u64,
    /// Milliseconds since the Unix epoch.
    pub source_modified: Option<i64>,
    pub existing_modified: Option<i64>,
    /// Identity of the existing item as shown, so a replace can refuse if another item has
    /// taken its place by the time the operation runs.
    pub existing_id: Option<String>,
    /// Whether the existing item may be replaced. Not when it is the item itself, holds
    /// it, or holds the destination: sending it to the Recycle Bin would take those too.
    pub can_replace: bool,
}

/// The items of `sources` that collide with a name in `dest`. Items already in `dest`
/// don't collide with themselves: copying them there makes a numbered copy, and moving
/// them is a no-op.
pub fn find(sources: &[PathBuf], dest: &Path) -> Vec<Conflict> {
    sources
        .iter()
        .filter_map(|source| conflict(source, dest, sources))
        .collect()
}

fn conflict(source: &Path, dest: &Path, all: &[PathBuf]) -> Option<Conflict> {
    let name = source.file_name()?;
    let from_dest = source
        .parent()
        .is_some_and(|p| p == dest || noclobber::same_file(p, dest).unwrap_or(false));
    if from_dest {
        return None;
    }
    let existing = dest.join(name);
    let existing_meta = fs::symlink_metadata(&existing).ok()?;
    let source_meta = fs::symlink_metadata(source).ok()?;
    let millis = |m: &fs::Metadata| {
        m.modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .and_then(|d| i64::try_from(d.as_millis()).ok())
    };
    Some(Conflict {
        can_replace: can_replace(source, &existing, dest, all),
        existing_id: super::identity(&existing),
        source: source.to_path_buf(),
        name: name.to_string_lossy().into_owned(),
        source_is_dir: source_meta.is_dir(),
        existing_is_dir: existing_meta.is_dir(),
        source_size: if source_meta.is_dir() {
            0
        } else {
            source_meta.len()
        },
        existing_size: if existing_meta.is_dir() {
            0
        } else {
            existing_meta.len()
        },
        source_modified: millis(&source_meta),
        existing_modified: millis(&existing_meta),
        existing,
    })
}

/// Whether the Recycle Bin can take things from the drive of `path`. A network drive has
/// none, and what is trashed there may be deleted for good.
#[cfg(windows)]
fn has_recycle_bin(path: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Storage::FileSystem::GetDriveTypeW;
    use windows::core::PCWSTR;
    const DRIVE_REMOTE: u32 = 4;
    let Ok(full) = std::path::absolute(path) else {
        return false;
    };
    let mut root: Vec<u16> = full.as_os_str().encode_wide().collect();
    // "C:\" or "\\server\share\": the volume root the call wants.
    let keep = if root.get(1) == Some(&u16::from(b':')) {
        3
    } else {
        // A network path has no drive letter.
        return false;
    };
    root.truncate(keep);
    root.push(0);
    // SAFETY: `root` is NUL-terminated and outlives the call.
    unsafe { GetDriveTypeW(PCWSTR(root.as_ptr())) != DRIVE_REMOTE }
}

#[cfg(not(windows))]
fn has_recycle_bin(_path: &Path) -> bool {
    true
}

fn can_replace(source: &Path, existing: &Path, dest: &Path, all: &[PathBuf]) -> bool {
    // Without identities the existing item can't be told apart from the source, and
    // without a bin it would not be recoverable.
    if super::identity(source).is_none() || super::identity(existing).is_none() {
        return false;
    }
    if !has_recycle_bin(existing) {
        return false;
    }
    if noclobber::same_file(source, existing).unwrap_or(true) {
        return false;
    }
    let (Ok(existing), Ok(source), Ok(dest)) =
        (comparable(existing), comparable(source), comparable(dest))
    else {
        return false;
    };
    // Resolving links too: a folder reached through a link is still the same folder.
    let real = |p: &Path| fs::canonicalize(p).ok();
    let holds = |inner: &Path, outer: &Path| {
        inner.starts_with(outer)
            || matches!((real(inner), real(outer)), (Some(i), Some(o)) if i.starts_with(&o))
    };
    // The existing item must not be, or hold, any of the items being copied or moved.
    let holds_another = all
        .iter()
        .filter_map(|other| comparable(other).ok())
        .any(|other| holds(&other, &existing));
    !holds(&source, &existing) && !holds(&dest, &existing) && !holds_another
}

/// The existing items to send to the Recycle Bin for the sources the user chose to
/// `replace`, each with the identity of the item they were shown (if any). Every one
/// must be a real collision that may be replaced, and still the item that was shown.
pub fn to_replace(
    sources: &[PathBuf],
    replace: &[(PathBuf, Option<String>)],
    dest: &Path,
) -> io::Result<Vec<PathBuf>> {
    let found = find(sources, dest);
    let mut doomed: Vec<PathBuf> = replace
        .iter()
        .map(|(source, shown)| {
            let c = found.iter().find(|c| &c.source == source).ok_or_else(|| {
                io::Error::new(
                    ErrorKind::InvalidInput,
                    format!("Nothing to replace for \"{}\"", source.display()),
                )
            })?;
            if shown.is_some() && c.existing_id != *shown {
                Err(io::Error::new(
                    ErrorKind::InvalidInput,
                    format!(
                        "\"{}\" changed since it was shown, so it was not replaced",
                        c.name
                    ),
                ))
            } else if c.can_replace {
                Ok(c.existing.clone())
            } else {
                Err(io::Error::new(
                    ErrorKind::InvalidInput,
                    format!("\"{}\" can't be replaced by this", c.name),
                ))
            }
        })
        .collect::<io::Result<_>>()?;
    // The same item twice (duplicate sources) is trashed once.
    doomed.sort();
    doomed.dedup();
    Ok(doomed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    fn file(path: &Path, content: &str) -> PathBuf {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
        path.to_path_buf()
    }

    #[test]
    fn a_name_taken_in_the_destination_is_a_conflict_with_both_sides() {
        let t = tmp();
        let src = file(&t.path().join("a").join("f.txt"), "new");
        file(&t.path().join("dest").join("f.txt"), "older and longer");

        let found = find(std::slice::from_ref(&src), &t.path().join("dest"));

        assert_eq!(found.len(), 1);
        let c = &found[0];
        assert_eq!((c.source_size, c.existing_size), (3, 16));
        assert_eq!(c.name, "f.txt");
        assert!(c.can_replace);
        assert!(c.source_modified.is_some() && c.existing_modified.is_some());
    }

    #[test]
    fn free_names_and_items_already_in_the_destination_are_not_conflicts() {
        let t = tmp();
        let free = file(&t.path().join("a").join("free.txt"), "x");
        let there = file(&t.path().join("dest").join("f.txt"), "x");

        assert!(find(&[free, there], &t.path().join("dest")).is_empty());
    }

    #[test]
    fn a_folder_cannot_replace_one_that_holds_it() {
        let t = tmp();
        // Moving /p/x/y/x into /p, where /p/x exists and holds the source.
        let src = file(
            &t.path().join("p").join("x").join("y").join("x").join("f"),
            "f",
        );
        let src = src.parent().unwrap().to_path_buf();

        let found = find(&[src], &t.path().join("p"));

        assert_eq!(found.len(), 1);
        assert!(
            !found[0].can_replace,
            "trashing /p/x would trash the source"
        );
    }

    #[test]
    fn an_item_cannot_replace_a_second_name_for_itself() {
        let t = tmp();
        let src = file(&t.path().join("a").join("f.txt"), "same file");
        fs::create_dir_all(t.path().join("dest")).unwrap();
        fs::hard_link(&src, t.path().join("dest").join("f.txt")).unwrap();

        let found = find(std::slice::from_ref(&src), &t.path().join("dest"));

        assert_eq!(found.len(), 1);
        assert!(!found[0].can_replace, "it would trash the source too");
    }

    #[test]
    fn to_replace_names_only_what_the_user_chose_and_refuses_the_rest() {
        let t = tmp();
        let a = file(&t.path().join("s").join("a.txt"), "a");
        let b = file(&t.path().join("s").join("b.txt"), "b");
        file(&t.path().join("dest").join("a.txt"), "old a");
        file(&t.path().join("dest").join("b.txt"), "old b");
        let dest = t.path().join("dest");

        let chosen = [(a.clone(), None)];
        let doomed = to_replace(&[a.clone(), b.clone()], &chosen, &dest).unwrap();
        assert_eq!(doomed, [dest.join("a.txt")]);

        let stranger = t.path().join("elsewhere.txt");
        assert!(to_replace(&[a, b], &[(stranger, None)], &dest).is_err());
    }

    #[test]
    fn replace_refuses_when_the_existing_item_changed_since_it_was_shown() {
        let t = tmp();
        let src = file(&t.path().join("s").join("f.txt"), "new");
        let existing = file(&t.path().join("dest").join("f.txt"), "old");
        let dest = t.path().join("dest");
        let shown = find(std::slice::from_ref(&src), &dest)[0]
            .existing_id
            .clone();
        assert!(shown.is_some());
        // Another item takes the name before the operation runs.
        fs::remove_file(&existing).unwrap();
        file(&existing, "somebody else's");

        let chosen = [(src.clone(), shown)];
        let err = to_replace(std::slice::from_ref(&src), &chosen, &dest).unwrap_err();

        assert!(
            err.to_string().contains("changed since it was shown"),
            "{err}"
        );
    }

    #[test]
    fn replace_refuses_an_existing_item_that_holds_another_source() {
        let t = tmp();
        let a = file(&t.path().join("a").join("x").join("f.txt"), "a");
        let a = a.parent().unwrap().to_path_buf();
        let inner = file(&t.path().join("dest").join("x").join("f.txt"), "inner");
        let dest = t.path().join("dest");

        // `dest/x` collides with `a/x`, and also holds the second source.
        let found = find(&[a, inner], &dest);

        let clash = found.iter().find(|c| c.name == "x").unwrap();
        assert!(!clash.can_replace);
    }

    #[test]
    fn duplicate_sources_replace_once() {
        let t = tmp();
        let src = file(&t.path().join("s").join("f.txt"), "new");
        file(&t.path().join("dest").join("f.txt"), "old");
        let dest = t.path().join("dest");
        let chosen = [(src.clone(), None), (src.clone(), None)];

        let doomed = to_replace(&[src.clone(), src], &chosen, &dest).unwrap();

        assert_eq!(doomed, [dest.join("f.txt")]);
    }
}
