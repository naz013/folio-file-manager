//! Recreating Windows links (junctions and symbolic links) at a copy's destination.
//!
//! Neither is a plain `std` call: a junction has to be written as reparse data, and a
//! symbolic link needs Developer Mode or the privilege to create it. Both are created
//! empty and never followed, so copying a link never copies what it points to.

use std::fs;
use std::io::{self, ErrorKind};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::{OpenOptionsExt, symlink_dir, symlink_file};
use std::os::windows::io::AsRawHandle;
use std::path::Path;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_TAG_INFO, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    FileAttributeTagInfo, GetFileInformationByHandleEx,
};
use windows::Win32::System::IO::DeviceIoControl;

const FILE_READ_ATTRIBUTES: u32 = 0x80;
const GENERIC_WRITE: u32 = 0x4000_0000;
const FILE_SHARE_ALL: u32 = 0x7;
const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
const IO_REPARSE_TAG_MOUNT_POINT: u32 = 0xA000_0003;
const IO_REPARSE_TAG_SYMLINK: u32 = 0xA000_000C;
const FSCTL_SET_REPARSE_POINT: u32 = 0x0009_00A4;
const ERROR_PRIVILEGE_NOT_HELD: i32 = 1314;
/// What a volume without reparse points (FAT, exFAT, some network shares) answers.
const ERROR_INVALID_FUNCTION: i32 = 1;
const ERROR_NOT_SUPPORTED: i32 = 50;

/// Creates at `dst` a link of the same kind as `src`, pointing at `target` (what
/// `read_link` returned for `src`), failing if `dst` exists. Reports `Unsupported` for
/// a kind of link that can't be recreated here, or when symbolic links aren't allowed.
pub fn copy(src: &Path, target: &Path, dst: &Path) -> io::Result<()> {
    let (attributes, tag) = reparse_info(src)?;
    match tag {
        IO_REPARSE_TAG_MOUNT_POINT => junction(target, dst),
        IO_REPARSE_TAG_SYMLINK => symlink(attributes & FILE_ATTRIBUTE_DIRECTORY != 0, target, dst),
        _ => Err(io::Error::new(
            ErrorKind::Unsupported,
            "This kind of link can't be copied",
        )),
    }
}

/// The attributes and reparse tag of the link `path` itself.
fn reparse_info(path: &Path) -> io::Result<(u32, u32)> {
    let file = fs::OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES)
        .share_mode(FILE_SHARE_ALL)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS.0 | FILE_FLAG_OPEN_REPARSE_POINT.0)
        .open(path)?;
    let mut info = FILE_ATTRIBUTE_TAG_INFO::default();
    #[expect(clippy::cast_possible_truncation, reason = "an 8-byte struct")]
    let size = size_of::<FILE_ATTRIBUTE_TAG_INFO>() as u32;
    // SAFETY: `file` keeps the handle open, and `info` is a local
    // FILE_ATTRIBUTE_TAG_INFO of `size` bytes, the struct FileAttributeTagInfo writes.
    unsafe {
        GetFileInformationByHandleEx(
            HANDLE(file.as_raw_handle()),
            FileAttributeTagInfo,
            (&raw mut info).cast(),
            size,
        )
    }
    .map_err(|_| io::Error::last_os_error())?;
    Ok((info.FileAttributes, info.ReparseTag))
}

fn symlink(directory: bool, target: &Path, dst: &Path) -> io::Result<()> {
    // std asks for the unprivileged form first, which Developer Mode allows.
    let made = if directory {
        symlink_dir(target, dst)
    } else {
        symlink_file(target, dst)
    };
    made.map_err(|e| match e.raw_os_error() {
        Some(ERROR_PRIVILEGE_NOT_HELD) => io::Error::new(
            ErrorKind::Unsupported,
            "Symbolic links need Developer Mode or administrator rights",
        ),
        _ => volume_refused(e),
    })
}

/// A volume that can't hold links answers with codes `std` doesn't map; they mean the
/// link is skipped and reported, not that the whole copy fails.
fn volume_refused(e: io::Error) -> io::Error {
    match e.raw_os_error() {
        Some(ERROR_INVALID_FUNCTION | ERROR_NOT_SUPPORTED) => {
            io::Error::new(ErrorKind::Unsupported, "This volume can't hold links")
        }
        _ => e,
    }
}

/// A junction is an empty folder carrying mount-point reparse data.
fn junction(target: &Path, dst: &Path) -> io::Result<()> {
    let data = mount_point_data(target)?;
    fs::create_dir(dst)?;
    // Not following anything: `dst` is the empty folder just created.
    set_reparse_point(dst, &data)
        .map_err(volume_refused)
        .inspect_err(|_| {
            let _ = fs::remove_dir(dst);
        })
}

/// The `REPARSE_DATA_BUFFER` for a mount point to `target`: header, then the
/// substitute name (`\??\` + path) and the print name (the path), both NUL-terminated.
fn mount_point_data(target: &Path) -> io::Result<Vec<u8>> {
    let units = |s: &str| s.encode_utf16().collect::<Vec<u16>>();
    let mut print: Vec<u16> = target.as_os_str().encode_wide().collect();
    for prefix in [r"\??\", r"\\?\"] {
        if print.starts_with(&units(prefix)) {
            print.drain(..4);
        }
    }
    // `read_link` has already rewritten other forms (network paths, device names), so
    // only drive and volume targets can be turned back into the exact original.
    let plain = String::from_utf16_lossy(&print);
    let drive = plain.as_bytes().get(1) == Some(&b':') && plain.as_bytes()[0].is_ascii_alphabetic();
    if !drive && !plain.starts_with("Volume{") {
        return Err(io::Error::new(
            ErrorKind::Unsupported,
            "This kind of junction can't be copied",
        ));
    }
    let substitute = [units(r"\??\"), print.clone()].concat();
    let too_long = |_| io::Error::new(ErrorKind::InvalidInput, "The link's target is too long");
    let sub_len = u16::try_from(substitute.len() * 2).map_err(too_long)?;
    let print_len = u16::try_from(print.len() * 2).map_err(too_long)?;
    // Four u16 offsets/lengths, then both names and a NUL after each.
    let data_len = 8 + u32::from(sub_len) + 2 + u32::from(print_len) + 2;
    let data_len = u16::try_from(data_len).map_err(too_long)?;

    let mut buffer = Vec::with_capacity(8 + usize::from(data_len));
    buffer.extend(IO_REPARSE_TAG_MOUNT_POINT.to_le_bytes());
    buffer.extend(data_len.to_le_bytes());
    buffer.extend(0u16.to_le_bytes());
    for field in [0, sub_len, sub_len + 2, print_len] {
        buffer.extend(u16::to_le_bytes(field));
    }
    for unit in substitute.iter().chain(&[0]).chain(&print).chain(&[0]) {
        buffer.extend(unit.to_le_bytes());
    }
    Ok(buffer)
}

fn set_reparse_point(dir: &Path, data: &[u8]) -> io::Result<()> {
    let folder = fs::OpenOptions::new()
        .access_mode(GENERIC_WRITE)
        .share_mode(FILE_SHARE_ALL)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS.0 | FILE_FLAG_OPEN_REPARSE_POINT.0)
        .open(dir)?;
    #[expect(
        clippy::cast_possible_truncation,
        reason = "at most 8 + u16::MAX bytes"
    )]
    let size = data.len() as u32;
    // Required when there is no OVERLAPPED; the value itself isn't used.
    let mut returned = 0u32;
    // SAFETY: `folder` keeps the handle open, `data` is a complete reparse buffer of
    // `size` bytes that the call only reads, and `returned` is a local the call writes.
    unsafe {
        DeviceIoControl(
            HANDLE(folder.as_raw_handle()),
            FSCTL_SET_REPARSE_POINT,
            Some(data.as_ptr().cast()),
            size,
            None,
            0,
            Some(&raw mut returned),
            None,
        )
    }
    .map_err(|_| io::Error::last_os_error())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mount_point_data_has_the_documented_layout() {
        let data = mount_point_data(Path::new(r"C:\a")).unwrap();
        let u16_at = |i: usize| u16::from_le_bytes([data[i], data[i + 1]]);

        assert_eq!(&data[..4], IO_REPARSE_TAG_MOUNT_POINT.to_le_bytes());
        // `\??\C:\a` is 8 units, `C:\a` is 4; each name is followed by a NUL.
        assert_eq!(
            u16_at(4),
            8 + 16 + 2 + 8 + 2,
            "length after the 8-byte header"
        );
        assert_eq!((u16_at(8), u16_at(10)), (0, 16), "substitute name");
        assert_eq!((u16_at(12), u16_at(14)), (18, 8), "print name");
        assert_eq!(data.len(), 8 + usize::from(u16_at(4)));
        assert_eq!(u16_at(16 + 16), 0, "NUL after the substitute name");
    }

    #[test]
    fn a_target_that_is_not_a_drive_or_volume_path_is_unsupported() {
        for target in [r"\\server\share", r"relative\dir", r"\Device\Mup\x"] {
            let e = mount_point_data(Path::new(target)).unwrap_err();
            assert_eq!(e.kind(), ErrorKind::Unsupported, "{target}");
        }
        assert!(mount_point_data(Path::new(r"\\?\Volume{1234}\x")).is_ok());
    }

    #[test]
    fn a_volume_that_refuses_reparse_points_means_skip() {
        for code in [ERROR_INVALID_FUNCTION, ERROR_NOT_SUPPORTED] {
            let e = volume_refused(io::Error::from_raw_os_error(code));
            assert_eq!(e.kind(), ErrorKind::Unsupported);
        }
        let other = volume_refused(io::Error::from_raw_os_error(5));
        assert_ne!(other.kind(), ErrorKind::Unsupported);
    }
}
