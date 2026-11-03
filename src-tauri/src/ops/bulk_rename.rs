//! Renaming a batch of items in one step. The whole batch is checked before anything
//! changes, and one that can't finish is rolled back, so it never ends half done.

use super::{rename, validate_name};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};

/// One item of a batch and the name it should get. An item that keeps its name is still
/// listed, so the others can't take that name.
pub struct Planned {
    pub path: PathBuf,
    pub new_name: String,
}

/// Case-folded text of a path, to compare names the way Windows and macOS volumes do.
/// On a case-sensitive volume this flags two names that differ only in case, which is
/// the safe way to be wrong.
fn key(path: &Path) -> String {
    path.to_string_lossy().to_lowercase()
}

fn name_of(path: &Path) -> String {
    path.file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
}

fn target_of(item: &Planned) -> PathBuf {
    item.path.parent().map_or_else(
        || PathBuf::from(&item.new_name),
        |dir| dir.join(&item.new_name),
    )
}

fn changes(item: &Planned) -> bool {
    item.new_name != name_of(&item.path)
}

/// Why each rename can't be done, in the order of `plan`; `None` where it can, or where
/// the name stays the same.
pub fn check(plan: &[Planned]) -> Vec<Option<String>> {
    // Looked up per item below, so built once: how many items get each name in each
    // folder, and which paths of the batch are being renamed away.
    let mut wanted: HashMap<(Option<&Path>, String), usize> = HashMap::new();
    let mut leaving: HashMap<String, bool> = HashMap::new();
    for item in plan {
        *wanted
            .entry((item.path.parent(), item.new_name.to_lowercase()))
            .or_default() += 1;
        leaving.insert(key(&item.path), changes(item));
    }
    plan.iter()
        .map(|item| {
            if !changes(item) {
                return None;
            }
            if let Err(e) = validate_name(&item.new_name) {
                return Some(e.to_string());
            }
            if wanted[&(item.path.parent(), item.new_name.to_lowercase())] > 1 {
                return Some("Another item in the batch gets the same name".into());
            }
            let target = target_of(item);
            if std::fs::symlink_metadata(&target).is_err() || key(&target) == key(&item.path) {
                return None;
            }
            // Taken: fine only when the item holding the name is itself being renamed away.
            let leaves = leaving.get(&key(&target)).copied().unwrap_or(false);
            (!leaves).then(|| format!("\"{}\" already exists", item.new_name))
        })
        .collect()
}

/// Renames every changed item of `plan`. Returns the renames that stand afterwards, in
/// the order they were made (for the undo journal), and the error if the batch failed.
/// A failure undoes what was done; only a rename that can't be undone stays listed, and
/// the error then says where those items are.
pub fn apply(plan: &[Planned]) -> (Vec<(PathBuf, PathBuf)>, Result<(), String>) {
    if let Some((item, why)) = plan
        .iter()
        .zip(check(plan))
        .find_map(|(item, why)| why.map(|w| (item, w)))
    {
        return (
            Vec::new(),
            Err(format!("\"{}\": {why}", name_of(&item.path))),
        );
    }
    let todo: Vec<&Planned> = plan.iter().filter(|p| changes(p)).collect();
    let mut done: Vec<(PathBuf, PathBuf)> = Vec::new();
    if let Err(e) = rename_in_order(&todo, &mut done) {
        roll_back(&mut done);
        let left = if done.is_empty() {
            String::new()
        } else {
            let names: Vec<String> = done
                .iter()
                .map(|(_, to)| format!("\"{}\"", to.display()))
                .collect();
            format!(
                ". Some items could not be put back and are now named {}",
                names.join(", ")
            )
        };
        return (done, Err(format!("{e}{left}")));
    }
    (done, Ok(()))
}

/// Renames the items so that none needs a name before the item holding it has left: a
/// shift along a numbered series just runs from the far end. Only items on a cycle (a
/// swap) are parked under a temporary name for a moment, which keeps the window in which
/// a crash could leave an item under a name that isn't its own as small as possible.
fn rename_in_order(todo: &[&Planned], done: &mut Vec<(PathBuf, PathBuf)>) -> Result<(), String> {
    let holder: HashMap<String, usize> = todo
        .iter()
        .enumerate()
        .map(|(i, p)| (key(&p.path), i))
        .collect();
    // `waiting_for[i]`: the item that holds the name `i` is to get, if it is in the batch.
    let mut waiters: Vec<Vec<usize>> = vec![Vec::new(); todo.len()];
    let mut ready: VecDeque<usize> = VecDeque::new();
    for (i, p) in todo.iter().enumerate() {
        match holder.get(&key(&target_of(p))).copied().filter(|&j| j != i) {
            Some(j) => waiters[j].push(i),
            None => ready.push_back(i),
        }
    }
    let mut at: Vec<PathBuf> = todo.iter().map(|p| p.path.clone()).collect();
    let mut finished = vec![false; todo.len()];
    let mut parked = vec![false; todo.len()];
    let mut left = todo.len();
    while left > 0 {
        while let Some(i) = ready.pop_front() {
            step(&at[i], &todo[i].new_name, done).map_err(|e| e.to_string())?;
            finished[i] = true;
            left -= 1;
            ready.extend(std::mem::take(&mut waiters[i]));
        }
        if left == 0 {
            break;
        }
        // Everything left waits on another: a cycle. Park one item to open it.
        let Some(i) = (0..todo.len()).find(|&i| !finished[i] && !parked[i]) else {
            return Err("The names could not be put in order".into());
        };
        let temp = parked_name(&todo[i].path, i);
        step(&at[i], &temp, done).map_err(|e| e.to_string())?;
        at[i] = at[i]
            .parent()
            .map_or_else(|| PathBuf::from(&temp), |d| d.join(&temp));
        parked[i] = true;
        ready.extend(std::mem::take(&mut waiters[i]));
    }
    Ok(())
}

/// The temporary name of item `i`, built from its own name so it can be recognised if it
/// is ever left behind, unless that would pass the usual 255-character name limit.
fn parked_name(path: &Path, i: usize) -> String {
    let suffix = format!(".folio-bulk-{i}");
    // Room for the leading dot and the " (2)" a taken name gets.
    let room = 240 - suffix.len();
    let mut name = name_of(path);
    while name.len() > room {
        name.pop();
    }
    format!(".{name}{suffix}")
}

fn step(from: &Path, name: &str, done: &mut Vec<(PathBuf, PathBuf)>) -> std::io::Result<()> {
    let to = rename(from, name)?;
    done.push((from.to_path_buf(), to));
    Ok(())
}

/// Puts back, newest first, what `done` lists; whatever can't be put back stays listed.
fn roll_back(done: &mut Vec<(PathBuf, PathBuf)>) {
    let mut stuck = Vec::new();
    while let Some((from, to)) = done.pop() {
        if rename(&to, &name_of(&from)).is_err() {
            stuck.push((from, to));
        }
    }
    stuck.reverse();
    *done = stuck;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmp() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    fn file(dir: &Path, name: &str, body: &str) -> PathBuf {
        let p = dir.join(name);
        fs::write(&p, body).unwrap();
        p
    }

    fn plan(items: &[(&PathBuf, &str)]) -> Vec<Planned> {
        items
            .iter()
            .map(|(p, n)| Planned {
                path: (*p).clone(),
                new_name: (*n).to_string(),
            })
            .collect()
    }

    fn read(dir: &Path, name: &str) -> String {
        fs::read_to_string(dir.join(name)).unwrap()
    }

    fn undo(done: &[(PathBuf, PathBuf)]) {
        for (from, to) in done.iter().rev() {
            rename(to, &name_of(from)).unwrap();
        }
    }

    #[test]
    fn check_flags_name_taken_by_item_outside_the_batch() {
        let dir = tmp();
        let a = file(dir.path(), "a.txt", "a");
        file(dir.path(), "taken.txt", "t");
        let result = check(&plan(&[(&a, "taken.txt")]));
        assert!(result[0].as_ref().unwrap().contains("already exists"));
    }

    #[test]
    fn check_flags_name_taken_by_unchanged_batch_item() {
        let dir = tmp();
        let a = file(dir.path(), "a.txt", "a");
        let b = file(dir.path(), "b.txt", "b");
        let result = check(&plan(&[(&a, "b.txt"), (&b, "b.txt")]));
        assert!(result[0].is_some());
        assert!(result[1].is_none());
    }

    #[test]
    fn check_flags_two_items_getting_the_same_name() {
        let dir = tmp();
        let a = file(dir.path(), "a.txt", "a");
        let b = file(dir.path(), "b.txt", "b");
        let result = check(&plan(&[(&a, "same.txt"), (&b, "same.txt")]));
        assert!(result.iter().all(Option::is_some));
    }

    #[test]
    fn check_flags_names_that_differ_only_in_case() {
        let dir = tmp();
        let a = file(dir.path(), "a.txt", "a");
        let b = file(dir.path(), "b.txt", "b");
        let result = check(&plan(&[(&a, "Same.txt"), (&b, "same.TXT")]));
        assert!(result.iter().all(Option::is_some));
    }

    #[test]
    fn check_flags_invalid_names() {
        let dir = tmp();
        let a = file(dir.path(), "a.txt", "a");
        for bad in ["", "x/y"] {
            let result = check(&plan(&[(&a, bad)]));
            assert!(result[0].is_some(), "{bad:?} should be flagged");
        }
        // Only Windows refuses these.
        #[cfg(windows)]
        for bad in ["x\\y", "trailing.", "bad:name"] {
            let result = check(&plan(&[(&a, bad)]));
            assert!(result[0].is_some(), "{bad:?} should be flagged");
        }
    }

    #[test]
    fn check_returns_none_for_unchanged_name() {
        let dir = tmp();
        let a = file(dir.path(), "a.txt", "a");
        assert_eq!(check(&plan(&[(&a, "a.txt")])), vec![None]);
    }

    #[test]
    fn check_accepts_non_ascii_names() {
        let dir = tmp();
        let a = file(dir.path(), "a.txt", "a");
        assert_eq!(check(&plan(&[(&a, "résumé 日本語.txt")])), vec![None]);
    }

    #[test]
    fn check_accepts_swap_and_shift_within_batch() {
        let dir = tmp();
        let a = file(dir.path(), "a.txt", "a");
        let b = file(dir.path(), "b.txt", "b");
        let c = file(dir.path(), "c.txt", "c");
        let result = check(&plan(&[(&a, "b.txt"), (&b, "c.txt"), (&c, "d.txt")]));
        assert!(result.iter().all(Option::is_none));
    }

    #[test]
    fn check_allows_case_only_change_of_the_same_item() {
        let dir = tmp();
        let a = file(dir.path(), "a.txt", "a");
        assert_eq!(check(&plan(&[(&a, "A.txt")])), vec![None]);
    }

    #[test]
    fn apply_shifts_a_chain_and_keeps_contents_in_place() {
        let dir = tmp();
        let a = file(dir.path(), "a", "A");
        let b = file(dir.path(), "b", "B");
        let (done, result) = apply(&plan(&[(&a, "b"), (&b, "c")]));
        result.unwrap();
        assert_ne!(done.len(), 0);
        assert_eq!(read(dir.path(), "b"), "A");
        assert_eq!(read(dir.path(), "c"), "B");
        assert!(!dir.path().join("a").exists());
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
    }

    #[test]
    fn apply_swaps_two_items() {
        let dir = tmp();
        let a = file(dir.path(), "a.txt", "A");
        let b = file(dir.path(), "b.txt", "B");
        let (_, result) = apply(&plan(&[(&a, "b.txt"), (&b, "a.txt")]));
        result.unwrap();
        assert_eq!(read(dir.path(), "a.txt"), "B");
        assert_eq!(read(dir.path(), "b.txt"), "A");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
    }

    #[test]
    fn apply_renames_folders_to_non_ascii_names() {
        let dir = tmp();
        let d = dir.path().join("dir");
        fs::create_dir(&d).unwrap();
        fs::write(d.join("inner"), "x").unwrap();
        let (_, result) = apply(&plan(&[(&d, "папка ✓")]));
        result.unwrap();
        let inner = dir.path().join("папка ✓").join("inner");
        assert_eq!(fs::read_to_string(inner).unwrap(), "x");
    }

    #[test]
    fn apply_skips_unchanged_items() {
        let dir = tmp();
        let a = file(dir.path(), "a", "A");
        let b = file(dir.path(), "b", "B");
        let (done, result) = apply(&plan(&[(&a, "a"), (&b, "z")]));
        result.unwrap();
        assert_eq!(done.len(), 1);
        assert_eq!(read(dir.path(), "a"), "A");
        assert_eq!(read(dir.path(), "z"), "B");
    }

    #[test]
    fn apply_refuses_whole_batch_when_one_item_is_invalid() {
        let dir = tmp();
        let a = file(dir.path(), "a", "A");
        let b = file(dir.path(), "b", "B");
        let (done, result) = apply(&plan(&[(&a, "good"), (&b, "bad/name")]));
        assert!(result.is_err());
        assert_eq!(done.len(), 0);
        assert_eq!(read(dir.path(), "a"), "A");
        assert_eq!(read(dir.path(), "b"), "B");
        assert!(!dir.path().join("good").exists());
    }

    #[test]
    fn apply_refuses_whole_batch_when_a_name_is_taken_outside_it() {
        let dir = tmp();
        let a = file(dir.path(), "a", "A");
        let b = file(dir.path(), "b", "B");
        file(dir.path(), "taken", "T");
        let (done, result) = apply(&plan(&[(&a, "new"), (&b, "taken")]));
        assert!(result.is_err());
        assert_eq!(done.len(), 0);
        assert_eq!(read(dir.path(), "a"), "A");
        assert_eq!(read(dir.path(), "taken"), "T");
        assert!(!dir.path().join("new").exists());
    }

    #[test]
    fn apply_rolls_back_when_a_rename_fails_part_way() {
        // A path that does not exist passes `check` (nothing holds its target) but its
        // rename fails, after the first item was already renamed.
        let dir = tmp();
        let a = file(dir.path(), "a", "A");
        let ghost = dir.path().join("ghost");
        let (done, result) = apply(&plan(&[(&a, "a2"), (&ghost, "g2")]));
        assert!(result.is_err());
        assert!(done.is_empty(), "everything should have been undone");
        assert_eq!(read(dir.path(), "a"), "A");
        assert!(!dir.path().join("a2").exists());
    }

    #[test]
    fn apply_rolls_back_a_two_step_batch_when_a_step_fails() {
        let dir = tmp();
        let a = file(dir.path(), "a", "A");
        let b = file(dir.path(), "b", "B");
        let ghost = dir.path().join("ghost");
        // The a<->b swap takes the two-step path; the ghost fails in its first pass.
        let (done, result) = apply(&plan(&[(&a, "b"), (&b, "a"), (&ghost, "g2")]));
        assert!(result.is_err());
        assert_eq!(done.len(), 0);
        assert_eq!(read(dir.path(), "a"), "A");
        assert_eq!(read(dir.path(), "b"), "B");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
    }

    #[test]
    fn roll_back_restores_newest_first_and_empties_the_list() {
        let dir = tmp();
        let a = file(dir.path(), "a", "A");
        let mut done = Vec::new();
        step(&a, "b", &mut done).unwrap();
        step(&dir.path().join("b"), "c", &mut done).unwrap();
        roll_back(&mut done);
        assert_eq!(done.len(), 0);
        assert_eq!(read(dir.path(), "a"), "A");
    }

    #[test]
    fn roll_back_keeps_listed_what_cannot_be_put_back() {
        let dir = tmp();
        let a = file(dir.path(), "a", "A");
        let mut done = Vec::new();
        step(&a, "b", &mut done).unwrap();
        file(dir.path(), "a", "squatter");
        roll_back(&mut done);
        assert_eq!(done.len(), 1);
        assert_eq!(read(dir.path(), "b"), "A");
        assert_eq!(read(dir.path(), "a"), "squatter");
    }

    #[test]
    fn returned_renames_of_a_swap_undo_in_reverse() {
        let dir = tmp();
        let a = file(dir.path(), "a", "A");
        let b = file(dir.path(), "b", "B");
        let c = file(dir.path(), "c", "C");
        let (done, result) = apply(&plan(&[(&a, "b"), (&b, "a"), (&c, "d")]));
        result.unwrap();
        assert_eq!(read(dir.path(), "a"), "B");
        undo(&done);
        assert_eq!(read(dir.path(), "a"), "A");
        assert_eq!(read(dir.path(), "b"), "B");
        assert_eq!(read(dir.path(), "c"), "C");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 3);
    }

    #[test]
    fn returned_renames_of_a_simple_batch_undo_in_reverse() {
        let dir = tmp();
        let a = file(dir.path(), "a", "A");
        let b = file(dir.path(), "b", "B");
        let (done, result) = apply(&plan(&[(&a, "x"), (&b, "y")]));
        result.unwrap();
        assert_eq!(done.len(), 2);
        undo(&done);
        assert_eq!(read(dir.path(), "a"), "A");
        assert_eq!(read(dir.path(), "b"), "B");
    }

    #[test]
    fn a_shift_needs_no_temporary_names() {
        let dir = tmp();
        let a = file(dir.path(), "a", "A");
        let b = file(dir.path(), "b", "B");
        let (done, result) = apply(&plan(&[(&a, "b"), (&b, "c")]));
        result.unwrap();
        // b goes first, then a: two renames, nothing parked.
        assert_eq!(done.len(), 2);
        assert_eq!(fs::read_to_string(dir.path().join("c")).unwrap(), "B");
        assert_eq!(fs::read_to_string(dir.path().join("b")).unwrap(), "A");
    }

    #[test]
    fn a_three_way_cycle_is_renamed_and_undone() {
        let dir = tmp();
        let a = file(dir.path(), "a", "A");
        let b = file(dir.path(), "b", "B");
        let c = file(dir.path(), "c", "C");
        let (done, result) = apply(&plan(&[(&a, "b"), (&b, "c"), (&c, "a")]));
        result.unwrap();
        assert_eq!(fs::read_to_string(dir.path().join("b")).unwrap(), "A");
        assert_eq!(fs::read_to_string(dir.path().join("c")).unwrap(), "B");
        assert_eq!(fs::read_to_string(dir.path().join("a")).unwrap(), "C");
        undo(&done);
        assert_eq!(fs::read_to_string(&a).unwrap(), "A");
        assert_eq!(fs::read_to_string(&b).unwrap(), "B");
        assert_eq!(fs::read_to_string(&c).unwrap(), "C");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 3);
    }
}
