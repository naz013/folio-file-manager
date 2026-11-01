//! Cleanup detectors. They only ever produce suggestions (ADR 0003): nothing here
//! removes a file, and nothing in a protected location is suggested.

use crate::scan::{BigFile, Scanner};
use std::path::Path;

/// Folders whose contents belong to the system, to installed programs or to a program's
/// own library, in any spelling of case. A file with one of these anywhere in its path is
/// never suggested.
const PROTECTED_DIRS: [&str; 28] = [
    "windows",
    "windows.old",
    "windowsapps",
    "program files",
    "program files (x86)",
    "programdata",
    "appdata",
    "$recycle.bin",
    "system volume information",
    "recovery",
    "boot",
    "efi",
    "msocache",
    "config.msi",
    "$windows.~bt",
    "$windows.~ws",
    "$winreagent",
    "$sysreset",
    "$getcurrent",
    "steamapps",
    "xboxgames",
    ".trash",
    ".cache",
    ".local",
    ".var",
    ".config",
    "node_modules",
    ".git",
];

/// Top-level folders of a Unix-like system that cleanup leaves alone.
const PROTECTED_ROOTS: [&str; 17] = [
    "usr",
    "bin",
    "sbin",
    "etc",
    "lib",
    "lib64",
    "boot",
    "dev",
    "proc",
    "sys",
    "var",
    "opt",
    "system",
    "library",
    "applications",
    "private",
    "snap",
];

/// Folders that are one thing to the program that owns them: taking a file out of one
/// breaks the whole.
const BUNDLE_EXTENSIONS: [&str; 7] = [
    "app",
    "photoslibrary",
    "fcpbundle",
    "musiclibrary",
    "framework",
    "bundle",
    "xcodeproj",
];

/// Whether cleanup must leave `path` alone: system and program folders, application
/// bundles, a user's `Library`, a file sitting directly in any user profile or at the top
/// of a drive, and (on Windows) files that exist only in the cloud. The path is read as
/// text, so the rule is the same whichever system built the cache. A `\\?\` prefix is
/// ignored. A path is also checked after its links are resolved, in [`large_old`].
pub fn is_protected(path: &Path, home: Option<&Path>) -> bool {
    let full = path.to_string_lossy().to_lowercase();
    let text = full.strip_prefix("\\\\?\\").unwrap_or(&full);
    let parts: Vec<&str> = text.split(['/', '\\']).filter(|p| !p.is_empty()).collect();
    if parts.iter().any(|p| {
        PROTECTED_DIRS.contains(p)
            || Path::new(p)
                .extension()
                .is_some_and(|e| BUNDLE_EXTENSIONS.iter().any(|b| e == *b))
    }) {
        return true;
    }
    if text.starts_with('/') && parts.first().is_some_and(|p| PROTECTED_ROOTS.contains(p)) {
        return true;
    }
    // `/Users/me/Library`, `/home/me/Library`.
    if parts.len() >= 3 && ["users", "home"].contains(&parts[0]) && parts[2] == "library" {
        return true;
    }
    // `C:\Users\<anyone>\file` holds profile and registry files; `Default` is a template.
    let profile_file = parts.get(1) == Some(&"users") && parts.len() == 4;
    let default_profile = parts.get(1) == Some(&"users") && parts.get(2) == Some(&"default");
    // A file at the top of a drive (`C:\file`, `/file`), or directly in the profile.
    let at_drive_top = parts.len() <= 2;
    let in_home = path
        .parent()
        .zip(home)
        .is_some_and(|(parent, h)| same_place(h, parent));
    profile_file || default_profile || at_drive_top || in_home || is_cloud_only(path)
}

fn same_place(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| {
        let text = p.to_string_lossy().to_lowercase().replace('\\', "/");
        text.strip_prefix("//?/")
            .unwrap_or(&text)
            .trim_end_matches('/')
            .to_owned()
    };
    norm(a) == norm(b)
}

/// A OneDrive-style placeholder whose content is not on this disk: reading it would
/// download it, and removing it is not freeing local space.
#[cfg(windows)]
fn is_cloud_only(path: &Path) -> bool {
    use std::os::windows::fs::MetadataExt;
    const OFFLINE: u32 = 0x1000;
    const RECALL_ON_OPEN: u32 = 0x4_0000;
    const RECALL_ON_DATA_ACCESS: u32 = 0x40_0000;
    std::fs::symlink_metadata(path).is_ok_and(|m| {
        m.file_attributes() & (OFFLINE | RECALL_ON_OPEN | RECALL_ON_DATA_ACCESS) != 0
    })
}

#[cfg(not(windows))]
fn is_cloud_only(_path: &Path) -> bool {
    false
}

/// Seconds in a day.
const DAY: i64 = 86_400;

/// The most files one list holds.
pub const MAX_LIST: usize = 1000;

/// What is on disk now for a file that may be suggested or moved: its size and the time
/// it last changed (seconds since 1970). `None` if it is gone, is not a plain file, or
/// `skip` refuses it, either as listed or once its links are resolved: a scan through a
/// junction or a `subst` drive lists a system folder under another name.
fn on_disk(path: &Path, skip: &impl Fn(&Path, u64) -> bool) -> Option<(u64, i64)> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    if !meta.is_file() {
        return None;
    }
    let size = meta.len();
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(i64::MAX, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));
    // Asked as listed first: resolving a link opens the file, which would start a download
    // of a cloud-only one.
    if skip(path, size) {
        return None;
    }
    // Where it really is; a file whose true place can't be found is left alone, and so is
    // one on another drive than it is listed under (a `subst` drive).
    let resolved = std::fs::canonicalize(path).ok()?;
    let same_drive = drive_of(path).is_none() || drive_of(path) == drive_of(&resolved);
    (same_drive && !skip(&resolved, size)).then_some((size, modified))
}

/// The drive letter a path starts with (`c` for `C:\x` and `\\?\C:\x`), if it has one.
fn drive_of(path: &Path) -> Option<char> {
    let text = path.to_string_lossy().to_lowercase();
    let text = text.strip_prefix("\\\\?\\").unwrap_or(&text);
    let mut chars = text.chars();
    let letter = chars.next().filter(char::is_ascii_alphabetic)?;
    (chars.next() == Some(':')).then_some(letter)
}

/// Keeps the items that fit the Recycle Bin of their drive together: a bin that overflows
/// throws away its oldest contents for good, so a batch is cut off where its total passes
/// `limit(path)` for the drive. The rest are left for another round.
pub fn within_bin(items: Vec<&BigFile>, limit: impl Fn(&Path) -> Option<u64>) -> Vec<&BigFile> {
    let mut used: Vec<(Option<char>, u64)> = Vec::new();
    items
        .into_iter()
        .filter(|f| {
            let path = Path::new(&f.path);
            let drive = drive_of(path);
            let Some(room) = limit(path) else {
                return false;
            };
            let at = used
                .iter()
                .position(|(d, _)| *d == drive)
                .unwrap_or_else(|| {
                    used.push((drive, 0));
                    used.len() - 1
                });
            let total = used[at].1.saturating_add(f.size);
            if total > room {
                return false;
            }
            used[at].1 = total;
            true
        })
        .collect()
}

/// Large files nobody has touched for a while, biggest first. The scan cache says where
/// to look; each file is checked on disk again, so a file that changed or vanished since
/// the scan is not suggested, and the size and date returned are the ones on disk now.
/// Links are never followed or suggested. `skip(path, size)` is the deny list
/// ([`is_protected`], and where the Recycle Bin can't take the file); a parameter so tests
/// can run under any temp folder.
pub fn large_old(
    scanner: &Scanner,
    skip: impl Fn(&Path, u64) -> bool,
    min_bytes: u64,
    older_than_days: u32,
    limit: usize,
    now: i64,
) -> rusqlite::Result<Vec<BigFile>> {
    let before = now - i64::from(older_than_days) * DAY;
    let limit = limit.min(MAX_LIST);
    let mut found = scanner.large_before(min_bytes, before, limit, |f| {
        on_disk(Path::new(&f.path), &skip)
            .is_some_and(|(size, modified)| size >= min_bytes && modified <= before)
    })?;
    // The figures on disk, not the cache's.
    for f in &mut found {
        if let Some((size, modified)) = on_disk(Path::new(&f.path), &skip) {
            f.size = size;
            f.modified = modified;
        }
    }
    Ok(found)
}

/// The items of a list shown earlier that are still exactly as listed, safe to move: same
/// size, not changed since, not protected. Anything else is left alone.
pub fn still_as_listed(items: &[BigFile], skip: impl Fn(&Path, u64) -> bool) -> Vec<&BigFile> {
    items
        .iter()
        .filter(|f| {
            on_disk(Path::new(&f.path), &skip)
                .is_some_and(|(size, modified)| size == f.size && modified <= f.modified)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    const MB: u64 = 1 << 20;

    fn protected(p: &str) -> bool {
        is_protected(Path::new(p), Some(Path::new("C:/Users/me")))
    }

    #[test]
    fn system_and_program_folders_are_protected() {
        for p in [
            "C:\\Windows\\System32\\big.dll",
            "D:\\windows.old\\Users\\x\\a.bin",
            "C:\\Program Files\\App\\data.pak",
            "C:\\Program Files (x86)\\App\\data.pak",
            "C:\\ProgramData\\App\\cache.db",
            "C:\\Users\\me\\AppData\\Local\\App\\x.db",
            "C:\\$Recycle.Bin\\S-1\\a.iso",
            "/usr/lib/big.so",
            "/Applications/Foo.app/Contents/Resources/x",
            "/home/me/Tools/Thing.app/x",
            "/var/lib/db",
            "C:\\Users\\Other\\NTUSER.DAT",
            "C:\\Users\\Default\\AppData\\x",
            "C:\\MSOCache\\All Users\\x.cab",
            "D:\\SteamLibrary\\steamapps\\common\\G\\data.pak",
            "\\\\?\\C:\\big.bin",
            "\\\\?\\C:\\Windows\\Installer\\x.msp",
            "/Users/me/Library/Mail/x",
            "/Volumes/Data/Photos Library.photoslibrary/originals/a.heic",
        ] {
            assert!(protected(p), "{p} should be protected");
        }
    }

    #[test]
    fn files_at_the_top_of_a_drive_or_profile_are_protected() {
        assert!(protected("C:\\pagefile.sys"));
        assert!(protected("C:/Users/me/NTUSER.DAT"));
        assert!(protected("c:\\users\\me\\ntuser.dat"));
    }

    #[test]
    fn things_people_put_away_are_not_protected() {
        for p in [
            "C:\\Users\\me\\Videos\\movie.mkv",
            "D:\\Backups\\2019\\disk.img",
            "/home/me/Videos/a.mkv",
            "/mnt/data/a.iso",
        ] {
            assert!(!protected(p), "{p} should be offered");
        }
    }

    fn put(dir: &Path, name: &str, size: u64, days_old: u64) -> PathBuf {
        let p = dir.join(name);
        let f = std::fs::File::create(&p).unwrap();
        f.set_len(size).unwrap();
        let when = std::time::SystemTime::now() - std::time::Duration::from_secs(days_old * 86_400);
        f.set_modified(when).unwrap();
        p
    }

    /// A scanned folder with a `Videos` folder inside, so nothing sits at a drive top.
    fn scanned(dir: &tempfile::TempDir, make: impl FnOnce(&Path)) -> Scanner {
        let sub = dir.path().join("Videos");
        std::fs::create_dir(&sub).unwrap();
        make(&sub);
        let scanner = Scanner::open(None).unwrap();
        let cancel = std::sync::atomic::AtomicBool::new(false);
        scanner.scan(dir.path(), &cancel, &|_, _| {}).unwrap();
        scanner
    }

    #[test]
    fn only_big_old_files_are_suggested_biggest_first() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("Videos");
        let scanner = scanned(&dir, |sub| {
            put(sub, "small-old.bin", 2 * MB, 400);
            put(sub, "big-old.bin", 5 * MB, 400);
            put(sub, "bigger-old.bin", 6 * MB, 800);
            put(sub, "big-new.bin", 9 * MB, 3);
        });
        let now = crate::journal::unix_now();
        let found = large_old(&scanner, |_, _| false, 4 * MB, 365, 10, now).unwrap();
        let names: Vec<String> = found
            .iter()
            .map(|f| {
                Path::new(&f.path)
                    .strip_prefix(&sub)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        assert_eq!(names, ["bigger-old.bin", "big-old.bin"]);
    }

    #[test]
    fn a_list_made_earlier_only_moves_what_is_still_as_listed() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("Videos");
        let scanner = scanned(&dir, |sub| {
            put(sub, "same.bin", 5 * MB, 400);
            put(sub, "edited.bin", 5 * MB, 400);
            put(sub, "grown.bin", 5 * MB, 400);
            put(sub, "gone.bin", 5 * MB, 400);
        });
        let now = crate::journal::unix_now();
        let listed = large_old(&scanner, |_, _| false, MB, 365, 10, now).unwrap();
        assert_eq!(listed.len(), 4);
        std::fs::File::options()
            .write(true)
            .open(sub.join("edited.bin"))
            .unwrap()
            .set_modified(std::time::SystemTime::now())
            .unwrap();
        std::fs::File::options()
            .write(true)
            .open(sub.join("grown.bin"))
            .unwrap()
            .set_len(6 * MB)
            .unwrap();
        std::fs::remove_file(sub.join("gone.bin")).unwrap();
        let ok = still_as_listed(&listed, |_, _| false);
        let names: Vec<String> = ok
            .iter()
            .map(|f| {
                Path::new(&f.path)
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        assert_eq!(names, ["same.bin"]);
    }

    #[test]
    fn a_listed_file_that_became_protected_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let scanner = scanned(&dir, |sub| {
            put(sub, "a.bin", 5 * MB, 400);
        });
        let now = crate::journal::unix_now();
        let listed = large_old(&scanner, |_, _| false, MB, 365, 10, now).unwrap();
        assert_eq!(still_as_listed(&listed, |_, _| true).len(), 0);
    }

    #[test]
    fn a_file_too_big_for_the_bin_is_never_suggested() {
        let dir = tempfile::tempdir().unwrap();
        let scanner = scanned(&dir, |sub| {
            put(sub, "huge.bin", 8 * MB, 400);
            put(sub, "ok.bin", 3 * MB, 400);
        });
        let now = crate::journal::unix_now();
        let found = large_old(&scanner, |_, size| size > 5 * MB, MB, 365, 10, now).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].size, 3 * MB);
    }

    #[test]
    fn a_file_reached_through_a_link_to_a_protected_folder_is_not_suggested() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("Windows");
        std::fs::create_dir(&real).unwrap();
        put(&real, "big.msp", 5 * MB, 400);
        let alias = dir.path().join("Alias");
        #[cfg(windows)]
        let linked = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&alias)
            .arg(&real)
            .output()
            .is_ok_and(|o| o.status.success());
        #[cfg(unix)]
        let linked = std::os::unix::fs::symlink(&real, &alias).is_ok();
        assert!(
            linked || cfg!(not(windows)),
            "the test junction could not be made"
        );
        if !linked {
            return;
        }
        let item = BigFile {
            path: alias.join("big.msp").to_string_lossy().into_owned(),
            size: 5 * MB,
            modified: 0,
        };
        // As listed the path says nothing; resolved, it is inside `Windows`.
        let skip = |p: &Path, _: u64| p.to_string_lossy().to_lowercase().contains("windows");
        assert_eq!(still_as_listed(&[item], skip).len(), 0);
    }

    #[test]
    fn a_batch_is_cut_off_where_it_would_overflow_the_bin() {
        let item = |name: &str, mb: u64| BigFile {
            path: name.to_owned(),
            size: mb * MB,
            modified: 0,
        };
        let items = [
            item("C:\\a\\1", 4),
            item("C:\\a\\2", 4),
            item("D:\\a\\3", 4),
            item("C:\\a\\4", 1),
        ];
        let kept = within_bin(items.iter().collect(), |_| Some(6 * MB));
        let names: Vec<&str> = kept.iter().map(|f| f.path.as_str()).collect();
        // C: takes 4 then 1 (the second 4 would pass 6); D: has its own room.
        assert_eq!(names, ["C:\\a\\1", "D:\\a\\3", "C:\\a\\4"]);
        assert_eq!(within_bin(items.iter().collect(), |_| None).len(), 0);
    }

    #[test]
    fn the_drive_letter_is_read_from_either_spelling() {
        assert_eq!(drive_of(Path::new("C:\\x")), Some('c'));
        assert_eq!(drive_of(Path::new("\\\\?\\d:\\x")), Some('d'));
        assert_eq!(drive_of(Path::new("/x")), None);
    }

    #[test]
    fn a_file_changed_or_removed_since_the_scan_is_not_suggested() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("Videos");
        let scanner = scanned(&dir, |sub| {
            put(sub, "gone.bin", 5 * MB, 400);
            put(sub, "touched.bin", 5 * MB, 400);
            put(sub, "kept.bin", 5 * MB, 400);
        });
        std::fs::remove_file(sub.join("gone.bin")).unwrap();
        std::fs::File::options()
            .write(true)
            .open(sub.join("touched.bin"))
            .unwrap()
            .set_modified(std::time::SystemTime::now())
            .unwrap();
        let now = crate::journal::unix_now();
        let found = large_old(&scanner, |_, _| false, MB, 365, 10, now).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].path, sub.join("kept.bin").to_string_lossy());
    }

    #[test]
    fn the_limit_counts_only_accepted_files() {
        let dir = tempfile::tempdir().unwrap();
        let scanner = scanned(&dir, |sub| {
            for i in 0..5 {
                put(sub, &format!("f{i}.bin"), (5 + i) * MB, 400);
            }
        });
        let now = crate::journal::unix_now();
        // The two biggest are rejected by the filter; the limit still yields two.
        let found = scanner
            .large_before(MB, now, 2, |f| {
                !f.path.contains("f4") && !f.path.contains("f3")
            })
            .unwrap();
        assert_eq!(found.len(), 2);
    }
}
