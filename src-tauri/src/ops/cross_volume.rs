//! Moves between volumes, where a rename is impossible: copy the tree, then remove
//! from the source only what was copied and has not changed since.
//!
//! The copy is built under a `.folio-partial` name and only renamed once complete.
//! The source is never removed recursively. Files that appear or change during the
//! move, and files that cannot be removed, stay where they are and are reported, so
//! every file always has at least one complete copy.
//!
//! Links (symlinks, junctions) are recreated at the destination and then removed
//! from the source; what they point to is never copied or removed. Where a link
//! can't be recreated, it stays in the source and is reported.

use std::fs;
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::noclobber;

/// What a file looked like when it was copied. A file that no longer matches was
/// written to after the copy started, so its source must not be removed.
///
/// Size and modified time can be preserved by a writer (or be too coarse, as on FAT),
/// so the stamp also holds what user code can't set back: the file's change time and
/// identity. This is best effort; a write between the last check and the remove can
/// still be missed.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Stamp {
    len: u64,
    modified: SystemTime,
    change: platform::Change,
}

impl Stamp {
    fn of(path: &Path) -> io::Result<Self> {
        let meta = fs::symlink_metadata(path)?;
        if !meta.is_file() {
            return Err(io::Error::new(ErrorKind::InvalidData, "Not a regular file"));
        }
        Ok(Self {
            len: meta.len(),
            modified: meta.modified()?,
            change: platform::change(path, &meta)?,
        })
    }
}

struct CopiedFile {
    src: PathBuf,
    dst: PathBuf,
    src_stamp: Stamp,
    dst_stamp: Stamp,
}

struct CopiedLink {
    path: PathBuf,
    target: PathBuf,
}

/// The record of a finished copy, consumed by [`remove_copied`].
#[derive(Default)]
pub struct CopyLog {
    files: Vec<CopiedFile>,
    /// Source folders, children before parents, so they can be removed in order.
    dirs: Vec<PathBuf>,
    /// Links recreated at the destination; only the link itself is removed. Nothing
    /// below a link is copied or removed.
    links: Vec<CopiedLink>,
    /// Links that couldn't be recreated here; they stay in the source.
    skipped: Vec<PathBuf>,
    /// Everything created at the destination, for removing a failed copy.
    made: super::Created,
    /// Set by `built` for `renamed`: whether the finished top-level file was untouched
    /// before its final rename.
    untouched: bool,
}

/// Moves `src` into `dest_dir` as `name` (or the next free name) across volumes.
/// Returns where it went (`None` when it was a link that can't be recreated here)
/// and the source paths that were left behind.
pub fn move_by_copy(
    src: &Path,
    dest_dir: &Path,
    name: &str,
) -> io::Result<(Option<PathBuf>, Vec<PathBuf>)> {
    let (target, copied) = copy_tracked(src, dest_dir, name)?;
    Ok((target, remove_copied(copied)))
}

/// Copies `src` into `dest_dir` as `name` (or the next free name), recording each
/// file's stamp before it is read. It is the plain tree copy ([`super::copy_tree`]):
/// built under a `.folio-partial` name, renamed once complete, and a failure removes
/// what it created. Links are recreated, never followed (see [`super::copy_link`]).
pub fn copy_tracked(
    src: &Path,
    dest_dir: &Path,
    name: &str,
) -> io::Result<(Option<PathBuf>, CopyLog)> {
    let mut log = CopyLog::default();
    let target = super::copy_tree(src, dest_dir, name, &mut log)?;
    Ok((target, log))
}

impl super::Observe for CopyLog {
    fn made(&mut self) -> &mut super::Created {
        &mut self.made
    }

    fn skipped(&mut self) -> &mut Vec<PathBuf> {
        &mut self.skipped
    }

    fn copy_file(&mut self, src: &Path, dst: &Path) -> io::Result<()> {
        let src_stamp = Stamp::of(src)?;
        noclobber::copy_file(src, dst)?;
        self.made.file(dst)?;
        self.files.push(CopiedFile {
            src: src.to_path_buf(),
            dst: dst.to_path_buf(),
            src_stamp,
            dst_stamp: Stamp::of(dst)?,
        });
        Ok(())
    }

    fn link(&mut self, src: &Path, target: PathBuf) {
        self.links.push(CopiedLink {
            path: src.to_path_buf(),
            target,
        });
    }

    fn folder(&mut self, src: &Path) {
        self.dirs.push(src.to_path_buf());
    }

    fn built(&mut self, temp: &Path) {
        // A copied file is restamped after its rename only if nothing touched it in
        // between, so a stranger's write is never taken for ours.
        self.untouched = self
            .files
            .iter()
            .find(|f| f.dst == temp)
            .is_some_and(|f| Stamp::of(temp).is_ok_and(|s| s == f.dst_stamp));
    }

    fn renamed(&mut self, temp: &Path, target: &Path) {
        self.repoint(temp, target, self.untouched);
    }
}

impl CopyLog {
    /// Points the record at the copy's final name. Renaming a file can change its own
    /// stamp (NTFS updates the change time), so a renamed top-level file is restamped
    /// when `restamp`; otherwise the old stamp stays and the copy is never taken back.
    fn repoint(&mut self, temp: &Path, target: &Path, restamp: bool) {
        for file in &mut self.files {
            if let Ok(rest) = file.dst.strip_prefix(temp) {
                if rest.as_os_str().is_empty() {
                    file.dst = target.to_path_buf();
                    if let Some(stamp) = restamp.then(|| Stamp::of(target).ok()).flatten() {
                        file.dst_stamp = stamp;
                    }
                } else {
                    file.dst = target.join(rest);
                }
            }
        }
    }
}

/// Removes the copied sources that are unchanged since the copy, then the links and
/// folders, non-recursively. Returns what stayed in the source.
pub fn remove_copied(copied: CopyLog) -> Vec<PathBuf> {
    let mut left = copied.skipped;
    for file in copied.files {
        if !remove_source_file(&file, &|p| remove_even_read_only(p, |p| fs::remove_file(p))) {
            left.push(file.src);
        }
    }
    for link in copied.links {
        let unchanged = fs::read_link(&link.path).is_ok_and(|t| t == link.target);
        if !unchanged || platform::remove_link(&link.path).is_err() {
            left.push(link.path);
        }
    }
    for dir in copied.dirs {
        // Fails when something stayed inside; only report it when that wasn't a
        // file already reported (a file was added during the move, for example).
        let removed = remove_even_read_only(&dir, |p| fs::remove_dir(p)).is_ok();
        if !removed && !left.iter().any(|p| p.starts_with(&dir)) {
            left.push(dir);
        }
    }
    left
}

/// Returns true when the source is gone, false when it stayed. `remove` is the remove
/// call, swappable so tests can make it fail after it took effect.
fn remove_source_file(file: &CopiedFile, remove: &dyn Fn(&Path) -> io::Result<()>) -> bool {
    match Stamp::of(&file.src) {
        Ok(stamp) if stamp == file.src_stamp => {}
        // Gone already: nothing to keep.
        Err(e) if e.kind() == ErrorKind::NotFound => return true,
        // Changed since the copy, or no longer a file: the copy is stale, keep the source.
        _ => return false,
    }
    // Without a change time or file id (FAT, `exFAT`: most USB sticks and SD cards) a
    // same-size save inside the 2 s mtime resolution would pass, so compare contents.
    if !file.src_stamp.change.is_strong() && !same_content(&file.src, &file.dst).unwrap_or(false) {
        return false;
    }
    if remove(&file.src).is_ok() {
        return true;
    }
    // A remove can fail after it took effect (a network share dropping the reply), so
    // only a source that is verifiably still here and unchanged counts as kept.
    match Stamp::of(&file.src) {
        Ok(stamp) if stamp == file.src_stamp => {}
        Err(e) if e.kind() == ErrorKind::NotFound => return true,
        _ => return false,
    }
    // The source is in use or protected and stays as the one complete copy, so take
    // back ours, unless someone has written to it meanwhile. This deletes only the file
    // this move just created, never user data, so it doesn't go to the trash (ADR 0003).
    if Stamp::of(&file.dst).is_ok_and(|s| s == file.dst_stamp) {
        let _ = remove_even_read_only(&file.dst, |p| fs::remove_file(p));
    }
    false
}

/// True when both files hold the same bytes.
fn same_content(a: &Path, b: &Path) -> io::Result<bool> {
    use std::io::Read;
    let (mut a, mut b) = (fs::File::open(a)?, fs::File::open(b)?);
    if a.metadata()?.len() != b.metadata()?.len() {
        return Ok(false);
    }
    let (mut buf_a, mut buf_b) = (vec![0; 64 * 1024], vec![0; 64 * 1024]);
    loop {
        let n = a.read(&mut buf_a)?;
        if n == 0 {
            return Ok(b.read(&mut buf_b)? == 0);
        }
        b.read_exact(&mut buf_b[..n])?;
        if buf_a[..n] != buf_b[..n] {
            return Ok(false);
        }
    }
}

/// Removes a link itself (a symlink or junction); what it points to is kept. Refuses
/// anything that is not a link.
pub(super) fn remove_link(path: &Path) -> io::Result<()> {
    platform::remove_link(path)
}

fn not_a_link() -> io::Error {
    io::Error::new(ErrorKind::InvalidInput, "Not a link")
}

/// Runs `remove`, and on Windows retries with the read-only attribute cleared if that
/// is what blocks it. The attribute is restored when the remove still fails.
pub(super) fn remove_even_read_only(
    path: &Path,
    remove: impl Fn(&Path) -> io::Result<()>,
) -> io::Result<()> {
    let err = match remove(path) {
        Ok(()) => return Ok(()),
        Err(e) if cfg!(windows) && e.kind() == ErrorKind::PermissionDenied => e,
        Err(e) => return Err(e),
    };
    // On Unix a denied remove is about the folder, not the file's mode, so clearing
    // the attribute (making the file world-writable) could never help.
    // Never through a link: the attribute to clear would be its target's.
    let meta = fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() {
        return Err(err);
    }
    let mut perms = meta.permissions();
    if !perms.readonly() {
        return Err(err);
    }
    #[expect(
        clippy::permissions_set_readonly_false,
        reason = "Windows only: clears the attribute so a moved read-only item can leave its source"
    )]
    perms.set_readonly(false);
    fs::set_permissions(path, perms.clone())?;
    remove(path).inspect_err(|_| {
        perms.set_readonly(true);
        let _ = fs::set_permissions(path, perms);
    })
}

#[cfg(windows)]
mod platform {
    use std::fs::{self, Metadata};
    use std::io;
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    use std::os::windows::io::AsRawHandle;
    use std::path::Path;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Storage::FileSystem::{
        FILE_BASIC_INFO, FILE_ID_INFO, FILE_INFO_BY_HANDLE_CLASS, FileBasicInfo, FileIdInfo,
        GetFileInformationByHandleEx, GetVolumeInformationByHandleW,
    };

    const FILE_READ_ATTRIBUTES: u32 = 0x80;
    const FILE_SHARE_ALL: u32 = 0x7;
    const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;

    /// NTFS change time (updated by any write, including one that restores the
    /// modified time) and the 128-bit file id. Either is `None` where the file system
    /// doesn't have it.
    #[derive(Clone, Copy, PartialEq, Eq)]
    pub struct Change {
        time: Option<i64>,
        id: Option<[u8; 16]>,
        /// The volume keeps a real change time and file id (NTFS, `ReFS`). FAT and `exFAT`
        /// answer the queries with made-up or coarse values, so they don't count.
        strong: bool,
    }

    impl Change {
        pub fn is_strong(self) -> bool {
            self.strong && self.time.is_some() && self.id.is_some()
        }
    }

    pub fn change(path: &Path, _meta: &Metadata) -> io::Result<Change> {
        // Attributes-only access: no data is read (a cloud placeholder isn't downloaded)
        // and it doesn't conflict with how other programs have the file open. A fresh
        // handle also makes SMB fetch current values instead of its cached ones.
        let file = fs::OpenOptions::new()
            .access_mode(FILE_READ_ATTRIBUTES)
            .share_mode(FILE_SHARE_ALL)
            .open(path)?;
        let handle = HANDLE(file.as_raw_handle());
        // SAFETY: `file` keeps the handle open, and each class is paired with its struct.
        let (basic, id) = unsafe {
            (
                info::<FILE_BASIC_INFO>(handle, FileBasicInfo),
                info::<FILE_ID_INFO>(handle, FileIdInfo),
            )
        };
        Ok(Change {
            time: basic.map(|i| i.ChangeTime),
            id: id.map(|i| i.FileId.Identifier),
            strong: matches!(file_system(handle).as_deref(), Some("NTFS" | "ReFS")),
        })
    }

    /// The name of the file system holding `handle`'s file, such as "NTFS" or "exFAT".
    fn file_system(handle: HANDLE) -> Option<String> {
        let mut name = [0u16; 32];
        // SAFETY: `handle` is open (the caller holds its `File`) and `name` is a local
        // buffer the call writes a NUL-terminated string into, bounded by its length.
        unsafe { GetVolumeInformationByHandleW(handle, None, None, None, None, Some(&mut name)) }
            .ok()?;
        let len = name.iter().position(|&c| c == 0).unwrap_or(name.len());
        Some(String::from_utf16_lossy(&name[..len]))
    }

    /// One `GetFileInformationByHandleEx` query; `None` if the file system lacks it.
    ///
    /// # Safety
    /// `handle` must be open, and `T` must be the struct that `class` writes.
    unsafe fn info<T: Default>(handle: HANDLE, class: FILE_INFO_BY_HANDLE_CLASS) -> Option<T> {
        let mut out = T::default();
        #[expect(
            clippy::cast_possible_truncation,
            reason = "these structs are a few dozen bytes"
        )]
        let size = size_of::<T>() as u32;
        // SAFETY: the caller guarantees `handle` is open and `T` is what `class` writes;
        // `out` is a local `T` of exactly `size` bytes.
        unsafe { GetFileInformationByHandleEx(handle, class, (&raw mut out).cast(), size) }
            .ok()
            .map(|()| out)
    }

    /// Removes a symlink or junction itself; the folder or file it points to is kept.
    pub fn remove_link(path: &Path) -> io::Result<()> {
        let meta = fs::symlink_metadata(path)?;
        // Something else took the name since the link was made: not ours to remove.
        if !meta.file_type().is_symlink() {
            return Err(super::not_a_link());
        }
        let attributes = meta.file_attributes();
        if attributes & FILE_ATTRIBUTE_DIRECTORY == 0 {
            fs::remove_file(path)
        } else {
            fs::remove_dir(path)
        }
    }
}

#[cfg(unix)]
mod platform {
    use std::fs::{self, Metadata};
    use std::io;
    use std::os::unix::fs::MetadataExt;
    use std::path::Path;

    /// The inode's change time (set by every write, including one that restores the
    /// modified time) and its number.
    #[derive(Clone, Copy, PartialEq, Eq)]
    pub struct Change {
        ctime: (i64, i64),
        ino: u64,
    }

    impl Change {
        /// Every Unix file system, FAT drivers included, keeps a ctime and an inode.
        #[expect(clippy::unused_self, reason = "same signature as the Windows version")]
        pub fn is_strong(self) -> bool {
            true
        }
    }

    #[expect(
        clippy::unnecessary_wraps,
        reason = "same signature as the Windows version, which can fail"
    )]
    pub fn change(_path: &Path, meta: &Metadata) -> io::Result<Change> {
        Ok(Change {
            ctime: (meta.ctime(), meta.ctime_nsec()),
            ino: meta.ino(),
        })
    }

    /// Removes a symlink itself; what it points to is kept.
    pub fn remove_link(path: &Path) -> io::Result<()> {
        // Something else took the name since the link was made: not ours to remove.
        if !fs::symlink_metadata(path)?.file_type().is_symlink() {
            return Err(super::not_a_link());
        }
        fs::remove_file(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn tree() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let t = tempfile::tempdir().unwrap();
        let src = t.path().join("src");
        fs::create_dir_all(src.join("sub")).unwrap();
        fs::write(src.join("a.txt"), "a").unwrap();
        fs::write(src.join("sub").join("b.txt"), "b").unwrap();
        let dst = t.path().join("dst");
        (t, src, dst)
    }

    fn set_readonly(p: &Path, ro: bool) {
        let mut perms = fs::metadata(p).unwrap().permissions();
        perms.set_readonly(ro);
        fs::set_permissions(p, perms).unwrap();
    }

    /// `copy_tracked` to an exact path, which must be free.
    fn copy_tracked(src: &Path, dst: &Path) -> io::Result<CopyLog> {
        let name = dst.file_name().unwrap().to_str().unwrap();
        let (target, copied) = super::copy_tracked(src, dst.parent().unwrap(), name)?;
        assert_eq!(target.as_deref(), Some(dst));
        Ok(copied)
    }

    /// `move_by_copy` to an exact path, which must be free.
    fn move_by_copy(src: &Path, dst: &Path) -> io::Result<Vec<PathBuf>> {
        let name = dst.file_name().unwrap().to_str().unwrap();
        let (target, left) = super::move_by_copy(src, dst.parent().unwrap(), name)?;
        assert_eq!(target.as_deref(), Some(dst));
        Ok(left)
    }

    fn set_modified(p: &Path, time: SystemTime) {
        fs::File::options()
            .write(true)
            .open(p)
            .unwrap()
            .set_modified(time)
            .unwrap();
    }

    #[test]
    fn unchanged_tree_is_moved_completely() {
        let (_t, src, dst) = tree();
        let left = move_by_copy(&src, &dst).unwrap();
        assert!(left.is_empty(), "{left:?}");
        assert!(!src.exists());
        assert_eq!(
            fs::read_to_string(dst.join("sub").join("b.txt")).unwrap(),
            "b"
        );
    }

    #[test]
    fn single_file_is_moved() {
        let (_t, src, dst) = tree();
        let left = move_by_copy(&src.join("a.txt"), &dst).unwrap();
        assert!(left.is_empty(), "{left:?}");
        assert!(!src.join("a.txt").exists());
        assert_eq!(fs::read_to_string(&dst).unwrap(), "a");
    }

    #[test]
    fn file_added_after_copy_is_not_deleted() {
        let (_t, src, dst) = tree();
        let copied = copy_tracked(&src, &dst).unwrap();
        fs::write(src.join("sub").join("new.txt"), "new").unwrap();

        let left = remove_copied(copied);

        assert_eq!(
            fs::read_to_string(src.join("sub").join("new.txt")).unwrap(),
            "new"
        );
        assert!(!src.join("a.txt").exists() && !src.join("sub").join("b.txt").exists());
        assert_eq!(left, vec![src.join("sub")]);
    }

    #[test]
    fn file_modified_after_copy_is_not_deleted() {
        let (_t, src, dst) = tree();
        let copied = copy_tracked(&src, &dst).unwrap();
        // Same length, different content and time: only the timestamp gives it away.
        let a = src.join("a.txt");
        let before = fs::metadata(&a).unwrap().modified().unwrap();
        fs::write(&a, "A").unwrap();
        set_modified(&a, before + Duration::from_secs(2));

        let left = remove_copied(copied);

        assert_eq!(fs::read_to_string(src.join("a.txt")).unwrap(), "A");
        assert_eq!(fs::read_to_string(dst.join("a.txt")).unwrap(), "a");
        assert_eq!(left, vec![src.join("a.txt")]);
        assert!(!src.join("sub").exists());
    }

    #[test]
    fn same_size_rewrite_with_restored_mtime_is_not_deleted() {
        let (_t, src, dst) = tree();
        let a = src.join("a.txt");
        let before = fs::metadata(&a).unwrap().modified().unwrap();
        let copied = copy_tracked(&src, &dst).unwrap();
        // A tag editor that "preserves timestamps": same size, same mtime, new content.
        std::thread::sleep(Duration::from_millis(20));
        fs::write(&a, "A").unwrap();
        set_modified(&a, before);

        let left = remove_copied(copied);

        assert_eq!(fs::read_to_string(&a).unwrap(), "A");
        assert_eq!(left, vec![a]);
    }

    #[cfg(windows)]
    #[test]
    fn a_failed_move_copy_removes_its_partial_and_keeps_the_source() {
        use std::os::windows::fs::OpenOptionsExt;
        let (t, src, _dst) = tree();
        let dest = t.path().join("dest");
        fs::create_dir(&dest).unwrap();
        // No sharing at all: the copy of b.txt fails halfway through the tree.
        let guard = fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(src.join("sub").join("b.txt"))
            .unwrap();

        let res = super::move_by_copy(&src, &dest, "src");

        drop(guard);
        assert!(res.is_err());
        assert_eq!(
            fs::read_dir(&dest).unwrap().count(),
            0,
            "partial copy removed"
        );
        assert_eq!(fs::read_to_string(src.join("a.txt")).unwrap(), "a");
        assert_eq!(
            fs::read_to_string(src.join("sub").join("b.txt")).unwrap(),
            "b"
        );
    }

    #[test]
    fn a_renamed_single_file_is_still_taken_back_when_its_source_stays() {
        let (_t, src, dst) = tree();
        let copied = copy_tracked(&src.join("a.txt"), &dst).unwrap();
        let refuse = |_: &Path| Err(io::Error::other("in use"));

        assert!(!remove_source_file(&copied.files[0], &refuse));
        assert!(
            !dst.exists(),
            "the stamp follows the rename, so our copy is recognised"
        );
    }

    #[test]
    fn same_content_compares_bytes() {
        let (_t, src, _dst) = tree();
        let (a, b) = (src.join("a.txt"), src.join("sub").join("b.txt"));
        assert!(same_content(&a, &a).unwrap());
        assert!(
            !same_content(&a, &b).unwrap(),
            "same length, different bytes"
        );
        fs::write(&b, "ab").unwrap();
        assert!(!same_content(&a, &b).unwrap(), "different length");
    }

    #[cfg(windows)]
    #[test]
    fn temp_volume_has_a_strong_change_stamp() {
        // The temp folder is on NTFS on every supported Windows setup.
        let (_t, src, _dst) = tree();
        assert!(Stamp::of(&src.join("a.txt")).unwrap().change.is_strong());
    }

    #[test]
    fn destination_is_kept_when_a_failed_remove_took_effect() {
        let (_t, src, dst) = tree();
        let copied = copy_tracked(&src.join("a.txt"), &dst).unwrap();
        // A network share deletes the file, then the reply is lost.
        let flaky = |p: &Path| {
            fs::remove_file(p)?;
            Err(io::Error::other("connection reset"))
        };

        assert!(remove_source_file(&copied.files[0], &flaky));
        assert_eq!(fs::read_to_string(&dst).unwrap(), "a");
    }

    #[test]
    fn destination_is_kept_when_source_changes_during_a_failed_remove() {
        let (_t, src, dst) = tree();
        let a = src.join("a.txt");
        let copied = copy_tracked(&a, &dst).unwrap();
        let rewrite = |p: &Path| {
            fs::write(p, "rewritten")?;
            Err(io::Error::other("in use"))
        };

        assert!(!remove_source_file(&copied.files[0], &rewrite));
        assert_eq!(fs::read_to_string(&a).unwrap(), "rewritten");
        assert_eq!(fs::read_to_string(&dst).unwrap(), "a");
    }

    #[cfg(windows)]
    #[test]
    fn read_only_folder_is_moved() {
        let (_t, src, dst) = tree();
        set_readonly(&src.join("sub"), true);

        let left = move_by_copy(&src, &dst).unwrap();

        assert!(left.is_empty(), "{left:?}");
        assert!(!src.exists());
        let _ = fs::metadata(dst.join("sub")).map(|_| set_readonly(&dst.join("sub"), false));
    }

    #[test]
    fn file_resized_after_copy_is_not_deleted() {
        let (_t, src, dst) = tree();
        let copied = copy_tracked(&src, &dst).unwrap();
        fs::write(src.join("a.txt"), "longer").unwrap();
        let left = remove_copied(copied);
        assert_eq!(fs::read_to_string(src.join("a.txt")).unwrap(), "longer");
        assert_eq!(left, vec![src.join("a.txt")]);
    }

    #[test]
    fn read_only_source_is_moved_and_keeps_attribute() {
        let (_t, src, dst) = tree();
        set_readonly(&src.join("a.txt"), true);

        let left = move_by_copy(&src, &dst).unwrap();

        assert!(left.is_empty(), "{left:?}");
        assert!(!src.exists());
        let moved = dst.join("a.txt");
        assert!(fs::metadata(&moved).unwrap().permissions().readonly());
        set_readonly(&moved, false);
    }

    #[cfg(windows)]
    #[test]
    fn source_in_use_stays_as_the_only_copy() {
        use std::os::windows::fs::OpenOptionsExt;
        let (_t, src, dst) = tree();
        // Readers may share, deleters may not: the copy succeeds, the remove fails.
        let handle = fs::OpenOptions::new()
            .read(true)
            .share_mode(1) // FILE_SHARE_READ
            .open(src.join("a.txt"))
            .unwrap();

        let left = move_by_copy(&src, &dst).unwrap();

        assert_eq!(left, vec![src.join("a.txt")]);
        assert!(!dst.join("a.txt").exists(), "exactly one copy must remain");
        assert_eq!(fs::read_to_string(src.join("a.txt")).unwrap(), "a");
        assert_eq!(
            fs::read_to_string(dst.join("sub").join("b.txt")).unwrap(),
            "b"
        );
        assert!(!src.join("sub").exists());
        drop(handle);
    }

    #[cfg(windows)]
    #[test]
    fn read_only_source_in_use_keeps_its_attribute() {
        use std::os::windows::fs::OpenOptionsExt;
        let (_t, src, dst) = tree();
        let ro = src.join("a.txt");
        set_readonly(&ro, true);
        let handle = fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&ro)
            .unwrap();

        let left = move_by_copy(&src, &dst).unwrap();

        assert_eq!(left, vec![ro.clone()]);
        assert!(fs::metadata(&ro).unwrap().permissions().readonly());
        assert!(!dst.join("a.txt").exists());
        drop(handle);
        set_readonly(&ro, false);
    }

    #[cfg(windows)]
    #[test]
    fn our_copy_is_kept_if_written_to_before_rollback() {
        use std::os::windows::fs::OpenOptionsExt;
        let (_t, src, dst) = tree();
        let copied = copy_tracked(&src, &dst).unwrap();
        fs::write(dst.join("a.txt"), "edited at destination").unwrap();
        let handle = fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(src.join("a.txt"))
            .unwrap();

        let left = remove_copied(copied);

        assert_eq!(left, vec![src.join("a.txt")]);
        assert_eq!(
            fs::read_to_string(dst.join("a.txt")).unwrap(),
            "edited at destination"
        );
        drop(handle);
    }

    #[cfg(unix)]
    #[test]
    fn link_is_recreated_and_its_target_is_never_copied() {
        let (t, src, dst) = tree();
        let outside = t.path().join("outside");
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("keep.txt"), "keep").unwrap();
        std::os::unix::fs::symlink(&outside, src.join("link")).unwrap();

        let left = move_by_copy(&src, &dst).unwrap();

        assert!(left.is_empty(), "{left:?}");
        assert!(!src.exists());
        assert_eq!(fs::read_link(dst.join("link")).unwrap(), outside);
        assert_eq!(
            fs::read_to_string(outside.join("keep.txt")).unwrap(),
            "keep"
        );
    }

    #[cfg(windows)]
    fn junction(link: &Path, target: &Path) {
        let made = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()
            .unwrap();
        assert!(made.status.success());
    }

    #[cfg(windows)]
    fn is_link(path: &Path) -> bool {
        fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink())
    }

    #[cfg(windows)]
    #[test]
    fn a_top_level_junction_moves_without_touching_its_target() {
        let (t, src, _dst) = tree();
        let outside = t.path().join("outside");
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("keep.txt"), "keep").unwrap();
        let link = src.join("door");
        junction(&link, &outside);

        let (target, left) = super::move_by_copy(&link, t.path(), "moved").unwrap();

        assert_eq!(target, Some(t.path().join("moved")));
        assert!(left.is_empty(), "{left:?}");
        assert!(fs::symlink_metadata(&link).is_err(), "source link gone");
        assert!(is_link(&t.path().join("moved")));
        assert_eq!(
            fs::read_to_string(outside.join("keep.txt")).unwrap(),
            "keep"
        );
    }

    #[cfg(windows)]
    #[test]
    fn a_tree_with_a_junction_moves_and_leaves_nothing_behind() {
        let (t, src, dst) = tree();
        let outside = t.path().join("outside");
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("keep.txt"), "keep").unwrap();
        junction(&src.join("link"), &outside);

        let left = move_by_copy(&src, &dst).unwrap();

        assert!(left.is_empty(), "{left:?}");
        assert!(!src.exists(), "the whole source folder is gone");
        assert!(is_link(&dst.join("link")));
        assert_eq!(
            fs::read_to_string(dst.join("link").join("keep.txt")).unwrap(),
            "keep"
        );
        assert_eq!(fs::read_to_string(dst.join("a.txt")).unwrap(), "a");
        assert_eq!(
            fs::read_to_string(outside.join("keep.txt")).unwrap(),
            "keep"
        );
    }

    #[cfg(windows)]
    #[test]
    fn removing_a_link_never_clears_its_targets_read_only_flag() {
        use std::cell::Cell;
        let t = tempfile::tempdir().unwrap();
        let folder = t.path().join("folder");
        fs::create_dir(&folder).unwrap();
        set_readonly(&folder, true);
        let link = t.path().join("link");
        junction(&link, &folder);
        let calls = Cell::new(0);

        let res = remove_even_read_only(&link, |_| {
            calls.set(calls.get() + 1);
            Err(ErrorKind::PermissionDenied.into())
        });

        assert!(res.is_err());
        assert_eq!(calls.get(), 1, "no retry through the link");
        assert!(fs::metadata(&folder).unwrap().permissions().readonly());
        set_readonly(&folder, false);
    }
}
