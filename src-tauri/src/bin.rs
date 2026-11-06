//! The Recycle Bin / Trash as the sidebar shows it: what is in it, restoring items, and
//! deleting them for good. Permanent deletion is only ever what the user asks for here,
//! after a confirmation in the UI (ADR 0003); nothing else in Folio uses it.

use serde::{Deserialize, Serialize};

/// How many items are deleted between two progress reports.
const PURGE_BATCH: usize = 20;

/// One item in the bin.
#[derive(Serialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BinItem {
    /// The system's identifier of the item, to pass back to restore or delete it.
    pub id: String,
    pub name: String,
    /// Where it was before it was deleted.
    pub original: String,
    /// When it was deleted, in seconds since 1970.
    pub deleted: i64,
}

/// An item as it was listed: all three must still match for it to be acted on.
#[derive(Deserialize, Debug)]
#[cfg_attr(
    target_os = "macos",
    expect(
        dead_code,
        reason = "the macOS stub of the bin does not act on items yet"
    )
)]
pub struct BinRef {
    pub id: String,
    pub original: String,
    pub deleted: i64,
}

/// What an item weighs, read apart from the listing because the system is slow to say.
#[derive(Serialize, Debug, PartialEq, Eq)]
pub struct BinSize {
    pub id: String,
    /// Bytes for a file; `None` for a folder or when the system can't say.
    pub size: Option<u64>,
    /// How many entries a folder holds.
    pub entries: Option<usize>,
}

#[cfg(any(
    target_os = "windows",
    all(
        unix,
        not(target_os = "macos"),
        not(target_os = "ios"),
        not(target_os = "android")
    )
))]
mod system {
    use super::{BinItem, BinRef, BinSize};
    use trash::os_limited;
    use trash::{TrashItem, TrashItemSize};

    fn describe(item: &TrashItem) -> BinItem {
        BinItem {
            id: item.id.to_string_lossy().into_owned(),
            name: item.name.to_string_lossy().into_owned(),
            original: item.original_path().to_string_lossy().into_owned(),
            deleted: item.time_deleted,
        }
    }

    pub fn sizes() -> Result<Vec<BinSize>, String> {
        Ok(os_limited::list()
            .map_err(|e| e.to_string())?
            .iter()
            .map(|item| {
                let (size, entries) = match os_limited::metadata(item).map(|m| m.size) {
                    Ok(TrashItemSize::Bytes(b)) => (Some(b), None),
                    Ok(TrashItemSize::Entries(n)) => (None, Some(n)),
                    Err(_) => (None, None),
                };
                BinSize {
                    id: item.id.to_string_lossy().into_owned(),
                    size,
                    entries,
                }
            })
            .collect())
    }

    pub fn list() -> Result<Vec<BinItem>, String> {
        let mut items: Vec<BinItem> = os_limited::list()
            .map_err(|e| e.to_string())?
            .iter()
            .map(describe)
            .collect();
        // Latest first.
        items.sort_by_key(|i| std::cmp::Reverse(i.deleted));
        Ok(items)
    }

    /// The listed items that `refs` name. Each must match exactly (id, original place and
    /// time of deletion), so an id the system has since given to another item never picks
    /// it; if any is missing or changed, nothing is picked.
    fn pick(refs: &[BinRef]) -> Result<Vec<TrashItem>, String> {
        let listed = os_limited::list().map_err(|e| e.to_string())?;
        let mut picked = Vec::new();
        for r in refs {
            let item = listed.iter().find(|i| {
                i.id.to_string_lossy() == r.id
                    && i.original_path().to_string_lossy() == r.original
                    && i.time_deleted == r.deleted
            });
            match item {
                Some(item) => picked.push(item.clone()),
                None => return Err("The Recycle Bin has changed since it was listed".into()),
            }
        }
        Ok(picked)
    }

    /// Puts the items back where they were, one at a time. Nothing is overwritten or merged
    /// into: an item whose place is taken (by anything, a broken link included) stays in the
    /// bin, and of items deleted from the same place only the newest comes back.
    pub fn restore(refs: &[BinRef]) -> Result<(), String> {
        let mut items = pick(refs)?;
        items.sort_by_key(|i| std::cmp::Reverse(i.time_deleted));
        let mut blocked: Vec<String> = Vec::new();
        let mut restored: Vec<std::path::PathBuf> = Vec::new();
        for item in items {
            let place = item.original_path();
            // Checked just before each restore; a link counts as something being there.
            if restored.contains(&place) || std::fs::symlink_metadata(&place).is_ok() {
                blocked.push(item.name.to_string_lossy().into_owned());
                continue;
            }
            os_limited::restore_all([item]).map_err(|e| e.to_string())?;
            restored.push(place);
        }
        if blocked.is_empty() {
            return Ok(());
        }
        blocked.truncate(5);
        Err(format!(
            "Not restored, because something is already in their place: {}",
            blocked.join(", ")
        ))
    }

    /// Deletes the items for good, a few at a time, saying how many are gone after each
    /// batch. Stops at the first failure; what was already deleted stays deleted.
    pub fn purge(refs: &[BinRef], progress: &mut dyn FnMut(usize)) -> Result<(), String> {
        let items = pick(refs)?;
        let mut done = 0;
        for batch in items.chunks(super::PURGE_BATCH) {
            os_limited::purge_all(batch.to_vec()).map_err(|e| {
                format!(
                    "{e}. {done} of {} items were deleted before this.",
                    refs.len()
                )
            })?;
            done += batch.len();
            progress(done);
        }
        Ok(())
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
mod system {
    use super::{BinItem, BinRef, BinSize};

    const NO: &str = "The Trash can't be listed on this system yet";

    pub fn list() -> Result<Vec<BinItem>, String> {
        Err(NO.into())
    }

    pub fn sizes() -> Result<Vec<BinSize>, String> {
        Err(NO.into())
    }

    pub fn restore(_refs: &[BinRef]) -> Result<(), String> {
        Err(NO.into())
    }

    pub fn purge(_refs: &[BinRef], _progress: &mut dyn FnMut(usize)) -> Result<(), String> {
        Err(NO.into())
    }
}

pub use system::{list, purge, restore, sizes};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_is_not_an_item_in_the_bin() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("keep.txt");
        std::fs::write(&file, "x").unwrap();
        let forged = BinRef {
            id: file.to_string_lossy().into_owned(),
            original: file.to_string_lossy().into_owned(),
            deleted: 0,
        };
        assert!(purge(&[forged], &mut |_| {}).is_err());
        assert!(file.exists());
    }

    /// Puts a file in the real Recycle Bin: `cargo test -- --ignored a_taken_place`.
    #[test]
    #[ignore = "uses the real Recycle Bin"]
    #[cfg(windows)]
    fn a_taken_place_keeps_the_item_in_the_bin() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("folio-bin-unit-test.txt");
        std::fs::write(&file, "gone").unwrap();
        crate::ops::trash(std::slice::from_ref(&file)).unwrap();
        // Something else takes the place.
        std::fs::write(&file, "mine").unwrap();
        let item = list()
            .unwrap()
            .into_iter()
            .find(|i| i.original == file.to_string_lossy())
            .expect("in the bin");
        let r = BinRef {
            id: item.id.clone(),
            original: item.original.clone(),
            deleted: item.deleted,
        };
        assert!(restore(std::slice::from_ref(&r)).is_err());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "mine");
        // Tidy up: this one item goes for good.
        purge(&[r], &mut |_| {}).unwrap();
    }
}
