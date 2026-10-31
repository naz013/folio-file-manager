//! File-system primitives that never replace an existing item. A name taken between
//! choosing it and writing to it (by a sync client, another app or a second transfer)
//! fails with `AlreadyExists` instead of being overwritten.

use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};

/// How many taken names [`place`] skips before giving up.
const MAX_ATTEMPTS: usize = 1000;

/// Runs `op` on `dir/name`, or on the next free `name (2)`, `(3)`… when the name is
/// taken, retrying when another writer claims a name first. Returns the path used.
///
/// `op` must fail with `AlreadyExists` only when the path it was given is taken, and
/// must have created nothing in that case.
pub fn place(
    dir: &Path,
    name: &str,
    mut op: impl FnMut(&Path) -> io::Result<()>,
) -> io::Result<PathBuf> {
    for _ in 0..MAX_ATTEMPTS {
        let target = super::unique_destination(dir, name);
        match op(&target) {
            Ok(()) => return Ok(target),
            Err(e) if e.kind() == ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::new(
        ErrorKind::AlreadyExists,
        format!("No free name for \"{name}\""),
    ))
}

pub use platform::{copy_file, file_id, rename};

/// True when `a` and `b` name the same file or folder (links not followed), such as
/// `readme.md` and `README.md` on a case-insensitive volume.
///
/// Some network redirectors report 0 for every file id; that is treated as unknown,
/// so it never makes two items look like one.
pub fn same_file(a: &Path, b: &Path) -> io::Result<bool> {
    let (a, b) = (file_id(a)?, file_id(b)?);
    Ok(a.1 != 0 && a == b)
}

#[cfg(windows)]
mod platform {
    use std::cell::Cell;
    use std::ffi::c_void;
    use std::fs;
    use std::io::{self, ErrorKind};
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use std::path::Path;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, COPY_FILE_FAIL_IF_EXISTS, COPYPROGRESSROUTINE_PROGRESS,
        CopyFileExW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_ID_INFO,
        FileIdInfo, GetFileInformationByHandle, GetFileInformationByHandleEx,
        LPPROGRESS_ROUTINE_CALLBACK_REASON, MOVE_FILE_FLAGS, MoveFileExW, PROGRESS_CANCEL,
        PROGRESS_CONTINUE,
    };
    use windows::core::HSTRING;

    /// Called by `CopyFileExW` as chunks are copied. `data` points at the `Cell` holding
    /// the bytes already counted for this file.
    unsafe extern "system" fn progress_routine(
        _total_size: i64,
        transferred: i64,
        _stream_size: i64,
        _stream_transferred: i64,
        _stream_number: u32,
        _reason: LPPROGRESS_ROUTINE_CALLBACK_REASON,
        _source: HANDLE,
        _destination: HANDLE,
        data: *const c_void,
    ) -> COPYPROGRESSROUTINE_PROGRESS {
        // SAFETY: `data` is the address of the `Cell<i64>` that `copy_file` keeps alive
        // for the whole `CopyFileExW` call, and the routine runs on that same thread.
        let counted = unsafe { &*data.cast::<Cell<i64>>() };
        let new = u64::try_from(transferred - counted.get()).unwrap_or(0);
        counted.set(transferred);
        super::super::progress::add_bytes(new);
        // Waits here while paused; a cancel stops the copy and removes the partial file.
        match super::super::progress::checkpoint() {
            Ok(()) => PROGRESS_CONTINUE,
            Err(_) => PROGRESS_CANCEL,
        }
    }

    /// Copies a file (data, attributes, timestamps, alternate streams, like
    /// `fs::copy`) to `dst`, failing if `dst` exists.
    pub fn copy_file(src: &Path, dst: &Path) -> io::Result<()> {
        super::super::progress::checkpoint()?;
        let (src, dst) = (verbatim(src)?, verbatim(dst)?);
        let counted = Cell::new(0_i64);
        // SAFETY: both strings are NUL-terminated and outlive the call, and `counted`
        // outlives it too and is only touched by the routine, on this thread. No cancel
        // flag is passed; the routine cancels.
        let result = unsafe {
            CopyFileExW(
                &src,
                &dst,
                Some(progress_routine),
                Some((&raw const counted).cast()),
                None,
                COPY_FILE_FAIL_IF_EXISTS,
            )
        };
        match result {
            Ok(()) => Ok(()),
            // The routine stopped it, and the system removed the partial copy.
            Err(_) if super::super::progress::current().is_some_and(|p| p.is_cancelled()) => {
                Err(super::super::progress::cancelled())
            }
            Err(e) => Err(io_error(e)),
        }
    }

    /// Renames a file or folder, failing if `dst` exists. Across volumes it fails with
    /// `ERROR_NOT_SAME_DEVICE` (no copy fallback), like `fs::rename`.
    pub fn rename(src: &Path, dst: &Path) -> io::Result<()> {
        let (wide_src, wide_dst) = (verbatim(src)?, verbatim(dst)?);
        // SAFETY: both strings are NUL-terminated and outlive the call. Flags 0: no
        // MOVEFILE_REPLACE_EXISTING, no MOVEFILE_COPY_ALLOWED.
        match unsafe { MoveFileExW(&wide_src, &wide_dst, MOVE_FILE_FLAGS(0)) }.map_err(io_error) {
            // A folder in the way is reported as access denied, not as existing.
            Err(e)
                if e.kind() == ErrorKind::PermissionDenied
                    && dst.symlink_metadata().is_ok()
                    && !super::same_file(src, dst).unwrap_or(false) =>
            {
                Err(io::Error::new(ErrorKind::AlreadyExists, e))
            }
            result => result,
        }
    }

    /// Volume serial number and file index of what `path` names; links aren't followed.
    pub fn file_id(path: &Path) -> io::Result<(u64, u128)> {
        const FILE_READ_ATTRIBUTES: u32 = 0x80;
        const FILE_SHARE_ALL: u32 = 0x7;
        let file = fs::OpenOptions::new()
            .access_mode(FILE_READ_ATTRIBUTES)
            .share_mode(FILE_SHARE_ALL)
            // Folders need backup semantics; the reparse flag opens a link itself.
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS.0 | FILE_FLAG_OPEN_REPARSE_POINT.0)
            .open(path)?;
        let handle = HANDLE(file.as_raw_handle());
        let mut id = FILE_ID_INFO::default();
        #[expect(clippy::cast_possible_truncation, reason = "FILE_ID_INFO is 24 bytes")]
        let size = size_of::<FILE_ID_INFO>() as u32;
        // SAFETY: `file` keeps the handle open, and `id` is a local FILE_ID_INFO of
        // `size` bytes, the struct FileIdInfo writes.
        let full =
            unsafe { GetFileInformationByHandleEx(handle, FileIdInfo, (&raw mut id).cast(), size) };
        if full.is_ok() {
            return Ok((
                id.VolumeSerialNumber,
                u128::from_le_bytes(id.FileId.Identifier),
            ));
        }
        // FAT has no 128-bit ids; its 64-bit index is enough to tell two names apart.
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        // SAFETY: `file` keeps the handle open, and `info` is a local the call fills.
        unsafe { GetFileInformationByHandle(handle, &raw mut info) }.map_err(io_error)?;
        let index = (u128::from(info.nFileIndexHigh) << 32) | u128::from(info.nFileIndexLow);
        Ok((u64::from(info.dwVolumeSerialNumber), index))
    }

    /// The `\\?\` form of `path`, so paths over 260 characters work as they do in std.
    /// Built from UTF-16 units, so names that aren't valid Unicode survive.
    pub(super) fn verbatim(path: &Path) -> io::Result<HSTRING> {
        let full: Vec<u16> = std::path::absolute(path)?
            .as_os_str()
            .encode_wide()
            .collect();
        let units = |s: &str| s.encode_utf16().collect::<Vec<u16>>();
        let wide = if full.starts_with(&units(r"\\?\")) || full.starts_with(&units(r"\\.\")) {
            full
        } else if full.starts_with(&units(r"\\")) {
            [units(r"\\?\UNC\"), full[2..].to_vec()].concat()
        } else {
            [units(r"\\?\"), full].concat()
        };
        Ok(HSTRING::from_wide(&wide))
    }

    /// Maps a Win32 failure to `io::Error` with its Win32 code, so `kind()` works
    /// (`ERROR_FILE_EXISTS` is `AlreadyExists`) and callers can match raw codes.
    fn io_error(e: windows::core::Error) -> io::Error {
        let code = e.code().0.cast_unsigned();
        if code & 0xFFFF_0000 == 0x8007_0000 {
            io::Error::from_raw_os_error((code & 0xFFFF).cast_signed())
        } else {
            io::Error::other(e)
        }
    }
}

#[cfg(unix)]
mod platform {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    use std::ffi::{CStr, CString};
    use std::fs::{self, File, OpenOptions};
    use std::io::{self, ErrorKind};
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    use std::path::Path;

    /// Copies a file to `dst`, failing if `dst` exists. Writes only through the handle
    /// that created `dst`: reopening by path could hit a file someone renamed onto it.
    pub fn copy_file(src: &Path, dst: &Path) -> io::Result<()> {
        super::super::progress::checkpoint()?;
        // Refuse FIFOs and devices before opening one blocks or never ends.
        if !fs::metadata(src)?.is_file() {
            return Err(not_a_file());
        }
        let mut from = File::open(src)?;
        let meta = from.metadata()?;
        if !meta.is_file() {
            return Err(not_a_file());
        }
        // Readable only by us until the source's mode is applied at the end.
        let mut to = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(dst)?;
        fill(&mut from, &mut to, meta.permissions()).inspect_err(|_| {
            // Remove the partial file, but only if the name still points at ours.
            let ours = to.metadata().ok().map(|m| (m.dev(), m.ino()));
            let there = fs::symlink_metadata(dst).ok().map(|m| (m.dev(), m.ino()));
            if ours.is_some() && ours == there {
                let _ = fs::remove_file(dst);
            }
        })
    }

    fn not_a_file() -> io::Error {
        io::Error::new(ErrorKind::InvalidInput, "Not a regular file")
    }

    /// Data, extended attributes, ACLs, flags and mode, as `fs::copy` does on macOS.
    #[cfg(target_os = "macos")]
    fn fill(from: &mut File, to: &mut File, _perms: fs::Permissions) -> io::Result<()> {
        use std::os::fd::AsRawFd;
        // SAFETY: both descriptors are open for the call (the `File`s are borrowed), and
        // a null state is documented as allowed.
        let status = unsafe {
            libc::fcopyfile(
                from.as_raw_fd(),
                to.as_raw_fd(),
                std::ptr::null_mut(),
                libc::COPYFILE_DATA | libc::COPYFILE_METADATA,
            )
        };
        if status == 0 {
            // One call copies the file, so it counts when it is done.
            super::super::progress::add_bytes(from.metadata().map_or(0, |m| m.len()));
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    /// Data and mode, as `fs::copy` does on Linux, in chunks so progress, pause and
    /// cancel are noticed on big files.
    #[cfg(not(target_os = "macos"))]
    fn fill(from: &mut File, to: &mut File, perms: fs::Permissions) -> io::Result<()> {
        use std::io::{Read, Write};
        let mut buffer = vec![0_u8; 1024 * 1024];
        loop {
            super::super::progress::checkpoint()?;
            let n = from.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            to.write_all(&buffer[..n])?;
            super::super::progress::add_bytes(n as u64);
        }
        to.set_permissions(perms)
    }

    /// Device and inode of what `path` names; links aren't followed.
    pub fn file_id(path: &Path) -> io::Result<(u64, u128)> {
        let meta = fs::symlink_metadata(path)?;
        Ok((meta.dev(), u128::from(meta.ino())))
    }

    /// Renames a file or folder, failing if `dst` exists.
    pub fn rename(src: &Path, dst: &Path) -> io::Result<()> {
        match no_replace_rename(src, dst) {
            Err(e) if unsupported(&e) => rename_without_kernel_support(src, dst),
            result => result,
        }
    }

    fn unsupported(e: &io::Error) -> bool {
        // Not a match pattern: ENOTSUP and EOPNOTSUPP are the same number on Linux.
        let codes = [libc::EINVAL, libc::ENOSYS, libc::ENOTSUP, libc::EOPNOTSUPP];
        e.raw_os_error().is_some_and(|c| codes.contains(&c))
    }

    /// For file systems (some NFS, FUSE, SMB mounts) without a no-replace rename. A
    /// hard link fails atomically if `dst` exists; folders, and file systems without
    /// hard links, fall back to check-then-rename, which can still lose a race.
    fn rename_without_kernel_support(src: &Path, dst: &Path) -> io::Result<()> {
        let kind = fs::symlink_metadata(src)?.file_type();
        // Not for links: a hard link may resolve one and change what's moved.
        if kind.is_file() {
            match fs::hard_link(src, dst) {
                Ok(()) => {
                    return fs::remove_file(src).inspect_err(|_| {
                        // The source stays, so undo the second name for the same file.
                        let _ = fs::remove_file(dst);
                    });
                }
                Err(e) if e.kind() == ErrorKind::AlreadyExists => return Err(e),
                Err(_) => {}
            }
        }
        if fs::symlink_metadata(dst).is_ok() {
            return Err(ErrorKind::AlreadyExists.into());
        }
        fs::rename(src, dst)
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn no_replace_rename(src: &Path, dst: &Path) -> io::Result<()> {
        let src = CString::new(src.as_os_str().as_bytes())?;
        let dst = CString::new(dst.as_os_str().as_bytes())?;
        if raw_rename(&src, &dst) == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    #[cfg(target_os = "linux")]
    fn raw_rename(src: &CStr, dst: &CStr) -> libc::c_int {
        // SAFETY: both are valid NUL-terminated strings that outlive the call.
        unsafe {
            libc::renameat2(
                libc::AT_FDCWD,
                src.as_ptr(),
                libc::AT_FDCWD,
                dst.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        }
    }

    #[cfg(target_os = "macos")]
    fn raw_rename(src: &CStr, dst: &CStr) -> libc::c_int {
        // SAFETY: both are valid NUL-terminated strings that outlive the call.
        unsafe { libc::renamex_np(src.as_ptr(), dst.as_ptr(), libc::RENAME_EXCL) }
    }

    /// No no-replace rename on this target: report it so the fallback is used.
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    fn no_replace_rename(_src: &Path, _dst: &Path) -> io::Result<()> {
        Err(io::Error::from_raw_os_error(libc::ENOSYS))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn copy_file_refuses_an_existing_target() {
        let t = tempfile::tempdir().unwrap();
        let (a, b) = (t.path().join("a"), t.path().join("b"));
        fs::write(&a, "new").unwrap();
        fs::write(&b, "keep").unwrap();

        let err = copy_file(&a, &b).unwrap_err();

        assert_eq!(err.kind(), ErrorKind::AlreadyExists);
        assert_eq!(fs::read_to_string(&b).unwrap(), "keep");
    }

    #[test]
    fn copy_file_copies_into_a_free_name() {
        let t = tempfile::tempdir().unwrap();
        let (a, b) = (t.path().join("a"), t.path().join("b"));
        fs::write(&a, "data").unwrap();
        copy_file(&a, &b).unwrap();
        assert_eq!(fs::read_to_string(&b).unwrap(), "data");
    }

    #[test]
    fn rename_refuses_an_existing_file_or_folder() {
        let t = tempfile::tempdir().unwrap();
        let (a, b, d) = (t.path().join("a"), t.path().join("b"), t.path().join("d"));
        fs::write(&a, "new").unwrap();
        fs::write(&b, "keep").unwrap();
        fs::create_dir(&d).unwrap();

        assert_eq!(rename(&a, &b).unwrap_err().kind(), ErrorKind::AlreadyExists);
        assert!(rename(&a, &d).is_err());
        assert!(rename(&d, &b).is_err());

        assert_eq!(fs::read_to_string(&a).unwrap(), "new");
        assert_eq!(fs::read_to_string(&b).unwrap(), "keep");
        assert!(d.is_dir());
    }

    #[test]
    fn rename_moves_into_a_free_name() {
        let t = tempfile::tempdir().unwrap();
        let (a, b) = (t.path().join("a"), t.path().join("b"));
        fs::create_dir(&a).unwrap();
        rename(&a, &b).unwrap();
        assert!(b.is_dir() && !a.exists());
    }

    #[test]
    fn place_skips_a_name_claimed_by_another_writer() {
        let t = tempfile::tempdir().unwrap();
        let mut first = true;
        let used = place(t.path(), "f.txt", |p| {
            if first {
                // Someone else takes the name between the check and our write.
                first = false;
                fs::write(p, "theirs")?;
            }
            fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(p)
                .map(drop)
        })
        .unwrap();

        assert_eq!(used, t.path().join("f (2).txt"));
        assert_eq!(
            fs::read_to_string(t.path().join("f.txt")).unwrap(),
            "theirs"
        );
    }

    #[cfg(windows)]
    #[test]
    fn verbatim_maps_drive_unc_device_and_verbatim_paths() {
        let v = |p: &str| platform::verbatim(Path::new(p)).unwrap().to_string_lossy();
        assert_eq!(v(r"C:\a\..\b"), r"\\?\C:\b");
        assert_eq!(v(r"\\server\share\f"), r"\\?\UNC\server\share\f");
        assert_eq!(v(r"\\?\C:\x"), r"\\?\C:\x");
        assert_eq!(v(r"\\.\pipe\x"), r"\\.\pipe\x");
    }

    #[cfg(windows)]
    #[test]
    fn names_that_are_not_valid_unicode_survive() {
        use std::ffi::OsString;
        use std::os::windows::ffi::OsStringExt;
        let t = tempfile::tempdir().unwrap();
        // "a" + an unpaired surrogate: a legal NTFS name with no UTF-8 form.
        let odd = t.path().join(OsString::from_wide(&[0x61, 0xD800]));
        fs::write(&odd, "odd").unwrap();

        let copy = t.path().join(OsString::from_wide(&[0x62, 0xD800]));
        copy_file(&odd, &copy).unwrap();
        let moved = t.path().join(OsString::from_wide(&[0x63, 0xD800]));
        rename(&copy, &moved).unwrap();

        assert_eq!(fs::read_to_string(&odd).unwrap(), "odd");
        assert_eq!(fs::read_to_string(&moved).unwrap(), "odd");
        assert!(!copy.exists());
    }

    #[cfg(unix)]
    #[test]
    fn copy_file_refuses_a_fifo() {
        let t = tempfile::tempdir().unwrap();
        let fifo = t.path().join("fifo");
        let made = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap();
        assert!(made.success());
        let err = copy_file(&fifo, &t.path().join("copy")).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::InvalidInput);
        assert!(!t.path().join("copy").exists());
    }

    #[cfg(unix)]
    #[test]
    fn place_treats_a_dangling_link_as_taken() {
        let t = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(t.path().join("gone"), t.path().join("f.txt")).unwrap();
        let src = t.path().join("src.txt");
        fs::write(&src, "data").unwrap();

        let used = place(t.path(), "f.txt", |p| copy_file(&src, p)).unwrap();

        assert_eq!(used, t.path().join("f (2).txt"));
        assert!(
            !t.path().join("gone").exists(),
            "nothing written through the link"
        );
    }

    #[test]
    fn same_file_tells_names_apart_by_identity() {
        let t = tempfile::tempdir().unwrap();
        let (a, b, d) = (t.path().join("a"), t.path().join("b"), t.path().join("d"));
        fs::write(&a, "x").unwrap();
        fs::write(&b, "x").unwrap();
        fs::create_dir(&d).unwrap();

        assert!(same_file(&a, &a).unwrap());
        assert!(!same_file(&a, &b).unwrap(), "same content, different files");
        assert!(same_file(&d, &t.path().join("d")).unwrap());
        fs::hard_link(&a, t.path().join("link")).unwrap();
        assert!(same_file(&a, &t.path().join("link")).unwrap());
    }

    #[cfg(windows)]
    #[test]
    fn same_file_sees_through_letter_case_on_windows() {
        let t = tempfile::tempdir().unwrap();
        fs::write(t.path().join("readme.md"), "").unwrap();
        assert!(same_file(&t.path().join("readme.md"), &t.path().join("README.MD")).unwrap());
    }

    #[test]
    fn place_stops_at_other_errors() {
        let t = tempfile::tempdir().unwrap();
        let err = place(t.path(), "x", |_| Err(io::Error::other("boom"))).unwrap_err();
        assert_eq!(err.to_string(), "boom");
    }
}
