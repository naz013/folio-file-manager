//! Cleanup detectors. They only ever produce suggestions (ADR 0003): nothing here
//! removes a file, and nothing in a protected location is suggested.

use crate::scan::{BigFile, Scanner};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Folders whose contents belong to the system, to installed programs, to a program's own
/// library or to a backup, in any spelling of case. A file with one of these anywhere in
/// its path is never suggested. Folders that merely start the same way are in
/// [`PROTECTED_PREFIXES`].
const PROTECTED_DIRS: &[&str] = &[
    // The system.
    "windows",
    "windowsapps",
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
    "perflogs",
    "inetpub",
    "onedrivetemp",
    "wpsystem",
    // Programs and runtimes people install into their own folders, and game libraries.
    "anaconda3",
    "miniconda3",
    "miniforge3",
    "scoop",
    "steamapps",
    "xboxgames",
    "epic games",
    "gog games",
    "gog galaxy",
    "ea games",
    "origin games",
    "riot games",
    "ubisoft game launcher",
    "battle.net",
    "node_modules",
    ".git",
    // Backups and virtual machines: taking a piece out breaks the whole.
    "filehistory",
    "windowsimagebackup",
    "backups.backupdb",
    "mobilesync",
    "virtualbox vms",
    "virtual machines",
    "hyper-v",
    // The trash and index folders of other volumes.
    ".trashes",
    ".spotlight-v100",
    ".fseventsd",
    ".documentrevisions-v100",
    ".mobilebackups",
];

/// Folder names that start these are protected too (`Program Files (Arm)`, `Windows.old.000`,
/// `.Trash-1000`).
const PROTECTED_PREFIXES: &[&str] = &["program files", "windows.old", ".trash"];

/// Top-level folders of a Unix-like system that cleanup leaves alone.
const PROTECTED_ROOTS: &[&str] = &[
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
    "nix",
    "gnu",
    "run",
    "srv",
    "root",
    "lost+found",
    "system",
    "library",
    "applications",
    "private",
    "snap",
];

/// Folders that are one thing to the program that owns them: taking a file out of one
/// breaks the whole.
const BUNDLE_EXTENSIONS: &[&str] = &[
    "app",
    "photoslibrary",
    "photolibrary",
    "migratedphotolibrary",
    "fcpbundle",
    "musiclibrary",
    "imovielibrary",
    "tvlibrary",
    "aplibrary",
    "logicx",
    "band",
    "lrdata",
    "framework",
    "bundle",
    "xcodeproj",
    "xcworkspace",
    "xcarchive",
    "sparsebundle",
    "backupbundle",
    "pvm",
    "vmwarevm",
    "utm",
    "plugin",
    "kext",
    "appex",
    "vst",
    "vst3",
    "component",
    "prefpane",
    "pages",
    "numbers",
    "key",
    "rtfd",
    "scriv",
    "dsym",
];

/// Kinds of file that are one piece of something that cannot be cut up: a virtual disk, a
/// mail store, a database, a backup image. Never suggested, wherever they sit.
const PROTECTED_KINDS: &[&str] = &[
    "vhd", "vhdx", "avhdx", "vmdk", "vdi", "qcow2", "vmem", "vmsn", "vmss", "nvram", "hds", "pst",
    "ost", "nst", "lrcat", "kdbx", "mdf", "ldf", "ndf", "ibd", "tib", "mrimg", "vbk", "vib",
];

/// Files that hold a machine's or user's state, wherever they are found.
const PROTECTED_NAMES: &[&str] = &["pagefile.sys", "hiberfil.sys", "swapfile.sys"];

/// Whether cleanup must leave `path` alone: system and program folders, application
/// bundles, a user's `Library` and dot-folders, a file sitting directly in any user profile
/// or at the top of a drive, files that are one piece of something larger (see
/// [`PROTECTED_KINDS`]), network and device paths, and (on Windows) files that exist only in
/// the cloud. The path is read as text, so the rule is the same whichever system built the
/// cache. A `\\?\` prefix is ignored. A path is also checked after its links are resolved,
/// in [`large_old`].
pub fn is_protected(path: &Path, home: Option<&Path>) -> bool {
    let full = path.to_string_lossy().to_lowercase();
    // Network shares (`\\server\share`, `\\wsl.localhost\...`) and device paths
    // (`\\.\C:`) are not read as places on this machine; only `\\?\C:\` is.
    let slashed = full.replace('/', "\\");
    if slashed.starts_with("\\\\") {
        let verbatim_drive = slashed
            .strip_prefix("\\\\?\\")
            .is_some_and(|rest| rest.as_bytes().get(1) == Some(&b':'));
        if !verbatim_drive {
            return true;
        }
    }
    let text = full.strip_prefix("\\\\?\\").unwrap_or(&full);
    let parts: Vec<&str> = text.split(['/', '\\']).filter(|p| !p.is_empty()).collect();
    let last = parts.len().saturating_sub(1);
    let in_a_profile = |i: usize| i >= 2 && ["users", "home"].contains(&parts[i - 2]);
    for (i, part) in parts.iter().enumerate() {
        let is_dir = i < last;
        let extension = Path::new(part).extension().and_then(|e| e.to_str());
        let named = PROTECTED_DIRS.contains(part)
            || PROTECTED_PREFIXES.iter().any(|p| part.starts_with(p))
            || extension.is_some_and(|e| BUNDLE_EXTENSIONS.contains(&e))
            // `C:\Users\me\.rustup`, `/home/me/.thunderbird`, `~/snap`, `/Users/me/Library`:
            // what programs keep in the profile.
            || (is_dir && in_a_profile(i) && (part.starts_with('.') || ["snap", "library"].contains(part)));
        if named {
            return true;
        }
    }
    let name = parts.last().copied().unwrap_or("");
    let kind = Path::new(name).extension().and_then(|e| e.to_str());
    if PROTECTED_NAMES.contains(&name)
        || name.starts_with("ntuser.dat")
        || name.starts_with("usrclass.dat")
        || kind.is_some_and(|k| PROTECTED_KINDS.contains(&k))
    {
        return true;
    }
    // `/usr/...`, and the same on another volume (`/Volumes/Old/System/...`).
    if text.starts_with('/') {
        let top = if parts.first() == Some(&"volumes") {
            2
        } else {
            0
        };
        if parts.get(top).is_some_and(|p| PROTECTED_ROOTS.contains(p)) {
            return true;
        }
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

/// Whether an entry's attributes say its content is not on this disk: a OneDrive-style
/// placeholder. Reading one would download it, listing a folder of them makes the sync
/// provider fetch it, and removing it is not freeing local space.
#[cfg(windows)]
fn cloud_only_attributes(meta: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const OFFLINE: u32 = 0x1000;
    const RECALL_ON_OPEN: u32 = 0x4_0000;
    const RECALL_ON_DATA_ACCESS: u32 = 0x40_0000;
    meta.file_attributes() & (OFFLINE | RECALL_ON_OPEN | RECALL_ON_DATA_ACCESS) != 0
}

#[cfg(not(windows))]
fn cloud_only_attributes(_meta: &std::fs::Metadata) -> bool {
    false
}

fn is_cloud_only(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| cloud_only_attributes(&m))
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
    // An unfinished copy, or a file inside one, may be a copy still being built: the list of
    // unfinished copies is the only way to it, and it looks at whether a copy is running.
    if path
        .components()
        .any(|c| is_partial_name(&c.as_os_str().to_string_lossy()))
    {
        return None;
    }
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

/// Installers and archives: what piles up in Downloads once it has done its job.
const DOWNLOAD_KINDS: [&str; 26] = [
    "exe",
    "msi",
    "msix",
    "msixbundle",
    "appx",
    "appxbundle",
    "dmg",
    "pkg",
    "deb",
    "rpm",
    "apk",
    "iso",
    "img",
    "zip",
    "7z",
    "rar",
    "tar",
    "gz",
    "tgz",
    "bz2",
    "xz",
    "zst",
    "cab",
    "jar",
    "appimage",
    "run",
];

/// How deep into Downloads the search goes, and how many entries it looks at in all, so a
/// folder with a project unpacked in it can't keep it busy.
const DOWNLOADS_DEPTH: usize = 3;
const DOWNLOADS_SEEN: usize = 50_000;

/// The shortest age cleanup will look for in Downloads, whatever it is asked: a download
/// that is still arriving or was just made is never old.
pub const MIN_DOWNLOAD_AGE_DAYS: u32 = 14;

/// Whether `dir` is a Downloads folder worth searching. Windows lets Downloads be moved
/// to a drive's root or to the profile, and a search there would offer everything in it
/// as a leftover download.
pub fn downloads_folder_ok(dir: &Path, home: Option<&Path>) -> bool {
    let home = home.map(norm_path);
    let places = [Some(dir.to_path_buf()), std::fs::canonicalize(dir).ok()];
    places.iter().all(|place| {
        place.as_ref().is_some_and(|p| {
            let here = norm_path(p);
            // The profile itself, or a folder holding it.
            let above_home = home
                .as_ref()
                .is_some_and(|h| *h == here || h.starts_with(&format!("{here}/")));
            // A file in it must be one cleanup could suggest: not at a drive's top, not
            // directly in the profile, not in a system folder.
            !above_home
                && !is_protected(&p.join("x"), None)
                && !is_protected(&p.join("x"), home.as_deref().map(Path::new))
        })
    })
}

fn norm_path(p: &Path) -> String {
    let text = p.to_string_lossy().to_lowercase().replace('\\', "/");
    text.strip_prefix("//?/")
        .unwrap_or(&text)
        .trim_end_matches('/')
        .to_owned()
}

/// When a file came to be on this disk: its creation time, or its change time where the
/// system keeps none. A zip's entries, a copy from a USB stick and a file `wget` fetched
/// all keep an old change time while being new here.
pub fn arrived(path: &Path) -> i64 {
    let meta = std::fs::symlink_metadata(path).ok();
    let secs = |t: std::io::Result<std::time::SystemTime>| {
        t.ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
    };
    meta.map_or(i64::MAX, |m| secs(m.created()).max(secs(m.modified())))
}

/// Installers and archives in `dir` (the Downloads folder) that have not changed, and did
/// not arrive, for `older_than_days`, biggest first. The folder is read directly rather
/// than from the scan cache: these are often small, and nothing needs scanning first.
/// Links are not followed, and neither are cloud-only folders (listing one makes the sync
/// provider fetch it). A subfolder holding anything that is not an installer or archive is
/// an unpacked program or project, and is left whole. `skip` is the deny list, as for
/// [`large_old`]; `arrived_at` says when a file came to be here (see [`arrived`]).
pub fn old_downloads(
    dir: &Path,
    skip: impl Fn(&Path, u64) -> bool,
    arrived_at: impl Fn(&Path) -> i64,
    older_than_days: u32,
    limit: usize,
    now: i64,
) -> Vec<BigFile> {
    let before = now - i64::from(older_than_days.max(MIN_DOWNLOAD_AGE_DAYS)) * DAY;
    let mut found = Vec::new();
    let mut todo = vec![(dir.to_path_buf(), 0usize)];
    let mut seen = 0usize;
    while let Some((folder, depth)) = todo.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        let mut here = Vec::new();
        let mut has_other_files = false;
        for entry in entries.flatten() {
            seen += 1;
            if seen > DOWNLOADS_SEEN {
                return finish(found, limit);
            }
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let path = entry.path();
            if kind.is_dir() {
                // Not into folders no suggestion could come from (`node_modules`, `.git`),
                // which would only use up the budget, nor into ones that exist only in the
                // cloud.
                let name = entry.file_name().to_string_lossy().to_lowercase();
                if depth < DOWNLOADS_DEPTH
                    && !PROTECTED_DIRS.contains(&name.as_str())
                    && !is_cloud_only(&path)
                {
                    todo.push((path, depth + 1));
                }
            } else if kind.is_file() {
                let kind_ok = path.extension().is_some_and(|e| {
                    let e = e.to_string_lossy().to_lowercase();
                    DOWNLOAD_KINDS.contains(&e.as_str())
                        // Below the top level these are the program itself, not a setup.
                        && (depth == 0 || !matches!(e.as_str(), "jar" | "appimage" | "run"))
                });
                if kind_ok {
                    here.push(path);
                } else {
                    has_other_files = true;
                }
            }
        }
        if depth > 0 && has_other_files {
            continue;
        }
        for path in here {
            if let Some((size, modified)) = on_disk(&path, &skip)
                && modified.max(arrived_at(&path)) <= before
            {
                found.push(BigFile {
                    path: path.to_string_lossy().into_owned(),
                    size,
                    modified,
                });
            }
        }
    }
    finish(found, limit)
}

fn finish(mut found: Vec<BigFile>, limit: usize) -> Vec<BigFile> {
    found.sort_by(|a, b| b.size.cmp(&a.size).then_with(|| a.path.cmp(&b.path)));
    found.truncate(limit.min(MAX_LIST));
    found
}

/// Copies of one file: same size and same content, in different places.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct DuplicateGroup {
    /// The content's hash, to tell groups apart.
    pub id: String,
    pub size: u64,
    /// At least two, newest first.
    pub files: Vec<BigFile>,
}

/// What a duplicate search found, and whether it looked at everything.
#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Duplicates {
    pub groups: Vec<DuplicateGroup>,
    /// False when the search stopped at its limits or was replaced by a newer one, so
    /// there may be more.
    pub complete: bool,
    /// Files that could not be read (locked, gone) and so were left out.
    pub unreadable: usize,
}

/// How much of the start and end of a file is read for the first, cheap comparison.
const PEEK: u64 = 64 * 1024;
/// Most files looked at, most groups returned, most copies shown of one group, and most
/// bytes read for comparing, in one search.
const DUP_FILES: usize = 20_000;
const DUP_GROUPS: usize = 200;
const DUP_COPIES: usize = 20;
const DUP_BYTES: u64 = 8 << 30;

/// Counts the searches started; a search that finds a newer one has started stops, so a
/// changed setting or a closed view doesn't leave a long read running behind it.
static SEARCH: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Stops the search in progress, if any.
pub fn cancel_duplicates() {
    SEARCH.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
}

/// Whether a path is in a folder files only pass through (Downloads, Desktop, a temp
/// folder): the copy kept by default is not one of those if another exists.
fn passing_through(path: &str) -> bool {
    let lower = path.to_lowercase().replace('\\', "/");
    lower
        .split('/')
        .any(|part| ["downloads", "desktop", "temp", "tmp"].contains(&part))
}

fn hash_of(
    path: &Path,
    whole: bool,
    size: u64,
    buf: &mut [u8],
    stale: &dyn Fn() -> bool,
) -> std::io::Result<String> {
    use std::io::{Read, Seek, SeekFrom};
    // It may have become a placeholder since it was listed; reading one downloads it.
    if is_cloud_only(path) {
        return Err(std::io::Error::other("the file is only in the cloud"));
    }
    let mut file = std::fs::File::open(path)?;
    let mut hasher = blake3::Hasher::new();
    hasher.update(&size.to_le_bytes());
    if whole || size <= 2 * PEEK {
        loop {
            if stale() {
                return Err(std::io::Error::other("the search was replaced"));
            }
            let n = file.read(buf)?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
        }
    } else {
        let peek = &mut buf[..usize::try_from(PEEK).unwrap_or(65_536)];
        file.read_exact(peek)?;
        hasher.update(peek);
        file.seek(SeekFrom::Start(size - PEEK))?;
        file.read_exact(peek)?;
        hasher.update(peek);
    }
    Ok(hasher.finalize().to_hex().to_string())
}

/// Whether two files really hold the same bytes, read side by side. A hash says it is
/// all but certain; this is the last word before anything is offered for removal.
fn same_bytes(left: &Path, right: &Path, stale: &dyn Fn() -> bool) -> bool {
    use std::io::Read;
    if is_cloud_only(left) || is_cloud_only(right) {
        return false;
    }
    let (Ok(mut from_left), Ok(mut from_right)) =
        (std::fs::File::open(left), std::fs::File::open(right))
    else {
        return false;
    };
    let (mut one, mut two) = (vec![0u8; 1 << 16], vec![0u8; 1 << 16]);
    loop {
        if stale() {
            return false;
        }
        let Ok(n) = from_left.read(&mut one) else {
            return false;
        };
        if n == 0 {
            return from_right.read(&mut two).is_ok_and(|m| m == 0);
        }
        if from_right.read_exact(&mut two[..n]).is_err() || one[..n] != two[..n] {
            return false;
        }
    }
}

/// Files of the same content among the scanned files of at least `min_bytes`: size first
/// (free, from the scan cache), then the start and end of each file, then the whole file,
/// then a byte-for-byte check. Hard links to one file count once, since removing one
/// frees nothing. Nothing is read from a file the deny list refuses. The newest copy of
/// each group comes first. A newer search stops this one ([`cancel_duplicates`]).
pub fn duplicates(
    scanner: &Scanner,
    skip: impl Fn(&Path, u64) -> bool,
    min_bytes: u64,
) -> rusqlite::Result<Duplicates> {
    use std::sync::atomic::Ordering;
    let mine = SEARCH.fetch_add(1, Ordering::SeqCst) + 1;
    duplicates_with(scanner, skip, min_bytes, &|| {
        SEARCH.load(Ordering::SeqCst) != mine
    })
}

/// The files of one size as they are on disk now, one entry per file (not per hard link).
/// A file the deny list refuses, or whose identity can't be read (it could be a link to
/// another), is left out.
fn live_copies(same: Vec<BigFile>, skip: &impl Fn(&Path, u64) -> bool) -> Vec<BigFile> {
    let mut ids: Vec<String> = Vec::new();
    let mut live: Vec<BigFile> = Vec::new();
    for f in same {
        let path = Path::new(&f.path);
        let Some((size, modified)) = on_disk(path, skip) else {
            continue;
        };
        let Some(id) = crate::ops::identity(path) else {
            continue;
        };
        if ids.contains(&id) {
            continue;
        }
        ids.push(id);
        live.push(BigFile {
            path: f.path,
            size,
            modified,
        });
    }
    live
}

/// [`duplicates`] with its own test for being replaced. A search that is replaced stops,
/// says it is incomplete, and does not count what it left unread as unreadable.
pub(crate) fn duplicates_with(
    scanner: &Scanner,
    skip: impl Fn(&Path, u64) -> bool,
    min_bytes: u64,
    stale: &dyn Fn() -> bool,
) -> rusqlite::Result<Duplicates> {
    use std::collections::BTreeMap;
    let replaced = |groups: Vec<DuplicateGroup>, unreadable: usize| {
        Ok(Duplicates {
            groups,
            complete: false,
            unreadable,
        })
    };
    let candidates = scanner.same_size(min_bytes, DUP_FILES + 1)?;
    let mut complete = candidates.len() <= DUP_FILES;
    let mut unreadable = 0usize;
    let mut groups: Vec<DuplicateGroup> = Vec::new();
    let mut read_bytes = 0u64;
    let mut buf = vec![0u8; 1 << 20];
    // The cache lists them size by size, biggest first.
    let mut rest = candidates.into_iter().take(DUP_FILES).peekable();
    while let Some(first) = rest.next() {
        if stale() {
            return replaced(groups, unreadable);
        }
        let mut same = vec![first];
        while let Some(next) = rest.next_if(|n| n.size == same[0].size) {
            same.push(next);
        }
        let live = live_copies(same, &skip);
        if live.len() < 2 {
            continue;
        }
        let mut by_peek: BTreeMap<String, Vec<BigFile>> = BTreeMap::new();
        for f in live {
            match hash_of(Path::new(&f.path), false, f.size, &mut buf, stale) {
                Ok(h) => by_peek.entry(h).or_default().push(f),
                Err(_) => unreadable += 1,
            }
        }
        if stale() {
            return replaced(groups, 0);
        }
        for peeked in by_peek.into_values().filter(|g| g.len() > 1) {
            // Reading the whole of the survivors, and again to compare them, has a cost:
            // a size class that would pass the limit is left out, smaller ones go on.
            let size = peeked[0].size;
            let cost = size.saturating_mul(2 * peeked.len() as u64);
            if read_bytes.saturating_add(cost) > DUP_BYTES {
                complete = false;
                continue;
            }
            read_bytes += cost;
            let mut by_whole: BTreeMap<String, Vec<BigFile>> = BTreeMap::new();
            for f in peeked {
                match hash_of(Path::new(&f.path), true, f.size, &mut buf, stale) {
                    Ok(h) => by_whole.entry(h).or_default().push(f),
                    Err(_) => unreadable += 1,
                }
            }
            if stale() {
                return replaced(groups, 0);
            }
            for (id, mut files) in by_whole.into_iter().filter(|(_, g)| g.len() > 1) {
                files.sort_by(|a, b| {
                    passing_through(&a.path)
                        .cmp(&passing_through(&b.path))
                        .then_with(|| b.modified.cmp(&a.modified))
                        .then_with(|| a.path.cmp(&b.path))
                });
                // Only files that match the newest, byte for byte, are offered with it.
                let newest = files[0].path.clone();
                files.retain(|f| {
                    f.path == newest || same_bytes(Path::new(&newest), Path::new(&f.path), stale)
                });
                if stale() {
                    return replaced(groups, 0);
                }
                // A group with more copies than can be reviewed is cut to the first ones
                // (the copy kept by default is among them); the rest stay where they are.
                files.truncate(DUP_COPIES);
                if files.len() > 1 {
                    groups.push(DuplicateGroup {
                        id,
                        size: files[0].size,
                        files,
                    });
                }
            }
        }
        if groups.len() >= DUP_GROUPS {
            complete = false;
            break;
        }
    }
    // The most to gain first.
    groups.sort_by_key(|g| std::cmp::Reverse(g.size.saturating_mul(g.files.len() as u64 - 1)));
    groups.truncate(DUP_GROUPS);
    Ok(Duplicates {
        groups,
        complete,
        unreadable,
    })
}

/// A duplicate group the person reviewed: every copy, and the ones to move to the Recycle
/// Bin. The rest are kept.
#[derive(Deserialize)]
pub struct Reviewed {
    pub files: Vec<BigFile>,
    pub remove: Vec<String>,
}

/// A path in a form that two spellings of the same place share: lower case, one kind of
/// separator, no `\\?\` prefix, resolved through links where it can be.
fn same_spelling(path: &Path) -> String {
    let resolved = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let text = resolved.to_string_lossy().to_lowercase().replace('\\', "/");
    text.strip_prefix("//?/").unwrap_or(&text).to_owned()
}

/// A copy cleared for the Recycle Bin, with the copies that stay for it (path, identity,
/// size), so a last look just before the move can see they are still there.
pub struct Removal {
    pub file: BigFile,
    pub keepers: Vec<(String, String, u64)>,
}

/// Whether every copy that stays for `removal` is still the same file, as recorded.
pub fn keepers_still_there(removal: &Removal) -> bool {
    removal.keepers.iter().any(|(path, id, size)| {
        let path = Path::new(path);
        std::fs::symlink_metadata(path).is_ok_and(|m| m.is_file() && m.len() == *size)
            && crate::ops::identity(path).as_deref() == Some(id.as_str())
    })
}

/// The copies of the reviewed groups that may be moved, each group checked as a whole and
/// all of them together: a copy that stays is a different file from every copy that goes
/// (not the same path under another spelling, not a hard link to it, in this group or any
/// other), is still there with the size it was listed with, and, when `verify` is set,
/// holds the same bytes as each copy that goes. The figures sent back from the view are
/// not trusted for that last step; the files are compared. A group that fails any of it
/// moves nothing. `skip_move` is the deny list for what moves (and where the bin can take
/// it); `skip_keep` only for what stays, which needs no bin. Returns the copies to move and
/// how many were held back.
pub fn removable_copies(
    groups: &[Reviewed],
    skip_move: &impl Fn(&Path, u64) -> bool,
    skip_keep: &impl Fn(&Path, u64) -> bool,
    verify: bool,
) -> (Vec<Removal>, usize) {
    // Everything any group is about to remove: no group may keep one of these.
    let going: Vec<&String> = groups.iter().flat_map(|g| &g.remove).collect();
    let going_places: Vec<String> = going.iter().map(|p| same_spelling(Path::new(p))).collect();
    let going_ids: Vec<String> = going
        .iter()
        .filter_map(|p| crate::ops::identity(Path::new(p)))
        .collect();
    let mut ok = Vec::new();
    let mut held_back = 0;
    for g in groups {
        let marked: Vec<&BigFile> = g
            .files
            .iter()
            .filter(|f| g.remove.contains(&f.path))
            .collect();
        // Each removal named once, and only copies of this group, each one a file whose
        // identity can be read (else it can't be told from a link to a copy that stays).
        if marked.is_empty()
            || g.remove.len() != marked.len()
            || marked
                .iter()
                .any(|f| crate::ops::identity(Path::new(&f.path)).is_none())
        {
            held_back += marked.len().max(g.remove.len());
            continue;
        }
        let keepers: Vec<&BigFile> = g
            .files
            .iter()
            .filter(|f| !g.remove.contains(&f.path))
            .filter(|f| {
                let path = Path::new(&f.path);
                !going_places.contains(&same_spelling(path))
                    && crate::ops::identity(path).is_some_and(|id| !going_ids.contains(&id))
                    && on_disk(path, skip_keep).is_some_and(|(size, _)| size == f.size)
            })
            .collect();
        let proven = |gone: &BigFile| {
            !verify
                || keepers
                    .iter()
                    .any(|k| same_bytes(Path::new(&k.path), Path::new(&gone.path), &|| false))
        };
        if keepers.is_empty() {
            held_back += marked.len();
            continue;
        }
        let still = still_as_listed(&g.files, skip_move);
        for f in marked {
            if still.iter().any(|s| s.path == f.path) && proven(f) {
                ok.push(Removal {
                    file: BigFile {
                        path: f.path.clone(),
                        size: f.size,
                        modified: f.modified,
                    },
                    keepers: keepers
                        .iter()
                        .filter_map(|k| {
                            let id = crate::ops::identity(Path::new(&k.path))?;
                            Some((k.path.clone(), id, k.size))
                        })
                        .collect(),
                });
            } else {
                held_back += 1;
            }
        }
    }
    (ok, held_back)
}

/// `items` with repeats taken out (the same file under two spellings or two lists) and
/// without any file that is, or is a link to, one of the `keep` paths: the copies that
/// groups of duplicates keep. Moving one of those with a plain list would take the copy
/// a group relies on.
pub fn without_aliases<'a>(items: Vec<&'a BigFile>, keep: &[String]) -> Vec<&'a BigFile> {
    let kept_places: Vec<String> = keep.iter().map(|p| same_spelling(Path::new(p))).collect();
    let kept_ids: Vec<String> = keep
        .iter()
        .filter_map(|p| crate::ops::identity(Path::new(p)))
        .collect();
    let mut seen_places: Vec<String> = Vec::new();
    let mut seen_ids: Vec<String> = Vec::new();
    items
        .into_iter()
        .filter(|f| {
            let path = Path::new(&f.path);
            let place = same_spelling(path);
            let id = crate::ops::identity(path);
            let is_kept =
                kept_places.contains(&place) || id.as_ref().is_some_and(|id| kept_ids.contains(id));
            let repeat =
                seen_places.contains(&place) || id.as_ref().is_some_and(|id| seen_ids.contains(id));
            if is_kept || repeat {
                return false;
            }
            seen_places.push(place);
            seen_ids.extend(id);
            true
        })
        .collect()
}

/// The suffix of a copy in progress (`name.folio-partial`, or just `.folio-partial` for a
/// name too long to add it to). A copy is built under it and only given its real name once
/// complete, so one left behind is an unfinished copy.
pub const PARTIAL_SUFFIX: &str = crate::ops::PARTIAL;

/// Whether `name` is the name of an unfinished copy.
pub fn is_partial_name(name: &str) -> bool {
    name.to_lowercase().ends_with(PARTIAL_SUFFIX)
}

/// The shortest time an unfinished copy must have been left alone to be suggested, whatever
/// it is asked: a copy that is still going keeps changing it.
pub const MIN_PARTIAL_DAYS: u32 = 3;
/// Most entries looked at to size and date one unfinished copy; a bigger tree is not judged.
const PARTIAL_WALK: usize = 100_000;

/// An unfinished copy left behind: a file, or a folder with everything in it.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PartialCopy {
    pub path: String,
    /// All of it, in bytes.
    pub size: u64,
    /// When anything in it last changed (seconds since 1970).
    pub modified: i64,
    pub is_dir: bool,
}

/// When an entry last had anything done to it: the later of its change time and its creation
/// time. A copy keeps the change time of what it copies, so a file made a moment ago can look
/// old by that alone; the creation time is when it came to be here.
pub fn touched(meta: &std::fs::Metadata) -> i64 {
    let secs = |t: std::io::Result<std::time::SystemTime>| {
        t.ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(i64::MAX, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
    };
    // If either time is unknown the entry counts as touched now: it is never called old on
    // a guess.
    secs(meta.modified()).max(secs(meta.created()))
}

impl From<&PartialCopy> for BigFile {
    fn from(p: &PartialCopy) -> Self {
        Self {
            path: p.path.clone(),
            size: p.size,
            modified: p.modified,
        }
    }
}

/// The size of `path` and the last time anything in it was touched (`stamp` of every file
/// and folder in it), links not followed. `None` if it is a link, can't be read, or is too
/// big a tree to judge.
fn measure(path: &Path, stamp: &dyn Fn(&std::fs::Metadata) -> i64) -> Option<(u64, i64)> {
    let secs = stamp;
    let meta = std::fs::symlink_metadata(path).ok()?;
    if meta.file_type().is_symlink() || cloud_only_attributes(&meta) {
        return None;
    }
    if meta.is_file() {
        return Some((meta.len(), secs(&meta)));
    }
    if !meta.is_dir() {
        return None;
    }
    let (mut size, mut newest, mut seen) = (0u64, secs(&meta), 0usize);
    let mut todo = vec![path.to_path_buf()];
    while let Some(dir) = todo.pop() {
        for entry in std::fs::read_dir(&dir).ok()?.flatten() {
            seen += 1;
            if seen > PARTIAL_WALK {
                return None;
            }
            let kind = entry.file_type().ok()?;
            let meta = entry.metadata().ok()?;
            // One file only in the cloud spoils the whole: its size frees nothing here and
            // moving the folder would remove it from the cloud too. Nothing is opened to
            // find out; the attributes come with the listing.
            if cloud_only_attributes(&meta) {
                return None;
            }
            newest = newest.max(secs(&meta));
            if kind.is_dir() {
                todo.push(entry.path());
            } else if kind.is_file() {
                size = size.saturating_add(meta.len());
            }
        }
    }
    Some((size, newest))
}

/// An unfinished copy as it is on disk now: its size and when anything in it last changed,
/// or `None` if it is gone, is not named like one, is a link, or `skip` refuses it, as listed
/// or once its links are resolved (as for [`on_disk`]).
fn partial_on_disk(
    path: &Path,
    skip: &impl Fn(&Path, u64) -> bool,
    stamp: &dyn Fn(&std::fs::Metadata) -> i64,
) -> Option<(u64, i64)> {
    if !is_partial_name(&path.file_name()?.to_string_lossy()) {
        return None;
    }
    let (size, newest) = measure(path, stamp)?;
    if skip(path, size) {
        return None;
    }
    let resolved = std::fs::canonicalize(path).ok()?;
    let same_drive = drive_of(path).is_none() || drive_of(path) == drive_of(&resolved);
    (same_drive && !skip(&resolved, size)).then_some((size, newest))
}

/// Unfinished copies among the scanned folders and files that nothing has touched for
/// `older_than_days`, biggest first. The scan cache says where they are; each is measured on
/// disk again. One inside another is left to the outer one. A copy that is still running is
/// not told apart here: the caller must not ask while one is (see `Queue::copy_running`).
pub fn stale_partials(
    scanner: &Scanner,
    skip: impl Fn(&Path, u64) -> bool,
    stamp: &dyn Fn(&std::fs::Metadata) -> i64,
    older_than_days: u32,
    limit: usize,
    now: i64,
) -> rusqlite::Result<Vec<PartialCopy>> {
    let before = now - i64::from(older_than_days.max(MIN_PARTIAL_DAYS)) * DAY;
    let mut candidates = scanner.partial_candidates(5000)?;
    candidates.sort();
    let mut found: Vec<PartialCopy> = Vec::new();
    let mut outer: Vec<String> = Vec::new();
    for path in candidates {
        let place = same_spelling(Path::new(&path));
        if outer.iter().any(|o| place.starts_with(&format!("{o}/"))) {
            continue;
        }
        let is_dir = Path::new(&path).is_dir();
        if let Some((size, newest)) = partial_on_disk(Path::new(&path), &skip, stamp)
            && newest <= before
        {
            outer.push(place);
            found.push(PartialCopy {
                path,
                size,
                modified: newest,
                is_dir,
            });
        }
    }
    found.sort_by(|a, b| b.size.cmp(&a.size).then_with(|| a.path.cmp(&b.path)));
    found.truncate(limit.min(MAX_LIST));
    Ok(found)
}

/// The items of a list shown earlier that are still unfinished copies exactly as listed (same
/// size, nothing changed since) and have still been left alone for the minimum time. Anything
/// else is not moved.
pub fn still_partials<'a>(
    items: &'a [PartialCopy],
    skip: &impl Fn(&Path, u64) -> bool,
    stamp: &dyn Fn(&std::fs::Metadata) -> i64,
    now: i64,
) -> Vec<&'a PartialCopy> {
    let before = now - i64::from(MIN_PARTIAL_DAYS) * DAY;
    items
        .iter()
        .filter(|f| {
            partial_on_disk(Path::new(&f.path), skip, stamp).is_some_and(|(size, newest)| {
                size == f.size && newest <= f.modified && newest <= before
            })
        })
        .collect()
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
    const MB_LEN: usize = 1 << 20;

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
            "C:\\Users\\me\\anaconda3\\Library\\bin\\mkl_core.dll",
            "C:\\Users\\me\\scoop\\apps\\tool\\1.0\\tool.exe",
            "C:\\Users\\me\\.rustup\\toolchains\\stable\\bin\\rustc_driver.dll",
            "C:\\Users\\me\\.ollama\\models\\blobs\\sha256-abc",
            "C:\\Users\\me\\Library\\Mail\\x",
            "D:\\Epic Games\\Fortnite\\Content\\Paks\\a.pak",
            "D:\\GOG Games\\Witcher 3\\content\\a.bin",
            "C:\\Riot Games\\League of Legends\\Game\\DATA\\a.wad",
            "D:\\FileHistory\\me\\PC\\Data\\C\\Users\\me\\Pictures\\photo.jpg",
            "D:\\WindowsImageBackup\\PC\\Backup 2020\\disk.bin",
            "C:\\Users\\me\\VirtualBox VMs\\Win\\Win.vdi",
            "D:\\Disks\\machine.vhdx",
            "C:\\Users\\me\\Documents\\Outlook Files\\archive.pst",
            "C:\\Program Files (Arm)\\App\\data.pak",
            "C:\\Windows.old.000\\Users\\x\\old.bin",
            "D:\\Profiles\\other\\NTUSER.DAT",
            "D:\\hiberfil.sys",
            "E:\\data\\pagefile.sys",
            "/nix/store/abc/lib/x.so",
            "/Volumes/Ext/.Trashes/501/x.iso",
            "/media/me/usb/.Trash-1000/files/x.iso",
            "/Volumes/TM/Backups.backupdb/x",
            "/Users/me/Parallels/Win.pvm/disk.hdd/a.hds",
            "/Volumes/Ext/B.sparsebundle/bands/1",
            "/Users/me/Movies/M.imovielibrary/x",
            "/Users/me/Docs/Talk.key/Data/img.jpg",
            "/home/me/.thunderbird/p.default/Mail/Local Folders/Archives",
            "/home/me/snap/firefox/common/x",
            "/Volumes/OldMac/Users/bob/Library/x",
            "/Volumes/OldMac/System/Library/x",
            "\\\\wsl.localhost\\Ubuntu\\usr\\lib\\x.so",
            "\\\\server\\share\\file.bin",
            "\\\\.\\C:\\Users\\me\\NTUSER.DAT",
            "\\\\?\\UNC\\srv\\share\\x.bin",
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
            "C:\\Users\\me\\Videos\\Windows Movie Maker\\x.wmv",
            "C:\\Users\\me\\Documents\\programs-notes.docx",
            "\\\\?\\D:\\Backups\\disk.img",
            "/home/me/Videos/library.mkv",
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
    fn only_old_installers_and_archives_in_downloads_are_suggested() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("old").join("deeper");
        std::fs::create_dir_all(&nested).unwrap();
        put(dir.path(), "setup.EXE", 10, 200);
        put(dir.path(), "photos.zip", 30, 100);
        put(&nested, "tool.7z", 20, 400);
        put(dir.path(), "fresh.zip", 50, 5);
        put(dir.path(), "report.pdf", 60, 400);
        put(dir.path(), "movie.mkv", 70, 400);
        let now = crate::journal::unix_now();
        let found = old_downloads(dir.path(), |_, _| false, |_| 0, 90, 100, now);
        let names: Vec<String> = found
            .iter()
            .map(|f| {
                Path::new(&f.path)
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        // Biggest first; the new archive, the pdf and the video are not offered.
        assert_eq!(names, ["photos.zip", "tool.7z", "setup.EXE"]);
        assert_eq!(found[0].size, 30);
    }

    #[test]
    fn a_file_that_arrived_recently_is_not_old_whatever_its_change_date() {
        let dir = tempfile::tempdir().unwrap();
        put(dir.path(), "extracted.zip", 10, 400);
        put(dir.path(), "ancient.zip", 20, 400);
        let now = crate::journal::unix_now();
        let arrival = |p: &Path| {
            if p.ends_with("extracted.zip") {
                now - 60
            } else {
                0
            }
        };
        let found = old_downloads(dir.path(), |_, _| false, arrival, 90, 100, now);
        assert_eq!(found.len(), 1);
        assert!(found[0].path.ends_with("ancient.zip"));
        // And the real thing: a file made just now has just arrived.
        assert!(arrived(&dir.path().join("extracted.zip")) >= now - 5);
    }

    #[test]
    fn an_unpacked_folder_and_the_depth_limit_are_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let unpacked = dir.path().join("PortableTool");
        let plain = dir.path().join("setups");
        let deep = plain.join("a").join("b").join("c").join("d");
        std::fs::create_dir_all(&unpacked).unwrap();
        std::fs::create_dir_all(&deep).unwrap();
        put(&unpacked, "tool.exe", 10, 400);
        put(&unpacked, "tool.dll", 10, 400);
        put(&plain, "installer.msi", 10, 400);
        // A jar below the top level is the program itself; its folder is left whole.
        let server = dir.path().join("mc-server");
        std::fs::create_dir_all(&server).unwrap();
        put(&server, "server.jar", 10, 400);
        put(&server, "setup.zip", 10, 400);
        put(&deep, "too-deep.zip", 10, 400);
        put(dir.path(), "top.jar", 10, 400);
        let now = crate::journal::unix_now();
        let found = old_downloads(dir.path(), |_, _| false, |_| 0, 90, 100, now);
        let mut names: Vec<String> = found
            .iter()
            .map(|f| {
                Path::new(&f.path)
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        names.sort();
        assert_eq!(names, ["installer.msi", "top.jar"]);
    }

    #[test]
    fn an_age_below_the_floor_is_raised_to_it() {
        let dir = tempfile::tempdir().unwrap();
        put(dir.path(), "new.zip", 10, 3);
        put(dir.path(), "old.zip", 10, 30);
        let now = crate::journal::unix_now();
        let found = old_downloads(dir.path(), |_, _| false, |_| 0, 0, 100, now);
        assert_eq!(found.len(), 1);
        assert!(found[0].path.ends_with("old.zip"));
    }

    #[test]
    fn a_downloads_folder_that_is_a_drive_root_or_the_profile_is_refused() {
        let home = Path::new("C:\\Users\\me");
        assert!(!downloads_folder_ok(Path::new("D:\\"), Some(home)));
        assert!(!downloads_folder_ok(Path::new("C:\\Users\\me"), Some(home)));
        assert!(!downloads_folder_ok(Path::new("C:\\Users"), Some(home)));
        assert!(!downloads_folder_ok(
            Path::new("C:\\Windows\\Temp"),
            Some(home)
        ));
    }

    #[test]
    fn downloads_search_respects_the_deny_list_and_the_limit() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..5 {
            put(dir.path(), &format!("a{i}.zip"), 10 + i, 400);
        }
        put(dir.path(), "keep.zip", 99, 400);
        let now = crate::journal::unix_now();
        let skip = |p: &Path, _: u64| p.ends_with("keep.zip");
        let found = old_downloads(dir.path(), skip, |_| 0, 90, 3, now);
        assert_eq!(found.len(), 3);
        assert!(found.iter().all(|f| !f.path.ends_with("keep.zip")));
        assert_eq!(found[0].size, 14);
    }

    fn put_bytes(dir: &Path, name: &str, byte: u8, len: usize, days_old: u64) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, vec![byte; len]).unwrap();
        let when = std::time::SystemTime::now() - std::time::Duration::from_secs(days_old * 86_400);
        std::fs::File::options()
            .write(true)
            .open(&p)
            .unwrap()
            .set_modified(when)
            .unwrap();
        p
    }

    #[test]
    fn copies_are_grouped_newest_first_and_look_alikes_are_not() {
        let dir = tempfile::tempdir().unwrap();
        let scanner = scanned(&dir, |sub| {
            let a = sub.join("a");
            std::fs::create_dir(&a).unwrap();
            put_bytes(sub, "copy-old.bin", 7, 2 * MB_LEN, 300);
            put_bytes(&a, "copy-new.bin", 7, 2 * MB_LEN, 10);
            // The same size and the same start and end, but not the same bytes.
            let mut odd = vec![7u8; 2 * MB_LEN];
            odd[MB_LEN] = 8;
            std::fs::write(sub.join("lookalike.bin"), odd).unwrap();
            // The same size, wholly different.
            put_bytes(sub, "other.bin", 9, 2 * MB_LEN, 5);
        });
        let found = duplicates_with(&scanner, |_, _| false, MB, &|| false).unwrap();
        assert!(found.complete);
        assert_eq!(found.groups.len(), 1);
        let names: Vec<String> = found.groups[0]
            .files
            .iter()
            .map(|f| {
                Path::new(&f.path)
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        assert_eq!(names, ["copy-new.bin", "copy-old.bin"]);
    }

    #[test]
    fn a_hard_link_is_not_a_second_copy() {
        let dir = tempfile::tempdir().unwrap();
        let scanner = scanned(&dir, |sub| {
            let a = put_bytes(sub, "a.bin", 3, 2 * MB_LEN, 50);
            std::fs::hard_link(&a, sub.join("link.bin")).unwrap();
        });
        let found = duplicates_with(&scanner, |_, _| false, MB, &|| false).unwrap();
        assert_eq!(found.groups.len(), 0);
    }

    #[test]
    fn protected_copies_are_never_grouped() {
        let dir = tempfile::tempdir().unwrap();
        let scanner = scanned(&dir, |sub| {
            put_bytes(sub, "a.bin", 3, 2 * MB_LEN, 50);
            put_bytes(sub, "b.bin", 3, 2 * MB_LEN, 40);
        });
        let found = duplicates_with(&scanner, |p, _| p.ends_with("b.bin"), MB, &|| false).unwrap();
        assert_eq!(found.groups.len(), 0);
    }

    fn reviewed(files: &[&Path], remove: &[&Path]) -> Reviewed {
        Reviewed {
            files: files
                .iter()
                .map(|p| {
                    let m = std::fs::metadata(p).unwrap();
                    BigFile {
                        path: p.to_string_lossy().into_owned(),
                        size: m.len(),
                        modified: m
                            .modified()
                            .unwrap()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap()
                            .as_secs()
                            .try_into()
                            .unwrap(),
                    }
                })
                .collect(),
            remove: remove
                .iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect(),
        }
    }

    #[test]
    fn a_plain_list_never_takes_the_copy_a_group_keeps_or_the_same_file_twice() {
        let dir = tempfile::tempdir().unwrap();
        let a = put_bytes(dir.path(), "a.bin", 1, 100, 30);
        let b = put_bytes(dir.path(), "b.bin", 1, 100, 20);
        let link = dir.path().join("b-link.bin");
        std::fs::hard_link(&b, &link).unwrap();
        let listed = reviewed(&[&a, &b, &link], &[]).files;
        let items: Vec<&BigFile> = listed.iter().chain(listed.iter().take(1)).collect();
        let keep = vec![b.to_string_lossy().into_owned()];
        let left = without_aliases(items, &keep);
        // `a` once (it was listed twice); `b` and its hard link are the copy that stays.
        assert_eq!(left.len(), 1);
        assert!(left[0].path.ends_with("a.bin"));
    }

    #[test]
    fn a_group_never_loses_all_its_copies() {
        let dir = tempfile::tempdir().unwrap();
        let a = put_bytes(dir.path(), "a.bin", 1, 100, 30);
        let b = put_bytes(dir.path(), "b.bin", 1, 100, 20);
        let c = put_bytes(dir.path(), "c.bin", 1, 100, 10);
        let none = |_: &Path, _: u64| false;
        // Removing every copy moves nothing.
        let (ok, held) = removable_copies(
            &[reviewed(&[&a, &b, &c], &[&a, &b, &c])],
            &none,
            &none,
            true,
        );
        assert_eq!((ok.len(), held), (0, 3));
        // Removing all but one is fine.
        let (ok, held) =
            removable_copies(&[reviewed(&[&a, &b, &c], &[&a, &b])], &none, &none, true);
        assert_eq!((ok.len(), held), (2, 0));
        // Removing something that is not in the group is refused.
        let (ok, held) = removable_copies(&[reviewed(&[&a, &b], &[&c])], &none, &none, true);
        assert_eq!((ok.len(), held), (0, 1));
    }

    #[test]
    fn groups_never_keep_what_another_group_removes() {
        let dir = tempfile::tempdir().unwrap();
        let a = put_bytes(dir.path(), "a.bin", 1, 100, 30);
        let b = put_bytes(dir.path(), "b.bin", 1, 100, 20);
        let none = |_: &Path, _: u64| false;
        // Two groups over the same two files, each keeping what the other removes.
        let first = reviewed(&[&a, &b], &[&a]);
        let second = reviewed(&[&a, &b], &[&b]);
        let (ok, _) = removable_copies(&[first, second], &none, &none, true);
        assert_eq!(ok.len(), 0);
    }

    #[test]
    fn a_hard_link_of_a_removed_copy_is_not_a_copy_that_stays() {
        let dir = tempfile::tempdir().unwrap();
        let a = put_bytes(dir.path(), "a.bin", 1, 100, 30);
        let link = dir.path().join("link.bin");
        std::fs::hard_link(&a, &link).unwrap();
        let none = |_: &Path, _: u64| false;
        let (ok, held) = removable_copies(&[reviewed(&[&a, &link], &[&a])], &none, &none, true);
        assert_eq!((ok.len(), held), (0, 1));
    }

    #[test]
    fn a_path_spelled_another_way_is_not_a_copy_that_stays() {
        let dir = tempfile::tempdir().unwrap();
        let a = put_bytes(dir.path(), "A.bin", 1, 100, 30);
        let alias = PathBuf::from(a.to_string_lossy().to_uppercase().replace("A.BIN", "a.bin"));
        if !alias.exists() {
            return; // a case-sensitive volume: no such alias
        }
        let none = |_: &Path, _: u64| false;
        let group = Reviewed {
            files: reviewed(&[&a], &[])
                .files
                .into_iter()
                .chain(reviewed(&[&alias], &[]).files)
                .collect(),
            remove: vec![a.to_string_lossy().into_owned()],
        };
        let (ok, _) = removable_copies(&[group], &none, &none, true);
        assert_eq!(ok.len(), 0);
    }

    #[test]
    fn a_copy_that_no_longer_matches_byte_for_byte_blocks_the_move() {
        let dir = tempfile::tempdir().unwrap();
        let a = put_bytes(dir.path(), "a.bin", 1, 100, 30);
        let b = put_bytes(dir.path(), "b.bin", 1, 100, 20);
        let none = |_: &Path, _: u64| false;
        let group = reviewed(&[&a, &b], &[&a]);
        // The copy to keep is replaced by other bytes of the same size, with an older date.
        std::fs::write(&b, vec![9u8; 100]).unwrap();
        std::fs::File::options()
            .write(true)
            .open(&b)
            .unwrap()
            .set_modified(std::time::SystemTime::now() - std::time::Duration::from_hours(21_600))
            .unwrap();
        let (ok, held) = removable_copies(&[group], &none, &none, true);
        assert_eq!((ok.len(), held), (0, 1));
    }

    #[test]
    fn the_copy_kept_by_default_is_not_the_one_in_downloads() {
        assert!(passing_through("C:\\Users\\me\\Downloads\\a.zip"));
        assert!(passing_through("/home/me/Desktop/a.zip"));
        assert!(!passing_through("C:\\Users\\me\\Documents\\a.zip"));
    }

    #[test]
    fn nothing_moves_when_the_copy_to_keep_is_gone_or_changed() {
        let dir = tempfile::tempdir().unwrap();
        let a = put_bytes(dir.path(), "a.bin", 1, 100, 30);
        let b = put_bytes(dir.path(), "b.bin", 1, 100, 20);
        let none = |_: &Path, _: u64| false;
        let group = reviewed(&[&a, &b], &[&a]);
        std::fs::remove_file(&b).unwrap();
        let (ok, held) = removable_copies(&[group], &none, &none, true);
        assert_eq!((ok.len(), held), (0, 1));
        // The kept copy rewritten with different content (a new size) after the review.
        let b = put_bytes(dir.path(), "b.bin", 1, 100, 20);
        let group = reviewed(&[&a, &b], &[&a]);
        std::fs::write(&b, vec![2u8; 50]).unwrap();
        let (ok, _) = removable_copies(&[group], &none, &none, true);
        assert_eq!(ok.len(), 0);
    }

    /// Dates by change time only, and none for a folder: a folder made a moment ago in a
    /// test is not the file that is old in it.
    fn by_change_time(meta: &std::fs::Metadata) -> i64 {
        if meta.is_dir() {
            return 0;
        }
        meta.modified()
            .unwrap()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            .try_into()
            .unwrap()
    }

    #[test]
    fn other_lists_never_offer_an_unfinished_copy_or_what_is_in_one() {
        let dir = tempfile::tempdir().unwrap();
        let scanner = scanned(&dir, |sub| {
            // Old and big, as a copy that keeps the source's change date would look.
            put_bytes(sub, "movie.mkv.folio-partial", 1, 2 * MB_LEN, 900);
            let tree = sub.join("Photos.folio-partial");
            std::fs::create_dir_all(&tree).unwrap();
            put_bytes(&tree, "inside.bin", 2, 2 * MB_LEN, 900);
            put_bytes(sub, "ordinary.bin", 3, 2 * MB_LEN, 900);
            // The same bytes as the partials, so a duplicate search would pair them.
            put_bytes(sub, "copy-of-movie.mkv", 1, 2 * MB_LEN, 900);
            put_bytes(sub, "copy-of-inside.bin", 2, 2 * MB_LEN, 900);
        });
        let now = crate::journal::unix_now();
        let large = large_old(&scanner, |_, _| false, MB, 30, 100, now).unwrap();
        let mut names: Vec<String> = large
            .iter()
            .map(|f| {
                Path::new(&f.path)
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        names.sort();
        assert_eq!(
            names,
            ["copy-of-inside.bin", "copy-of-movie.mkv", "ordinary.bin"]
        );
        let found = duplicates_with(&scanner, |_, _| false, MB, &|| false).unwrap();
        assert_eq!(found.groups.len(), 0);
    }

    #[test]
    fn a_file_made_just_now_counts_as_touched_now_whatever_its_change_date() {
        let dir = tempfile::tempdir().unwrap();
        let path = put_bytes(dir.path(), "x.bin", 1, 10, 400);
        let meta = std::fs::metadata(&path).unwrap();
        // Changed 400 days ago, created a moment ago: touched a moment ago.
        assert!(touched(&meta) >= crate::journal::unix_now() - 60);
    }

    #[test]
    fn unfinished_copies_are_named_by_their_suffix() {
        assert!(is_partial_name("report.pdf.folio-partial"));
        assert!(is_partial_name(".folio-partial"));
        assert!(is_partial_name("Photos.FOLIO-PARTIAL"));
        assert!(!is_partial_name("folio-partial.txt"));
        assert!(!is_partial_name("notes.partial"));
    }

    #[test]
    fn only_old_unfinished_copies_are_suggested_with_their_whole_size() {
        let dir = tempfile::tempdir().unwrap();
        let scanner = scanned(&dir, |sub| {
            put_bytes(sub, "movie.mkv.folio-partial", 1, 2 * MB_LEN, 40);
            put_bytes(sub, ".folio-partial", 2, 2 * MB_LEN, 40);
            let tree = sub.join("Photos.folio-partial");
            std::fs::create_dir_all(tree.join("2019")).unwrap();
            put_bytes(&tree, "a.jpg", 3, 100, 40);
            put_bytes(&tree.join("2019"), "b.jpg", 3, 300, 41);
            // Another one inside: left to the outer.
            let inner = tree.join("inner.folio-partial");
            std::fs::create_dir(&inner).unwrap();
            put_bytes(&inner, "c.jpg", 3, 50, 40);
            // Still being written, a real file, and a name that only looks like one.
            put_bytes(sub, "fresh.zip.folio-partial", 4, 2 * MB_LEN, 1);
            put_bytes(sub, "real.zip", 5, 2 * MB_LEN, 400);
            put_bytes(sub, "folio-partial.txt", 6, 2 * MB_LEN, 400);
        });
        let now = crate::journal::unix_now();
        let found = stale_partials(&scanner, |_, _| false, &by_change_time, 7, 100, now).unwrap();
        let names: Vec<(String, u64, bool)> = found
            .iter()
            .map(|p| {
                (
                    Path::new(&p.path)
                        .file_name()
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                    p.size,
                    p.is_dir,
                )
            })
            .collect();
        assert_eq!(
            names,
            [
                // Equal sizes go by path.
                (".folio-partial".to_owned(), 2 * MB, false),
                ("movie.mkv.folio-partial".to_owned(), 2 * MB, false),
                ("Photos.folio-partial".to_owned(), 450, true),
            ]
        );
    }

    #[test]
    fn a_partial_copy_that_changed_after_it_was_listed_or_is_protected_is_not_moved() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("Videos");
        let scanner = scanned(&dir, |sub| {
            let tree = sub.join("Photos.folio-partial");
            std::fs::create_dir_all(&tree).unwrap();
            put_bytes(&tree, "a.jpg", 3, MB_LEN, 40);
            put_bytes(sub, "movie.mkv.folio-partial", 1, MB_LEN, 40);
        });
        let now = crate::journal::unix_now();
        let listed = stale_partials(&scanner, |_, _| false, &by_change_time, 7, 100, now).unwrap();
        assert_eq!(listed.len(), 2);
        // Another file lands in the folder after the list was made.
        put_bytes(&sub.join("Photos.folio-partial"), "late.jpg", 3, 10, 0);
        let ok = still_partials(&listed, &|_, _| false, &by_change_time, now);
        assert_eq!(ok.len(), 1);
        assert!(ok[0].path.ends_with("movie.mkv.folio-partial"));
        // And a deny list that has come to refuse them stops both.
        assert_eq!(
            still_partials(&listed, &|_, _| true, &by_change_time, now).len(),
            0
        );
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

    #[test]
    fn a_search_that_is_replaced_says_it_is_incomplete_and_counts_nothing_unreadable() {
        let dir = tempfile::tempdir().unwrap();
        let scanner = scanned(&dir, |sub| {
            put_bytes(sub, "a.bin", 7, 2 * MB_LEN, 300);
            put_bytes(sub, "b.bin", 7, 2 * MB_LEN, 200);
        });
        let found = duplicates_with(&scanner, |_, _| false, MB, &|| true).unwrap();
        assert!(!found.complete);
        assert_eq!((found.groups.len(), found.unreadable), (0, 0));
    }

    #[test]
    fn a_kept_copy_that_vanishes_or_is_swapped_after_checking_is_noticed() {
        let dir = tempfile::tempdir().unwrap();
        let a = put_bytes(dir.path(), "a.bin", 1, 100, 30);
        let b = put_bytes(dir.path(), "b.bin", 1, 100, 20);
        let none = |_: &Path, _: u64| false;
        let (ok, _) = removable_copies(&[reviewed(&[&a, &b], &[&a])], &none, &none, true);
        assert_eq!(ok.len(), 1);
        assert!(keepers_still_there(&ok[0]));
        // A different file of the same size put at the kept path is not the kept copy.
        std::fs::remove_file(&b).unwrap();
        std::fs::write(&b, vec![1u8; 100]).unwrap();
        assert!(!keepers_still_there(&ok[0]));
        std::fs::remove_file(&b).unwrap();
        assert!(!keepers_still_there(&ok[0]));
    }
}
