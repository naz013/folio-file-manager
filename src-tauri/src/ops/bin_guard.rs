//! What a delete may hand to the OS trash. The shell deletes for good, without asking, what
//! its bin can't take: a drive with no bin (network, removable, "Don't move files to the
//! Recycle Bin") or a file or folder bigger than the room left. Those are held back and reported, so a
//! delete is never silently a permanent one (ADR 0003).

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

/// What was sent on and what was held back, and why.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Screened {
    pub go: Vec<PathBuf>,
    /// On a drive with no bin, or already in a bin.
    pub no_bin: usize,
    /// A file or folder that does not fit in the room the bin has left.
    pub no_room: usize,
}

/// Whether `path` is inside an operating system's bin: Windows' `$Recycle.Bin`, or on other
/// systems the freedesktop trash and macOS `.Trash`, `.Trashes`. (A folder called `.Trash`
/// on a Windows drive is only a folder.)
pub fn in_bin(path: &Path) -> bool {
    let parts: Vec<String> = path
        .components()
        .filter_map(|c| match c {
            Component::Normal(s) => Some(s.to_string_lossy().to_lowercase()),
            _ => None,
        })
        .collect();
    if parts.iter().any(|p| p == "$recycle.bin") {
        return true;
    }
    if cfg!(windows) {
        return false;
    }
    // Only where the systems put them: a folder of the user's own that is called .Trash
    // (or .trash-cache) is only a folder.
    let uid_trash = |p: &str| {
        p.strip_prefix(".trash-")
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
    };
    parts.iter().any(|p| p == ".trashes" || uid_trash(p))
        || parts
            .windows(3)
            .any(|w| w[0] == ".local" && w[1] == "share" && w[2] == "trash")
        // The macOS and old desktop ~/.Trash, directly in a home folder.
        || (parts.len() >= 3
            && matches!(parts[0].as_str(), "home" | "users")
            && parts[2] == ".trash")
        // A volume's own .Trash, at its top: /.Trash, or /Volumes/<name>/.Trash.
        || parts.first().is_some_and(|p| p == ".trash")
        || (parts.len() >= 3 && parts[0] == "volumes" && parts[2] == ".trash")
}

/// The size of `path`, a file or a folder's whole tree, counting no further once it passes
/// `cap` (the answer is then more than `cap`). Links are not followed and count for nothing;
/// what can't be read counts for nothing too.
fn size_up_to(path: &Path, cap: u64) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return 0;
    };
    if meta.is_file() {
        return meta.len();
    }
    if !meta.is_dir() {
        return 0;
    }
    let mut total = 0_u64;
    let mut folders = vec![path.to_path_buf()];
    while let Some(dir) = folders.pop() {
        let Ok(read) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in read.flatten() {
            let Ok(m) = std::fs::symlink_metadata(entry.path()) else {
                continue;
            };
            if m.is_file() {
                total = total.saturating_add(m.len());
                if total > cap {
                    return total;
                }
            } else if m.is_dir() {
                folders.push(entry.path());
            }
        }
    }
    total
}

/// The drive a path is on, for counting what it has used of its bin.
fn drive(path: &Path) -> String {
    path.components()
        .next()
        .map(|c| c.as_os_str().to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

/// Keeps what the bin can take. `limit` is the room the bin of that path's drive has left, or
/// `None` where it has no bin; it is asked once for each drive. Files and the whole of each
/// folder count against that room.
pub fn screen(targets: Vec<PathBuf>, limit: impl Fn(&Path) -> Option<u64>) -> Screened {
    let mut out = Screened::default();
    // What is left of each drive's room as this batch is counted (`None`: no bin there).
    let mut room: HashMap<String, Option<u64>> = HashMap::new();
    for path in targets {
        if in_bin(&path) {
            out.no_bin += 1;
            continue;
        }
        let Some(left) = room.entry(drive(&path)).or_insert_with(|| limit(&path)) else {
            out.no_bin += 1;
            continue;
        };
        let size = size_up_to(&path, *left);
        if size > *left {
            out.no_room += 1;
            continue;
        }
        *left -= size;
        out.go.push(path);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(dir: &Path, name: &str, size: usize) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, vec![0_u8; size]).unwrap();
        p
    }

    #[test]
    fn a_drive_without_a_bin_holds_everything_back() {
        let dir = tempfile::tempdir().unwrap();
        let a = file(dir.path(), "a", 1);
        let got = screen(vec![a], |_| None);
        assert_eq!(got.go.len(), 0);
        assert_eq!(got.no_bin, 1);
    }

    #[test]
    fn what_is_already_in_a_bin_is_not_sent_again() {
        let dir = tempfile::tempdir().unwrap();
        let inside = dir.path().join("$Recycle.Bin").join("S-1-5-21");
        std::fs::create_dir_all(&inside).unwrap();
        let held = file(&inside, "$R1.txt", 1);
        let fine = file(dir.path(), "fine", 1);
        let got = screen(vec![held, fine.clone()], |_| Some(u64::MAX));
        assert_eq!(got.go, vec![fine]);
        assert_eq!(got.no_bin, 1);
    }

    #[test]
    fn files_are_counted_together_against_the_room_left() {
        let dir = tempfile::tempdir().unwrap();
        let a = file(dir.path(), "a", 60);
        let b = file(dir.path(), "b", 60);
        let c = file(dir.path(), "c", 30);
        let got = screen(vec![a.clone(), b, c.clone()], |_| Some(100));
        assert_eq!(got.go, vec![a, c]);
        assert_eq!(got.no_room, 1);
        assert_eq!(got.no_bin, 0);
    }

    #[test]
    fn a_folder_counts_by_everything_in_it_and_a_missing_item_is_left_to_the_job() {
        let dir = tempfile::tempdir().unwrap();
        let small = dir.path().join("small");
        std::fs::create_dir(&small).unwrap();
        file(&small, "a", 5);
        let big = dir.path().join("big");
        std::fs::create_dir_all(big.join("deep/er")).unwrap();
        file(&big.join("deep/er"), "b", 50);
        let gone = dir.path().join("gone");
        let got = screen(vec![small.clone(), big, gone.clone()], |_| Some(20));
        assert_eq!(got.go, vec![small, gone]);
        assert_eq!(got.no_room, 1);
    }

    #[test]
    fn a_drive_is_asked_about_once() {
        let dir = tempfile::tempdir().unwrap();
        let paths = (0..5)
            .map(|i| file(dir.path(), &format!("f{i}"), 1))
            .collect();
        let asked = std::cell::Cell::new(0);
        let got = screen(paths, |_| {
            asked.set(asked.get() + 1);
            Some(u64::MAX)
        });
        assert_eq!(got.go.len(), 5);
        assert_eq!(asked.get(), 1);
    }

    #[test]
    fn the_room_of_one_drive_is_not_spent_by_another() {
        let got = screen(vec![PathBuf::from("x:/a"), PathBuf::from("y:/b")], |p| {
            Some(if p.starts_with("x:") { 0 } else { 10 })
        });
        assert_eq!(got.go.len(), 2);
    }

    #[test]
    fn bin_paths_are_told_by_their_folder_names() {
        assert!(in_bin(Path::new("C:/$RECYCLE.BIN/S-1/$R1")));
        assert!(!in_bin(Path::new("C:/Users/a/Trash/x")));
        assert!(!in_bin(Path::new("C:/a/$Recycle.Bin.bak/x")));
        #[cfg(not(windows))]
        {
            assert!(in_bin(Path::new("/home/a/.local/share/Trash/files/x")));
            assert!(in_bin(Path::new("/home/a/.Trash/x")));
            assert!(in_bin(Path::new("/Volumes/X/.Trashes/501/x")));
            assert!(in_bin(Path::new("/Users/a/.Trash/x")));
            assert!(in_bin(Path::new("/media/a/disk/.Trash-1000/files/x")));
            assert!(!in_bin(Path::new("/home/a/Trash/x")));
            assert!(!in_bin(Path::new("/home/a/proj/.trash-cache/x")));
            assert!(!in_bin(Path::new("/home/a/notes/.Trash/x")));
        }
        #[cfg(windows)]
        assert!(!in_bin(Path::new("D:\\stuff\\.Trash\\x")));
    }
}
