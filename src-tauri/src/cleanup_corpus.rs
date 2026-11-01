//! The cleanup detectors against a known corpus (issue #36): a tree laid out like a Windows
//! machine with two drives, holding things cleanup must never suggest (system and program
//! folders, profile files, app bundles, game libraries, unpacked projects), things it
//! should, and near misses that look like them. Every detector is run over the whole
//! corpus and the answer must be exactly the expected list: nothing from a protected path
//! (no hits), nothing that merely resembles a candidate (no false positives), and
//! nothing missed.
//!
//! The corpus lives under a temp folder as `c/...` and `d/...`. The deny list reads paths as
//! text, so each path is shown to it as it would be on a real machine (`C:/Windows/...`).

use crate::cleanup::{duplicates_with, is_protected, large_old, old_downloads};
use crate::scan::Scanner;
use std::path::{Path, PathBuf};

const MB: usize = 1 << 20;
const HOME: &str = "C:/Users/me";

struct Corpus {
    _dir: tempfile::TempDir,
    root: PathBuf,
}

impl Corpus {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        Self { _dir: dir, root }
    }

    /// A file of `mb` megabytes of `byte`, last changed `days_old` days ago.
    fn file(&self, rel: &str, byte: u8, mb: usize, days_old: u64) -> PathBuf {
        self.bytes(rel, &vec![byte; mb * MB], days_old)
    }

    fn bytes(&self, rel: &str, content: &[u8], days_old: u64) -> PathBuf {
        let path = self.root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, content).unwrap();
        let when = std::time::SystemTime::now() - std::time::Duration::from_hours(24 * days_old);
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(when)
            .unwrap();
        path
    }

    /// A path of the corpus as the deny list would see it on a real machine:
    /// `<root>/c/windows/x` is `C:/windows/x`.
    fn as_on_a_machine(&self, path: &Path) -> String {
        // As given, not resolved: a link into a protected folder must not be recognised here,
        // so the detectors' own check of the resolved path is what catches it.
        let text = path.to_string_lossy().replace('\\', "/");
        let text = text.strip_prefix("//?/").unwrap_or(&text).to_owned();
        let root = self.root.to_string_lossy().replace('\\', "/");
        let root = root.strip_prefix("//?/").unwrap_or(&root).to_owned();
        let rest = text
            .to_lowercase()
            .strip_prefix(&root.to_lowercase())
            .map_or(text.clone(), str::to_owned);
        let rest = rest.trim_start_matches('/');
        let (drive, tail) = rest.split_once('/').unwrap_or((rest, ""));
        format!("{}:/{tail}", drive.to_uppercase())
    }

    /// The deny list as the app applies it, over this corpus.
    fn skip(&self) -> impl Fn(&Path, u64) -> bool + '_ {
        move |path, _| {
            is_protected(
                Path::new(&self.as_on_a_machine(path)),
                Some(Path::new(HOME)),
            )
        }
    }

    fn scanner(&self) -> Scanner {
        let scanner = Scanner::open(None).unwrap();
        let cancel = std::sync::atomic::AtomicBool::new(false);
        scanner.scan(&self.root, &cancel, &|_, _| {}).unwrap();
        scanner
    }

    /// Paths as on a machine, lower case and sorted, to compare with an expected list.
    fn names<'a>(&self, paths: impl IntoIterator<Item = &'a str>) -> Vec<String> {
        let mut names: Vec<String> = paths
            .into_iter()
            .map(|p| self.as_on_a_machine(Path::new(p)).to_lowercase())
            .collect();
        names.sort();
        names
    }
}

fn expected(list: &[&str]) -> Vec<String> {
    let mut v: Vec<String> = list.iter().map(|s| (*s).to_lowercase()).collect();
    v.sort();
    v
}

/// Everything below must be left alone by every detector, however big, old or duplicated.
const PROTECTED: &[&str] = &[
    "c/Windows/System32/big.dll",
    "c/Windows/Installer/{guid}/patch.msp",
    "c/Windows.old/Users/x/old.bin",
    "c/Program Files/App/data.pak",
    "c/Program Files (x86)/App/data.pak",
    "c/ProgramData/App/cache.db",
    "c/Users/me/AppData/Local/App/cache.db",
    "c/Users/Other/NTUSER.DAT",
    "c/Users/Default/ntuser.dat",
    "c/Users/me/NTUSER.DAT",
    "c/pagefile.sys",
    "c/MSOCache/All Users/office.cab",
    "c/$Recycle.Bin/S-1-5/deleted.iso",
    "c/System Volume Information/restore.bin",
    "c/Apps/Foo.app/Contents/Resources/big.bin",
    "c/Users/me/Pictures/Photos.photoslibrary/originals/a.heic",
    "c/Users/me/dev/proj/node_modules/pkg/big.bin",
    "c/Users/me/dev/proj/.git/objects/pack/p.pack",
    "d/SteamLibrary/steamapps/common/Game/data.pak",
    "d/XboxGames/Game/Content/data.bin",
    // Programs, runtimes and libraries people keep in their own folders.
    "c/Users/me/anaconda3/Library/bin/mkl_core.dll",
    "c/Users/me/scoop/apps/tool/1.0/tool.exe",
    "c/Users/me/.rustup/toolchains/stable/bin/rustc_driver.dll",
    "c/Users/me/.ollama/models/blobs/sha256-abc",
    "c/Users/me/.android/avd/Pixel.avd/userdata-qemu.img",
    "c/Users/me/.vscode/extensions/ext-1.0/node.exe",
    "d/Epic Games/Fortnite/FortniteGame/Content/Paks/a.pak",
    "d/GOG Games/Witcher 3/content/a.bundle.bin",
    "c/Riot Games/League of Legends/Game/DATA/a.wad",
    "d/Ubisoft Game Launcher/games/X/a.forge",
    // Backups, virtual machines and mail stores.
    "d/FileHistory/me/PC/Data/C/Users/me/Pictures/photo (2019_01_01 10_00_00 UTC).jpg",
    "d/WindowsImageBackup/PC/Backup 2020/disk.vhdx",
    "c/Users/me/VirtualBox VMs/Win/Win.vdi",
    "c/Users/me/Documents/Virtual Machines/U/U-s001.vmdk",
    "c/Users/Public/Documents/Hyper-V/Virtual hard disks/vm.vhdx",
    "c/Users/me/Apple/MobileSync/Backup/0001/ab/abcdef",
    "c/Users/me/Documents/Outlook Files/archive.pst",
    "d/Disks/standalone.vhdx",
    // More system folders and other spellings.
    "c/Program Files (Arm)/App/data.pak",
    "c/Windows.old.000/Users/x/old.bin",
    "c/PerfLogs/Admin/log.etl",
    "c/inetpub/wwwroot/uploads/big.bin",
    "d/WpSystem/S-1-5-21/AppData/Local/Packages/x/big.bin",
    "d/Profiles/other/NTUSER.DAT",
    "d/hiberfil.sys",
];

/// Folders named like protected ones on purpose: cleanup leaves them alone to be safe.
const PROTECTED_ON_PURPOSE: &[&str] = &[
    "c/Users/me/Videos/Recovery/x.mkv",
    "c/Users/me/Documents/Windows/notes.docx",
];

#[test]
fn nothing_in_a_protected_path_is_ever_suggested_by_any_detector() {
    let corpus = Corpus::new();
    for rel in PROTECTED.iter().chain(PROTECTED_ON_PURPOSE) {
        // Big, old, and (below) the same bytes as files that are fair game.
        corpus.file(rel, 5, 5, 900);
    }
    corpus.file("c/Users/me/Documents/copy-of-protected.bin", 5, 5, 900);
    corpus.file("c/Users/me/Pictures/another-copy.bin", 5, 5, 900);
    // A backup of a file the person has: the original must never be grouped with it.
    corpus.file("c/Users/me/Pictures/photo.jpg", 9, 5, 900);
    let scanner = corpus.scanner();
    let now = crate::journal::unix_now();

    let large = large_old(&scanner, corpus.skip(), MB as u64, 30, 1000, now).unwrap();
    let hits: Vec<&str> = large.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(
        corpus.names(hits),
        expected(&[
            "C:/Users/me/Documents/copy-of-protected.bin",
            "C:/Users/me/Pictures/another-copy.bin",
            "C:/Users/me/Pictures/photo.jpg",
        ]),
        "large & old files"
    );

    let found = duplicates_with(&scanner, corpus.skip(), MB as u64, &|| false).unwrap();
    // Every protected file has the same bytes as the two fair copies; with them refused,
    // those two are the only group, and the File History copy of photo.jpg is not in it.
    assert_eq!(found.groups.len(), 1, "duplicates: {:?}", found.groups);
    let files: Vec<&str> = found.groups[0]
        .files
        .iter()
        .map(|f| f.path.as_str())
        .collect();
    assert_eq!(
        corpus.names(files),
        expected(&[
            "C:/Users/me/Documents/copy-of-protected.bin",
            "C:/Users/me/Pictures/another-copy.bin",
        ])
    );

    // And in Downloads: protected files placed there under protected folder names.
    let downloads = corpus.root.join("c/Users/me/Downloads");
    for rel in [
        "c/Users/me/Downloads/node_modules/pkg/dep.zip",
        "c/Users/me/Downloads/proj/.git/hooks/pack.zip",
        "c/Users/me/Downloads/Windows/Installer/x.msi",
    ] {
        corpus.file(rel, 1, 1, 900);
    }
    corpus.file("c/Users/me/Downloads/old.zip", 1, 1, 900);
    let offered = old_downloads(&downloads, corpus.skip(), |_| 0, 90, 1000, now);
    let hits: Vec<&str> = offered.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(
        corpus.names(hits),
        expected(&["C:/Users/me/Downloads/old.zip"])
    );
}

#[test]
fn large_and_old_files_are_exactly_the_big_old_ones_outside_protected_paths() {
    let corpus = Corpus::new();
    for rel in PROTECTED {
        corpus.file(rel, 5, 6, 900);
    }
    // Fair game.
    corpus.file("c/Users/me/Videos/old-movie.mkv", 1, 5, 800);
    corpus.file("c/Backups/2019/disk.img", 2, 7, 1500);
    corpus.file("d/Media/concert.mp4", 3, 5, 400);
    // Near misses: too small, too new, or both.
    corpus.file("c/Users/me/Videos/new-movie.mkv", 4, 9, 20);
    corpus.file("c/Users/me/Documents/small.txt", 6, 1, 900);
    corpus.file("c/Users/me/Videos/just-under-a-year.mkv", 7, 5, 300);
    corpus.file("c/Users/me/Videos/not-big-enough.mkv", 8, 3, 900);
    let scanner = corpus.scanner();
    let now = crate::journal::unix_now();
    let found = large_old(&scanner, corpus.skip(), 4 * MB as u64, 365, 1000, now).unwrap();
    let hits: Vec<&str> = found.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(
        corpus.names(hits),
        expected(&[
            "C:/Users/me/Videos/old-movie.mkv",
            "C:/Backups/2019/disk.img",
            "D:/Media/concert.mp4",
        ])
    );
    // Biggest first.
    assert!(found.windows(2).all(|w| w[0].size >= w[1].size));
}

#[test]
fn old_downloads_are_exactly_the_old_installers_and_archives_that_stand_alone() {
    let corpus = Corpus::new();
    let dl = "c/Users/me/Downloads";
    // Offered: installers and archives that have sat there, at the top or in a folder of
    // nothing else.
    corpus.file(&format!("{dl}/setup.EXE"), 1, 1, 400);
    corpus.file(&format!("{dl}/archive.zip"), 2, 1, 200);
    corpus.file(&format!("{dl}/old-setups/installer.msi"), 3, 1, 400);
    corpus.file(&format!("{dl}/old-setups/tools.7z"), 4, 1, 400);
    // Not offered.
    corpus.file(&format!("{dl}/fresh.zip"), 5, 1, 5); // new
    corpus.file(&format!("{dl}/report.pdf"), 6, 1, 900); // not an installer or archive
    corpus.file(&format!("{dl}/movie.mkv"), 7, 1, 900);
    corpus.file(&format!("{dl}/Portable/tool.exe"), 8, 1, 900); // an unpacked program
    corpus.file(&format!("{dl}/Portable/tool.dll"), 8, 1, 900);
    corpus.file(&format!("{dl}/mc-server/server.jar"), 9, 1, 900); // the program itself
    corpus.file(&format!("{dl}/mc-server/setup.zip"), 9, 1, 900);
    corpus.file(&format!("{dl}/a/b/c/d/too-deep.zip"), 1, 1, 900); // deeper than it looks
    corpus.file(&format!("{dl}/top.jar"), 2, 1, 900); // a jar at the top level is offered
    // Outside Downloads entirely: never looked at.
    corpus.file("c/Users/me/Documents/old.zip", 3, 1, 900);
    let now = crate::journal::unix_now();
    let found = old_downloads(&corpus.root.join(dl), corpus.skip(), |_| 0, 90, 1000, now);
    let hits: Vec<&str> = found.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(
        corpus.names(hits),
        expected(&[
            "C:/Users/me/Downloads/setup.EXE",
            "C:/Users/me/Downloads/archive.zip",
            "C:/Users/me/Downloads/old-setups/installer.msi",
            "C:/Users/me/Downloads/old-setups/tools.7z",
            "C:/Users/me/Downloads/top.jar",
        ])
    );
}

#[test]
fn duplicates_are_exactly_the_identical_files_and_nothing_that_only_looks_alike() {
    let corpus = Corpus::new();
    // A real group.
    corpus.file("c/Users/me/Documents/photo.raw", 1, 2, 300);
    corpus.file("c/Users/me/Pictures/photo-copy.raw", 1, 2, 100);
    // A group with a copy in a protected path: that copy is never read or offered, and the
    // one left has no partner.
    corpus.file("c/Windows/System32/shared.dll", 2, 2, 300);
    corpus.file("c/Users/me/Documents/copy-of-shared.dll", 2, 2, 100);
    // Same size, same first and last 64 KiB, different in the middle.
    corpus.file("c/Users/me/Documents/lookalike-a.bin", 3, 2, 100);
    let mut odd = vec![3u8; 2 * MB];
    odd[MB] = 4;
    corpus.bytes("c/Users/me/Documents/lookalike-b.bin", &odd, 100);
    // Same size, nothing alike.
    corpus.file("c/Users/me/Documents/other-a.bin", 5, 2, 100);
    corpus.file("c/Users/me/Documents/other-b.bin", 6, 2, 100);
    // Two names for one file: not two copies.
    let one = corpus.file("c/Users/me/Documents/one.bin", 7, 2, 100);
    std::fs::hard_link(&one, corpus.root.join("c/Users/me/Documents/one-link.bin")).unwrap();
    // Too small to be looked at.
    corpus.file("c/Users/me/Documents/tiny-a.txt", 8, 0, 100);
    corpus.bytes("c/Users/me/Documents/tiny-b.txt", &vec![8u8; 1000], 100);
    let scanner = corpus.scanner();
    let found = duplicates_with(&scanner, corpus.skip(), MB as u64, &|| false).unwrap();
    assert!(found.complete);
    assert_eq!(found.unreadable, 0);
    assert_eq!(found.groups.len(), 1, "{:?}", found.groups);
    let files: Vec<&str> = found.groups[0]
        .files
        .iter()
        .map(|f| f.path.as_str())
        .collect();
    assert_eq!(
        corpus.names(files),
        expected(&[
            "C:/Users/me/Documents/photo.raw",
            "C:/Users/me/Pictures/photo-copy.raw"
        ])
    );
    // The newest is first.
    assert!(found.groups[0].files[0].path.ends_with("photo-copy.raw"));
}

#[test]
fn the_deny_list_agrees_with_the_corpus_on_every_protected_path() {
    // The paths themselves, as the detectors see them, are all refused; a few that look
    // protected but are ordinary are not.
    let corpus = Corpus::new();
    for rel in PROTECTED {
        let path = corpus.file(rel, 1, 1, 1);
        assert!(corpus.skip()(&path, 0), "{rel} should be protected");
    }
    for rel in PROTECTED_ON_PURPOSE {
        let path = corpus.file(rel, 1, 1, 1);
        assert!(corpus.skip()(&path, 0), "{rel} is protected on purpose");
    }
    for rel in [
        "c/Users/me/Videos/Windows Movie Maker/x.wmv",
        "c/Users/me/Videos/a.mkv",
        "c/Users/me/Documents/windows-notes.docx",
        "d/Backups/2019/disk.img",
        "c/Users/me/Downloads/setup.exe",
        "c/Work/Programs/tool.exe",
    ] {
        let path = corpus.file(rel, 1, 1, 1);
        assert!(!corpus.skip()(&path, 0), "{rel} should be offered");
    }
}

#[test]
fn a_link_into_a_protected_folder_is_never_suggested() {
    let corpus = Corpus::new();
    corpus.file("c/Windows/System32/big.dll", 4, 5, 900);
    let link = corpus.root.join("c/Users/me/Videos/Link");
    std::fs::create_dir_all(link.parent().unwrap()).unwrap();
    let target = corpus.root.join("c/Windows/System32");
    #[cfg(windows)]
    let made = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(&link)
        .arg(&target)
        .output()
        .is_ok_and(|o| o.status.success());
    #[cfg(unix)]
    let made = std::os::unix::fs::symlink(&target, &link).is_ok();
    assert!(made, "the corpus junction could not be made");
    // A file of its own next to it, to be sure the scan found things.
    corpus.file("c/Users/me/Videos/own.mkv", 1, 5, 900);
    let scanner = corpus.scanner();
    let now = crate::journal::unix_now();
    let found = large_old(&scanner, corpus.skip(), MB as u64, 30, 1000, now).unwrap();
    let hits: Vec<&str> = found.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(
        corpus.names(hits),
        expected(&["C:/Users/me/Videos/own.mkv"])
    );
    // And listed by the spelling through the link, a file is still refused: the detectors
    // check where it really is.
    let via_link = crate::scan::BigFile {
        path: link.join("big.dll").to_string_lossy().into_owned(),
        size: (5 * MB) as u64,
        // Not older than the file: only the place it is in can refuse it.
        modified: i64::MAX,
    };
    let listed = [via_link];
    // Without the protected folder in the way it would be accepted: the control.
    assert_eq!(
        crate::cleanup::still_as_listed(&listed, |_, _| false).len(),
        1
    );
    assert_eq!(
        crate::cleanup::still_as_listed(&listed, corpus.skip()).len(),
        0
    );
}

#[test]
fn files_bigger_than_the_bin_room_are_never_suggested() {
    let corpus = Corpus::new();
    corpus.file("c/Users/me/Videos/fits.mkv", 1, 5, 900);
    corpus.file("c/Users/me/Videos/too-big.mkv", 2, 7, 900);
    corpus.file("d/Media/on-a-drive-with-no-bin.mp4", 3, 5, 900);
    let scanner = corpus.scanner();
    let now = crate::journal::unix_now();
    // C: has room for 6 MB; D: has no Recycle Bin.
    let room = |p: &str| {
        if p.starts_with("C:") {
            Some(6 * MB as u64)
        } else {
            None
        }
    };
    let skip = |p: &Path, size: u64| {
        let machine = corpus.as_on_a_machine(p);
        corpus.skip()(p, size) || room(&machine).is_none_or(|r| size > r)
    };
    let found = large_old(&scanner, skip, MB as u64, 30, 1000, now).unwrap();
    let hits: Vec<&str> = found.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(
        corpus.names(hits),
        expected(&["C:/Users/me/Videos/fits.mkv"])
    );
}
