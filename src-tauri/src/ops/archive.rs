//! Zip archives: files and folders packed into one, and one unpacked into a new folder.
//!
//! Both build their result under a temporary name and give it its real name only when it is
//! complete, so a failed or cancelled job leaves nothing half-made at the real name, and
//! only what this module made is ever removed. Both report progress, pause and cancel
//! through [`progress`](super::progress), like a copy.

use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, BufWriter, ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, DateTime, ZipArchive, ZipWriter};

use crate::civil::{civil_from_days, days_from_civil};
use crate::cleanup::is_cloud_only;
use crate::platform::{self, clock};

use super::{PARTIAL, made, progress, validate_name};

/// Bytes moved between a checkpoint and the next.
const CHUNK: usize = 64 * 1024;
/// A file this big is written with the 64-bit zip fields, which a reader of 32-bit ones
/// can't open; smaller files stay readable by everything.
const BIG_FILE: u64 = u32::MAX as u64 / 2;

/// One thing going into an archive.
struct Item {
    path: PathBuf,
    /// Its name inside the archive: `/` between parts, relative to the folder it came from.
    name: String,
    is_dir: bool,
    size: u64,
    modified: Option<SystemTime>,
}

/// What [`compress`] made.
#[derive(Debug)]
pub struct Packed {
    pub path: PathBuf,
    /// Links, and items whose names are not text, that were left out: an archive holds the
    /// files themselves.
    pub skipped: usize,
}

/// What [`extract`] made.
#[derive(Debug)]
pub struct Unpacked {
    pub path: PathBuf,
    pub files: usize,
    /// Entries that were left out: links, names that point outside the folder, and names
    /// Windows can't keep.
    pub skipped: usize,
}

fn name_of(path: &Path) -> String {
    path.file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
}

/// Everything in `sources`, folders first-to-last as they are met. Links, and names that are
/// not text, are counted and left out; a folder that can't be read, or a file that is only in
/// the cloud, is an error.
fn gather(sources: &[PathBuf]) -> io::Result<(Vec<Item>, usize)> {
    let mut items = Vec::new();
    let mut links = 0;
    let mut cloud = 0_usize;
    let mut work: Vec<(PathBuf, String)> = sources
        .iter()
        .rev()
        .map(|p| (p.clone(), name_of(p)))
        .collect();
    while let Some((path, name)) = work.pop() {
        progress::checkpoint()?;
        let meta = fs::symlink_metadata(&path)?;
        if meta.file_type().is_symlink() {
            links += 1;
        } else if meta.is_dir() {
            let mut children = fs::read_dir(&path)?
                .map(|e| e.map(|e| e.file_name()))
                .collect::<io::Result<Vec<_>>>()?;
            children.sort();
            items.push(Item {
                path: path.clone(),
                name: name.clone(),
                is_dir: true,
                size: 0,
                modified: meta.modified().ok(),
            });
            for child in children.into_iter().rev() {
                // A name that isn't text can't be stored as it is, and a lossy one could
                // collide with another.
                let Some(text) = child.to_str() else {
                    links += 1;
                    continue;
                };
                work.push((path.join(&child), format!("{name}/{text}")));
            }
        } else if !meta.is_file() {
            // A pipe or device would block the read, and holds no file's content.
            links += 1;
        } else if is_cloud_only(&path) {
            // Reading it would download it, however big it is.
            cloud += 1;
        } else {
            items.push(Item {
                path,
                name,
                is_dir: false,
                size: meta.len(),
                modified: meta.modified().ok(),
            });
        }
    }
    if cloud > 0 {
        return Err(io::Error::new(
            ErrorKind::Unsupported,
            format!(
                "{cloud} file{} only in the cloud. Make {} available on this computer first, so Folio doesn't download {}.",
                if cloud == 1 { " is" } else { "s are" },
                if cloud == 1 { "it" } else { "them" },
                if cloud == 1 { "it" } else { "them" },
            ),
        ));
    }
    Ok((items, links))
}

/// A zip timestamp for `time`, in the wall-clock time of the local zone as zip keeps it; none
/// for what a zip can't hold (before 1980).
fn zip_time(time: SystemTime) -> Option<DateTime> {
    let utc = i64::try_from(time.duration_since(UNIX_EPOCH).ok()?.as_secs()).ok()?;
    let secs = utc + clock::offset_at_utc(utc);
    let (year, month, day) = civil_from_days(secs.div_euclid(86_400));
    let rest = secs.rem_euclid(86_400);
    DateTime::from_date_and_time(
        u16::try_from(year).ok()?,
        month,
        day,
        u8::try_from(rest / 3600).ok()?,
        u8::try_from(rest % 3600 / 60).ok()?,
        u8::try_from(rest % 60).ok()?,
    )
    .ok()
}

/// The time of a zip timestamp, which is wall-clock time in the local zone.
fn system_time(stamp: DateTime) -> Option<SystemTime> {
    let days = days_from_civil(i64::from(stamp.year()), stamp.month(), stamp.day());
    let secs = days * 86_400
        + i64::from(stamp.hour()) * 3600
        + i64::from(stamp.minute()) * 60
        + i64::from(stamp.second());
    let utc = secs - clock::offset_at_wall(secs);
    Some(UNIX_EPOCH + Duration::from_secs(u64::try_from(utc).ok()?))
}

fn write_archive(file: &mut File, items: &[Item]) -> io::Result<()> {
    let mut zip = ZipWriter::new(BufWriter::new(file));
    let mut buffer = vec![0_u8; CHUNK];
    for item in items {
        progress::checkpoint()?;
        let mut options = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Deflated)
            .large_file(item.size > BIG_FILE);
        if let Some(stamp) = item.modified.and_then(zip_time) {
            options = options.last_modified_time(stamp);
        }
        if item.is_dir {
            zip.add_directory(&item.name, options)?;
        } else {
            zip.start_file(&item.name, options)?;
            let mut source = File::open(&item.path)?;
            loop {
                progress::checkpoint()?;
                let n = source.read(&mut buffer)?;
                if n == 0 {
                    break;
                }
                zip.write_all(&buffer[..n])?;
                progress::add_bytes(n as u64);
            }
            // A file that changed on the way would be stored torn.
            let now = source.metadata()?;
            if now.len() != item.size || now.modified().ok() != item.modified {
                return Err(io::Error::other(format!(
                    "\"{}\" changed while it was being compressed",
                    item.name
                )));
            }
        }
        progress::add_item();
    }
    zip.finish()?.flush()
}

/// Packs `sources` (files and folders, all from one folder) into a new archive `dir/name`,
/// or the next free name. Links are left out and counted.
pub fn compress(sources: &[PathBuf], dir: &Path, name: &str) -> io::Result<Packed> {
    let (items, skipped) = gather(sources)?;
    if let Some(p) = progress::current() {
        p.set_totals(items.iter().map(|i| i.size).sum(), items.len() as u64);
    }
    let temp = made::stage_with(dir, name, |file| write_archive(file, &items))?;
    let path = made::finish(&temp, dir, name)?;
    Ok(Packed { path, skipped })
}

/// Room kept free beyond what an unpacking needs.
const ROOM_MARGIN: u64 = 64 << 20;

#[expect(
    clippy::cast_precision_loss,
    reason = "a size shown to one decimal place"
)]
fn show_size(bytes: u64) -> String {
    const GB: u64 = 1 << 30;
    const MB: u64 = 1 << 20;
    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    }
}

/// Refuses an unpacking that needs more than `free` bytes (plus a margin), when that is known.
fn check_room(need: u64, free: Option<u64>) -> io::Result<()> {
    match free {
        Some(free) if need.saturating_add(ROOM_MARGIN) > free => Err(io::Error::new(
            ErrorKind::StorageFull,
            format!(
                "This archive unpacks to {} but only {} is free",
                show_size(need),
                show_size(free)
            ),
        )),
        _ => Ok(()),
    }
}

/// The "downloaded from the internet" mark of `archive` (a hidden stream on NTFS), if it has one.
#[cfg(windows)]
fn web_mark(archive: &Path) -> Option<Vec<u8>> {
    fs::read(stream_of(archive)).ok()
}

#[cfg(not(windows))]
fn web_mark(_archive: &Path) -> Option<Vec<u8>> {
    None
}

#[cfg(windows)]
fn stream_of(file: &Path) -> PathBuf {
    let mut path = file.as_os_str().to_owned();
    path.push(":Zone.Identifier");
    PathBuf::from(path)
}

/// A new temporary folder in `dir`, named after `name`, that this call made.
fn stage_folder(dir: &Path, name: &str) -> io::Result<PathBuf> {
    for n in 0..100 {
        let temp = dir.join(format!("{name}.{}-{n}{PARTIAL}", std::process::id()));
        match fs::create_dir(&temp) {
            Ok(()) => return Ok(temp),
            Err(e) if e.kind() == ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::from(ErrorKind::AlreadyExists))
}

/// An entry that is left out still counts as done, so progress reaches its total.
fn skip(skipped: &mut usize, size: u64) {
    *skipped += 1;
    progress::add_bytes(size);
    progress::add_item();
}

/// What a zip entry that can't be read means to the user.
fn unreadable(e: zip::result::ZipError) -> io::Error {
    use zip::result::ZipError;
    match e {
        ZipError::UnsupportedArchive(why) if why.contains("assword") => io::Error::new(
            ErrorKind::Unsupported,
            "This archive is password protected, which Folio can't open",
        ),
        ZipError::UnsupportedArchive(_) | ZipError::CompressionMethodNotSupported(_) => {
            io::Error::new(
                ErrorKind::Unsupported,
                "This archive uses a compression method Folio can't read (it reads stored and deflate)",
            )
        }
        other => other.into(),
    }
}

fn is_link(mode: Option<u32>) -> bool {
    mode.is_some_and(|m| m & 0o170_000 == 0o120_000)
}

/// What an unpacking made, so that a failed one removes that and nothing else.
#[derive(Default)]
struct Made {
    files: Vec<PathBuf>,
    dirs: Vec<PathBuf>,
}

impl Made {
    /// Creates `dir` and the folders above it that are missing, noting each one made.
    fn make_dirs(&mut self, dir: &Path) -> io::Result<()> {
        let mut missing = Vec::new();
        let mut at = Some(dir);
        while let Some(d) = at {
            if fs::symlink_metadata(d).is_ok() {
                break;
            }
            missing.push(d.to_path_buf());
            at = d.parent();
        }
        for d in missing.into_iter().rev() {
            match fs::create_dir(&d) {
                Ok(()) => self.dirs.push(d),
                Err(e) if e.kind() == ErrorKind::AlreadyExists && d.is_dir() => {}
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    /// Removes what was made, files then folders from the deepest, and finally the staging
    /// folder `root`. Anything someone else put there meanwhile stays, and the error says so.
    fn remove(&self, root: &Path, error: io::Error) -> io::Error {
        for file in &self.files {
            let _ = fs::remove_file(file);
        }
        for dir in self.dirs.iter().rev() {
            let _ = fs::remove_dir(dir);
        }
        match fs::remove_dir(root) {
            Ok(()) => error,
            Err(_) => io::Error::new(
                error.kind(),
                format!(
                    "{error}. Something else was put in \"{}\" meanwhile, so it was left there",
                    root.display()
                ),
            ),
        }
    }
}

fn unpack_into(
    zip: &mut ZipArchive<BufReader<File>>,
    root: &Path,
    skipped: &mut usize,
    made: &mut Made,
    zone: Option<&[u8]>,
) -> io::Result<usize> {
    let mut files = 0;
    let mut buffer = vec![0_u8; CHUNK];
    for i in 0..zip.len() {
        progress::checkpoint()?;
        let mut entry = zip.by_index(i).map_err(unreadable)?;
        let declared = entry.size();
        // A name that is absolute or climbs out with `..` would write outside the folder.
        let Some(relative) = entry.enclosed_name() else {
            skip(skipped, declared);
            continue;
        };
        // On Windows a name with `:` (a hidden stream), a device name or a trailing dot would
        // mean something else, or fail.
        let unfit = cfg!(windows)
            && relative
                .components()
                .any(|c| validate_name(&c.as_os_str().to_string_lossy()).is_err());
        if unfit || is_link(entry.unix_mode()) {
            skip(skipped, declared);
            continue;
        }
        let out = root.join(&relative);
        if entry.is_dir() {
            made.make_dirs(&out)?;
            progress::add_item();
            continue;
        }
        if let Some(parent) = out.parent() {
            made.make_dirs(parent)?;
        }
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&out)
            .map_err(|e| {
                let why = if e.kind() == ErrorKind::AlreadyExists {
                    "the archive has another entry with this name (or one that differs only in letter case)".to_owned()
                } else {
                    e.to_string()
                };
                io::Error::new(e.kind(), format!("\"{}\": {why}", relative.display()))
            })?;
        made.files.push(out.clone());
        // The size in the archive bounds what is written, so an entry can't swell past it.
        let mut limited = (&mut entry).take(declared.saturating_add(1));
        let mut written = 0_u64;
        loop {
            progress::checkpoint()?;
            let n = limited.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            file.write_all(&buffer[..n])?;
            written += n as u64;
            progress::add_bytes(n as u64);
        }
        if written != declared {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                format!(
                    "\"{}\" is not the size the archive says",
                    relative.display()
                ),
            ));
        }
        if let Some(time) = entry.last_modified().and_then(system_time) {
            let _ = file.set_modified(time);
        }
        #[cfg(windows)]
        if let Some(mark) = zone {
            // A file that can't take the mark is still unpacked.
            let _ = fs::write(stream_of(&out), mark);
        }
        #[cfg(not(windows))]
        let _ = zone;
        files += 1;
        progress::add_item();
    }
    Ok(files)
}

/// Unpacks the archive at `archive` into a new folder next to it, named after it (or the next
/// free name). Entries that are links or point outside the folder are left out and counted.
pub fn extract(archive: &Path) -> io::Result<Unpacked> {
    let dir = archive
        .parent()
        .ok_or_else(|| io::Error::new(ErrorKind::InvalidInput, "No folder to unpack into"))?;
    // Windows drops a trailing dot or space from a name, so one isn't asked for.
    let stem = archive
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned());
    let name = stem
        .as_deref()
        .map(|s| s.trim_end_matches(['.', ' ']))
        .filter(|s| !s.is_empty())
        .unwrap_or("Archive")
        .to_owned();
    if is_cloud_only(archive) {
        return Err(io::Error::new(
            ErrorKind::Unsupported,
            "This archive is only in the cloud. Make it available on this computer first, so Folio doesn't download it.",
        ));
    }
    let mut zip = ZipArchive::new(BufReader::new(File::open(archive)?))?;
    let mut bytes = 0_u64;
    for i in 0..zip.len() {
        bytes = bytes.saturating_add(zip.by_index_raw(i)?.size());
    }
    // What the archive says it holds is checked against the room there is, before anything is
    // written: an archive can claim far more than its own size suggests.
    check_room(bytes, platform::free_space(dir))?;
    if let Some(p) = progress::current() {
        p.set_totals(bytes, zip.len() as u64);
    }
    // A downloaded archive carries a mark that its files should too (Windows warns before
    // running them).
    let zone = web_mark(archive);
    let temp = stage_folder(dir, &name)?;
    let mut skipped = 0;
    let mut made = Made::default();
    match unpack_into(&mut zip, &temp, &mut skipped, &mut made, zone.as_deref()) {
        Ok(files) => {
            let path = made::finish(&temp, dir, &name)?;
            Ok(Unpacked {
                path,
                files,
                skipped,
            })
        }
        Err(e) => Err(made.remove(&temp, e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn tree() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join("docs/deep")).unwrap();
        fs::create_dir_all(tmp.path().join("docs/empty")).unwrap();
        fs::write(tmp.path().join("docs/a.txt"), "alpha").unwrap();
        fs::write(tmp.path().join("docs/deep/b.txt"), "beta".repeat(5000)).unwrap();
        fs::write(tmp.path().join("single.txt"), "one").unwrap();
        tmp
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut out = Vec::new();
        let mut stack = vec![dir.to_path_buf()];
        while let Some(d) = stack.pop() {
            for e in fs::read_dir(&d).unwrap().flatten() {
                let p = e.path();
                out.push(
                    p.strip_prefix(dir)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/"),
                );
                if p.is_dir() {
                    stack.push(p);
                }
            }
        }
        out.sort();
        out
    }

    #[test]
    fn dates_round_trip_through_a_zip_time() {
        let t = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let back = system_time(zip_time(t).unwrap()).unwrap();
        // Zip seconds have a granularity of two.
        let off = back
            .duration_since(t)
            .unwrap_or_else(|e| e.duration())
            .as_secs();
        assert!(off <= 2, "{off}");
        assert!(
            zip_time(UNIX_EPOCH).is_none(),
            "1970 is before a zip's first year"
        );
    }

    #[test]
    fn a_folder_and_a_file_round_trip_with_empty_folders() {
        let tmp = tree();
        let packed = compress(
            &[tmp.path().join("docs"), tmp.path().join("single.txt")],
            tmp.path(),
            "out.zip",
        )
        .unwrap();
        assert_eq!(packed.path, tmp.path().join("out.zip"));
        assert_eq!(packed.skipped, 0);
        let unpacked = extract(&packed.path).unwrap();
        assert_eq!(unpacked.path, tmp.path().join("out"));
        assert_eq!(unpacked.files, 3);
        assert_eq!(
            names(&unpacked.path),
            [
                "docs",
                "docs/a.txt",
                "docs/deep",
                "docs/deep/b.txt",
                "docs/empty",
                "single.txt"
            ]
        );
        assert_eq!(
            fs::read(unpacked.path.join("docs/deep/b.txt")).unwrap(),
            "beta".repeat(5000).into_bytes()
        );
    }

    #[test]
    fn a_name_that_is_taken_gets_the_next_free_one_and_the_old_file_stays() {
        let tmp = tree();
        fs::write(tmp.path().join("out.zip"), "mine").unwrap();
        let packed = compress(&[tmp.path().join("single.txt")], tmp.path(), "out.zip").unwrap();
        assert_eq!(packed.path, tmp.path().join("out (2).zip"));
        assert_eq!(fs::read(tmp.path().join("out.zip")).unwrap(), b"mine");
        assert!(
            names(tmp.path()).iter().all(|n| !n.ends_with(PARTIAL)),
            "{:?}",
            names(tmp.path())
        );
    }

    #[test]
    fn a_zip_that_is_unpacked_twice_makes_two_folders() {
        let tmp = tree();
        let packed = compress(&[tmp.path().join("single.txt")], tmp.path(), "s.zip").unwrap();
        let first = extract(&packed.path).unwrap();
        let second = extract(&packed.path).unwrap();
        assert_eq!(first.path, tmp.path().join("s"));
        assert_eq!(second.path, tmp.path().join("s (2)"));
    }

    fn evil_zip(path: &Path) {
        let mut zip = ZipWriter::new(File::create(path).unwrap());
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        zip.start_file("../escaped.txt", options).unwrap();
        zip.write_all(b"x").unwrap();
        zip.start_file("/abs.txt", options).unwrap();
        zip.write_all(b"x").unwrap();
        zip.start_file("fine.txt", options).unwrap();
        zip.write_all(b"ok").unwrap();
        zip.add_symlink("link", "fine.txt", options).unwrap();
        zip.finish().unwrap();
    }

    #[test]
    fn names_that_climb_out_and_links_are_skipped_and_counted() {
        let tmp = tempfile::tempdir().unwrap();
        let inner = tmp.path().join("in");
        fs::create_dir(&inner).unwrap();
        evil_zip(&inner.join("evil.zip"));
        let unpacked = extract(&inner.join("evil.zip")).unwrap();
        // The name that climbs out and the link are left out; a leading `/` is harmless once
        // the name is taken as relative, so that entry may come through inside the folder.
        assert!(unpacked.skipped >= 2, "{}", unpacked.skipped);
        let got = names(&unpacked.path);
        assert!(got.contains(&"fine.txt".to_owned()), "{got:?}");
        assert!(
            got.iter().all(|n| n == "fine.txt" || n == "abs.txt"),
            "{got:?}"
        );
        assert!(!tmp.path().join("escaped.txt").exists());
        assert!(!inner.join("escaped.txt").exists());
    }

    #[test]
    fn a_broken_archive_leaves_nothing_behind() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("bad.zip"), "this is not a zip").unwrap();
        assert!(extract(&tmp.path().join("bad.zip")).is_err());
        assert_eq!(names(tmp.path()), ["bad.zip"]);
    }

    #[cfg(windows)]
    #[test]
    fn names_windows_would_misread_are_skipped_and_progress_still_finishes() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("odd.zip");
        let mut zip = ZipWriter::new(File::create(&path).unwrap());
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        for name in ["stream.txt:hidden", "CON", "dots.", "ok.txt"] {
            zip.start_file(name, options).unwrap();
            zip.write_all(b"data").unwrap();
        }
        zip.finish().unwrap();
        let p = Arc::new(progress::Progress::default());
        let unpacked = progress::scope(&p, || extract(&path)).unwrap();
        assert_eq!(unpacked.skipped, 3);
        assert_eq!(names(&unpacked.path), ["ok.txt"]);
        assert_eq!((p.items(), p.total_items()), (4, 4));
        assert_eq!(p.bytes(), p.total_bytes());
    }

    #[test]
    fn an_archive_that_is_not_one_is_refused_cleanly() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("x.zip"), vec![0_u8; 10]).unwrap();
        assert!(extract(&tmp.path().join("x.zip")).is_err());
    }

    #[test]
    fn a_failed_unpack_removes_what_it_made_and_keeps_what_someone_else_put_there() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("stage");
        fs::create_dir(&root).unwrap();
        let mut made = Made::default();
        made.make_dirs(&root.join("a/b")).unwrap();
        fs::write(root.join("a/b/mine.txt"), "x").unwrap();
        made.files.push(root.join("a/b/mine.txt"));
        // Dropped in by another program while the unpacking ran.
        fs::write(root.join("a/theirs.txt"), "keep me").unwrap();
        let error = made.remove(&root, io::Error::other("boom"));
        assert!(error.to_string().contains("left there"), "{error}");
        assert_eq!(fs::read(root.join("a/theirs.txt")).unwrap(), b"keep me");
        assert!(!root.join("a/b").exists());
        assert!(!root.join("a/b/mine.txt").exists());
    }

    #[test]
    fn a_failed_unpack_with_nothing_foreign_leaves_no_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("stage");
        fs::create_dir(&root).unwrap();
        let mut made = Made::default();
        made.make_dirs(&root.join("a/b")).unwrap();
        let error = made.remove(&root, io::Error::other("boom"));
        assert_eq!(error.to_string(), "boom");
        assert!(!root.exists());
    }

    #[test]
    fn an_archive_that_would_not_fit_is_refused_before_anything_is_written() {
        assert!(check_room(10 << 30, Some(1 << 30)).is_err());
        assert!(check_room(1 << 20, Some(1 << 30)).is_ok());
        // Room that can't be told is not a reason to refuse.
        assert!(check_room(u64::MAX, None).is_ok());
        let tight = check_room((1 << 30) - 1, Some(1 << 30)).unwrap_err();
        assert_eq!(tight.kind(), ErrorKind::StorageFull);
    }

    #[cfg(windows)]
    #[test]
    fn a_downloaded_archive_marks_what_it_unpacks() {
        let tmp = tree();
        let packed = compress(&[tmp.path().join("single.txt")], tmp.path(), "z.zip").unwrap();
        fs::write(
            stream_of(&packed.path),
            "[ZoneTransfer]
ZoneId=3
",
        )
        .unwrap();
        let unpacked = extract(&packed.path).unwrap();
        let mark = fs::read(stream_of(&unpacked.path.join("single.txt"))).unwrap();
        assert!(String::from_utf8_lossy(&mark).contains("ZoneId=3"));
        // An archive without the mark gives none.
        let plain = compress(&[tmp.path().join("single.txt")], tmp.path(), "p.zip").unwrap();
        let again = extract(&plain.path).unwrap();
        assert!(fs::read(stream_of(&again.path.join("single.txt"))).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn pipes_are_left_out_of_a_pack_and_do_not_block_it() {
        let tmp = tree();
        let fifo = std::ffi::CString::new(tmp.path().join("docs/pipe").to_str().unwrap()).unwrap();
        // SAFETY: `fifo` is a NUL-terminated path that outlives the call.
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        let packed = compress(&[tmp.path().join("docs")], tmp.path(), "d.zip").unwrap();
        assert_eq!(packed.skipped, 1);
    }

    #[test]
    fn cancelling_leaves_no_archive_and_no_temporary_file() {
        let tmp = tree();
        let p = Arc::new(progress::Progress::default());
        p.cancel();
        let result = progress::scope(&p, || {
            compress(&[tmp.path().join("docs")], tmp.path(), "out.zip")
        });
        assert!(progress::is_cancelled_error(&result.unwrap_err()));
        assert!(!names(tmp.path()).iter().any(|n| n.starts_with("out")));
    }

    #[test]
    fn cancelling_an_unpack_removes_the_folder_it_began() {
        let tmp = tree();
        let packed = compress(&[tmp.path().join("docs")], tmp.path(), "d.zip").unwrap();
        let p = Arc::new(progress::Progress::default());
        p.cancel();
        let result = progress::scope(&p, || extract(&packed.path));
        assert!(progress::is_cancelled_error(&result.unwrap_err()));
        assert!(!names(tmp.path()).iter().any(|n| n.ends_with(PARTIAL)));
        assert!(!tmp.path().join("d").exists());
    }

    #[test]
    fn progress_counts_the_bytes_and_files_of_a_pack() {
        let tmp = tree();
        let p = Arc::new(progress::Progress::default());
        progress::scope(&p, || {
            compress(&[tmp.path().join("docs")], tmp.path(), "d.zip").unwrap();
        });
        assert_eq!(p.total_bytes(), 5 + 20_000);
        assert_eq!(p.bytes(), 5 + 20_000);
        // docs, docs/deep, docs/empty, a.txt, b.txt
        assert_eq!((p.items(), p.total_items()), (5, 5));
    }

    #[cfg(unix)]
    #[test]
    fn links_are_left_out_of_a_pack() {
        let tmp = tree();
        std::os::unix::fs::symlink(tmp.path().join("single.txt"), tmp.path().join("docs/link"))
            .unwrap();
        let packed = compress(&[tmp.path().join("docs")], tmp.path(), "d.zip").unwrap();
        assert_eq!(packed.skipped, 1);
        let unpacked = extract(&packed.path).unwrap();
        assert!(!unpacked.path.join("docs/link").exists());
    }
}
