//! What a delete may hand to the OS trash. The shell deletes for good, without asking, what
//! its bin can't take: a drive with no bin (network, removable, "Don't move files to the
//! Recycle Bin") or a file or folder bigger than the room left. Those are held back and reported, so a
//! delete is never silently a permanent one (ADR 0003).

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::{LazyLock, Mutex, MutexGuard, PoisonError};

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

/// What jobs running now have set aside in each bin, by volume: room that is not yet used
/// in the bin itself, so a job asking later does not count it twice over. Without it two
/// jobs on different lanes, each fitting the room on its own, could together overflow a bin,
/// and Windows deletes the oldest things in an overflowing bin for good.
pub struct Held(Mutex<Counts>);

/// Per volume: what is set aside now, and the total ever given back. A job whose screen
/// began before another one ended read the bin's room without that job's files in it, so
/// what was given back since counts against it too.
#[derive(Default)]
struct Counts {
    held: HashMap<String, u64>,
    given_back: HashMap<String, u64>,
}

impl Held {
    pub fn new() -> Self {
        Self(Mutex::new(Counts::default()))
    }

    fn lock(&self) -> MutexGuard<'_, Counts> {
        // A panic elsewhere cannot leave the counts half-written, so they are still good.
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Default for Held {
    fn default() -> Self {
        Self::new()
    }
}

/// What is known of a folder that items are in: its volume, and where it really is.
struct Folder {
    key: String,
    real: Option<PathBuf>,
}

/// The room set aside by one [`screen`], given back when it is dropped (the job is over).
pub struct Reservation<'a> {
    held: &'a Held,
    taken: HashMap<String, u64>,
}

impl Drop for Reservation<'_> {
    fn drop(&mut self) {
        let mut all = self.held.lock();
        for (key, bytes) in self.taken.drain() {
            if let Some(left) = all.held.get_mut(&key) {
                *left = left.saturating_sub(bytes);
                if *left == 0 {
                    all.held.remove(&key);
                }
            }
            let total = all.given_back.entry(key).or_insert(0);
            *total = total.saturating_add(bytes);
        }
    }
}

/// What every job in this process has set aside.
static HELD: LazyLock<Held> = LazyLock::new(Held::new);

/// What was sent on and what was held back, and why. Dropping it gives the room back.
pub struct Screened<'a> {
    pub go: Vec<PathBuf>,
    /// On a volume with no bin, or already in a bin.
    pub no_bin: usize,
    /// A file or folder that does not fit in the room the bin has left.
    pub no_room: usize,
    _room: Reservation<'a>,
}

/// Keeps what the bin can take, setting its room aside until the result is dropped.
/// `key_of` names the volume an item is on (the same whatever the path's spelling) and
/// `room_of` is the room that volume's bin has left, or `None` where it has no bin; it is
/// asked once for each volume. Files and the whole of each folder count against that room,
/// together with what other jobs have set aside in [`Held`].
pub fn screen_with(
    held: &Held,
    targets: Vec<PathBuf>,
    key_of: impl Fn(&Path) -> String,
    room_of: impl Fn(&Path) -> Option<u64>,
) -> Screened<'_> {
    let mut go = Vec::new();
    let (mut no_bin, mut no_room) = (0, 0);
    // The volume of each folder, and the room each volume's bin had when first asked
    // (`None`: no bin there).
    let mut folders: HashMap<PathBuf, Folder> = HashMap::new();
    // Per volume: its room, and what had been given back when that was read.
    let mut rooms: HashMap<String, (Option<u64>, u64)> = HashMap::new();
    // Set aside as the items are counted, so a panic or an early return gives it back.
    let mut reservation = Reservation {
        held,
        taken: HashMap::new(),
    };
    for path in targets {
        let folder = path.parent().map(Path::to_path_buf).unwrap_or_default();
        let here = folders.entry(folder.clone()).or_insert_with(|| Folder {
            key: key_of(&path),
            // Where the shell will find it: links followed, short names spelled out.
            real: std::fs::canonicalize(&folder).ok(),
        });
        let inside = in_bin(&path)
            || here
                .real
                .as_ref()
                .zip(path.file_name())
                .is_some_and(|(real, name)| in_bin(&real.join(name)));
        if inside {
            no_bin += 1;
            continue;
        }
        let key = here.key.clone();
        let &mut (room, back_then) = rooms.entry(key.clone()).or_insert_with(|| {
            // Noted before the room is read, so nothing given back after it is missed.
            let back = held.lock().given_back.get(&key).copied().unwrap_or(0);
            (room_of(&path), back)
        });
        let Some(room) = room else {
            no_bin += 1;
            continue;
        };
        let size = size_up_to(&path, room);
        let mut all = held.lock();
        let used = all.held.get(&key).copied().unwrap_or(0).saturating_add(
            all.given_back
                .get(&key)
                .copied()
                .unwrap_or(0)
                .saturating_sub(back_then),
        );
        if size > room.saturating_sub(used) {
            no_room += 1;
            continue;
        }
        *all.held.entry(key.clone()).or_insert(0) += size;
        *reservation.taken.entry(key).or_insert(0) += size;
        go.push(path);
    }
    Screened {
        go,
        no_bin,
        no_room,
        _room: reservation,
    }
}

/// [`screen_with`] for the bins of this computer and what all jobs here have set aside.
pub fn screen(targets: Vec<PathBuf>) -> Screened<'static> {
    screen_with(
        &HELD,
        targets,
        |p| crate::platform::bin_volume(p).key,
        crate::platform::recycle_limit,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(dir: &Path, name: &str, size: usize) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, vec![0_u8; size]).unwrap();
        p
    }

    /// Every path on one volume, with the room given.
    fn screen_in(held: &Held, paths: Vec<PathBuf>, room: Option<u64>) -> Screened<'_> {
        screen_with(held, paths, |_| "v".into(), |_| room)
    }

    #[test]
    fn a_drive_without_a_bin_holds_everything_back() {
        let dir = tempfile::tempdir().unwrap();
        let a = file(dir.path(), "a", 1);
        let held = Held::new();
        let got = screen_in(&held, vec![a], None);
        assert_eq!(got.go.len(), 0);
        assert_eq!(got.no_bin, 1);
    }

    #[test]
    fn what_is_already_in_a_bin_is_not_sent_again() {
        let dir = tempfile::tempdir().unwrap();
        let inside = dir.path().join("$Recycle.Bin").join("S-1-5-21");
        std::fs::create_dir_all(&inside).unwrap();
        let binned = file(&inside, "$R1.txt", 1);
        let fine = file(dir.path(), "fine", 1);
        let held = Held::new();
        let got = screen_in(&held, vec![binned, fine.clone()], Some(u64::MAX));
        assert_eq!(got.go, vec![fine]);
        assert_eq!(got.no_bin, 1);
    }

    #[test]
    fn files_are_counted_together_against_the_room_left() {
        let dir = tempfile::tempdir().unwrap();
        let a = file(dir.path(), "a", 60);
        let b = file(dir.path(), "b", 60);
        let c = file(dir.path(), "c", 30);
        let held = Held::new();
        let got = screen_in(&held, vec![a.clone(), b, c.clone()], Some(100));
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
        let held = Held::new();
        let got = screen_in(&held, vec![small.clone(), big, gone.clone()], Some(20));
        assert_eq!(got.go, vec![small, gone]);
        assert_eq!(got.no_room, 1);
    }

    #[test]
    fn a_volume_is_asked_about_once() {
        let dir = tempfile::tempdir().unwrap();
        let paths = (0..5)
            .map(|i| file(dir.path(), &format!("f{i}"), 1))
            .collect();
        let asked = std::cell::Cell::new(0);
        let held = Held::new();
        let got = screen_with(
            &held,
            paths,
            |_| "v".into(),
            |_| {
                asked.set(asked.get() + 1);
                Some(u64::MAX)
            },
        );
        assert_eq!(got.go.len(), 5);
        assert_eq!(asked.get(), 1);
    }

    #[test]
    fn the_room_of_one_volume_is_not_spent_by_another() {
        let held = Held::new();
        let got = screen_with(
            &held,
            vec![PathBuf::from("x:/a"), PathBuf::from("y:/b")],
            |p| if p.starts_with("x:") { "x" } else { "y" }.into(),
            |p| Some(if p.starts_with("x:") { 0 } else { 10 }),
        );
        assert_eq!(got.go.len(), 2);
    }

    #[test]
    fn jobs_at_the_same_time_cannot_both_fill_one_bin() {
        let dir = tempfile::tempdir().unwrap();
        let a = file(dir.path(), "a", 60);
        let b = file(dir.path(), "b", 60);
        let held = Held::new();
        let first = screen_in(&held, vec![a.clone()], Some(100));
        let second = screen_in(&held, vec![b.clone()], Some(100));
        assert_eq!(first.go, vec![a]);
        assert_eq!((second.go.len(), second.no_room), (0, 1));
        // The room comes back when the first job is over.
        drop(first);
        let third = screen_in(&held, vec![b.clone()], Some(100));
        assert_eq!(third.go, vec![b]);
    }

    #[test]
    fn room_given_back_while_a_screen_runs_still_counts() {
        let dir = tempfile::tempdir().unwrap();
        let a = file(dir.path(), "a", 60);
        let b = file(dir.path(), "b", 60);
        let held = Held::new();
        let first = screen_in(&held, vec![a], Some(100));
        let first = std::cell::RefCell::new(Some(first));
        // The second job reads the room while the first is still running; the first ends
        // before the second has counted its files. Its 60 is in the bin by then, but the
        // room that was read does not show it.
        let second = screen_with(
            &held,
            vec![b],
            |_| "v".into(),
            |_| {
                let room = Some(100);
                first.borrow_mut().take();
                room
            },
        );
        assert_eq!((second.go.len(), second.no_room), (0, 1));
    }

    #[test]
    fn a_link_into_a_bin_is_held_back() {
        let dir = tempfile::tempdir().unwrap();
        let inside = dir.path().join("$Recycle.Bin").join("S-1");
        std::fs::create_dir_all(&inside).unwrap();
        let held_file = file(&inside, "$R1", 1);
        let link = dir.path().join("link");
        #[cfg(windows)]
        {
            let made = std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(&link)
                .arg(&inside)
                .output()
                .unwrap();
            assert!(made.status.success());
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(&inside, &link).unwrap();
        let held = Held::new();
        let got = screen_in(&held, vec![link.join("$R1")], Some(u64::MAX));
        assert!(held_file.exists());
        assert_eq!((got.go.len(), got.no_bin), (0, 1));
    }

    #[test]
    fn a_held_back_item_sets_nothing_aside() {
        let dir = tempfile::tempdir().unwrap();
        let big = file(dir.path(), "big", 500);
        let small = file(dir.path(), "small", 60);
        let held = Held::new();
        let first = screen_in(&held, vec![big], Some(100));
        assert_eq!(first.no_room, 1);
        let second = screen_in(&held, vec![small.clone()], Some(100));
        assert_eq!(second.go, vec![small]);
    }

    #[test]
    fn what_is_set_aside_is_given_back_even_with_nothing_left_over() {
        let dir = tempfile::tempdir().unwrap();
        let a = file(dir.path(), "a", 10);
        let held = Held::new();
        drop(screen_in(&held, vec![a], Some(100)));
        assert!(held.lock().held.is_empty());
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
