//! Per-OS integrations. Each function has a portable signature; unsupported
//! platforms return an error and the UI falls back to built-in visuals.

use std::path::Path;

#[cfg(windows)]
mod clipboard;
pub mod clock;
#[cfg(windows)]
mod drives;
#[cfg(windows)]
mod win;
#[cfg(windows)]
mod win_menu;

/// Shows the native shell context menu for `paths` at the cursor and runs the chosen entry.
/// `hwnd` is the raw handle of the owning window; must run on that window's UI thread.
#[cfg(windows)]
pub fn show_context_menu(
    hwnd: isize,
    paths: &[std::path::PathBuf],
    extended: bool,
) -> Result<(), String> {
    let hwnd = ::windows::Win32::Foundation::HWND(hwnd as *mut std::ffi::c_void);
    win_menu::show(hwnd, paths, extended).map_err(|e| e.to_string())
}

/// Bytes a program may still write on the volume holding `path`; `None` where that can't be
/// told.
pub fn free_space(path: &Path) -> Option<u64> {
    #[cfg(windows)]
    {
        drives::free_for(path)
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let c_path = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
        // SAFETY: `statvfs` is plain data that the call fills in; `c_path` is NUL-terminated
        // and outlives the call.
        let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
        if unsafe { libc::statvfs(c_path.as_ptr(), &raw mut stat) } != 0 {
            return None;
        }
        u64::try_from(stat.f_bavail)
            .ok()?
            .checked_mul(u64::try_from(stat.f_frsize).ok()?)
    }
    #[cfg(not(any(windows, unix)))]
    {
        let _ = path;
        None
    }
}

/// Opens a terminal whose working folder is `dir`: Windows Terminal if it is installed,
/// else PowerShell in its own console window.
pub fn open_terminal(dir: &Path) -> Result<(), String> {
    if !dir.is_dir() {
        return Err(format!("\"{}\" is not a folder", dir.display()));
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        /// `CREATE_NEW_CONSOLE`: the shell gets a window of its own.
        const NEW_CONSOLE: u32 = 0x10;
        // The folder is the working directory, never part of a command line, so no name
        // needs quoting.
        let wt = std::process::Command::new("wt.exe")
            .args(["-d", "."])
            .current_dir(dir)
            .spawn();
        if wt.is_ok() {
            return Ok(());
        }
        std::process::Command::new("powershell.exe")
            .arg("-NoExit")
            .current_dir(dir)
            .creation_flags(NEW_CONSOLE)
            .spawn()
            .map(drop)
            .map_err(|e| format!("Could not open a terminal: {e}"))
    }
    #[cfg(not(windows))]
    {
        Err("Opening a terminal is not supported on this system yet".into())
    }
}

/// A drive's root, capacity and free space in bytes.
pub struct DriveInfo {
    pub path: String,
    pub total: u64,
    pub free: u64,
}

/// The drives the space breakdown can scan. Elsewhere there is one root, `/`.
pub fn list_drives() -> Vec<DriveInfo> {
    #[cfg(windows)]
    {
        drives::list()
            .into_iter()
            .map(|d| DriveInfo {
                path: d.path,
                total: d.total,
                free: d.free,
            })
            .collect()
    }
    #[cfg(not(windows))]
    {
        vec![DriveInfo {
            path: "/".into(),
            total: 0,
            free: 0,
        }]
    }
}

/// How much more the drive's Recycle Bin can take, so a batch moved there stays whole, or
/// `None` where it has none, or is set to delete at once (removable and network drives,
/// "Don't move files to the Recycle Bin"), where a delete is for good. This is read from
/// the user's own setting for the drive, less what is already in the bin. Elsewhere the
/// Trash has no limit.
#[cfg_attr(
    not(windows),
    expect(
        clippy::unnecessary_wraps,
        reason = "the Windows version has no limit to give"
    )
)]
pub fn recycle_limit(path: &Path) -> Option<u64> {
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};
        let Some(Component::Prefix(prefix)) = path.components().next() else {
            return None;
        };
        let letter = match prefix.kind() {
            Prefix::Disk(l) | Prefix::VerbatimDisk(l) => char::from(l),
            _ => return None,
        };
        drives::bin_room(letter)
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Some(u64::MAX)
    }
}

/// Puts `paths` on the system clipboard as files, for another program to paste: moved if
/// `cut`, else copied. Only Windows has a file clipboard to mirror to so far.
#[cfg_attr(
    not(windows),
    expect(clippy::unnecessary_wraps, reason = "the Windows version can fail")
)]
pub fn clipboard_write_files(paths: &[std::path::PathBuf], cut: bool) -> Result<(), String> {
    #[cfg(windows)]
    {
        clipboard::write_files(paths, cut)
    }
    #[cfg(not(windows))]
    {
        let _ = (paths, cut);
        Ok(())
    }
}

/// Files another program put on the system clipboard since the last look, as (paths,
/// whether they were cut). What Folio wrote itself is never returned, and neither is the
/// same clipboard twice.
pub fn clipboard_take_files() -> Option<(Vec<std::path::PathBuf>, bool)> {
    #[cfg(windows)]
    {
        clipboard::take_files()
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// Renders the OS thumbnail (or file icon when no thumbnail exists) as PNG,
/// fitted into a `size`×`size` box.
///
/// A file whose content is only in the cloud gets none: drawing one reads the file, and that
/// would download it. Every caller is covered here.
pub fn thumbnail_png(path: &Path, size: u32) -> Result<Vec<u8>, String> {
    if crate::cleanup::might_download(path) {
        return Err("not on this computer".into());
    }
    #[cfg(windows)]
    {
        win::thumbnail_png(path, size)
    }
    #[cfg(not(windows))]
    {
        let _ = (path, size);
        Err("thumbnails are not implemented on this platform yet".into())
    }
}

/// Encodes straight (non-premultiplied) RGBA pixels as PNG.
// Unused on Linux until thumbnails and drag-out are ported there (1.1).
#[cfg_attr(target_os = "linux", allow(dead_code))]
pub fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(rgba).map_err(|e| e.to_string())?;
    writer.finish().map_err(|e| e.to_string())?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_png_signature() {
        let png = encode_png(2, 1, &[255, 0, 0, 255, 0, 0, 255, 128]).unwrap();
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    }

    #[cfg(windows)]
    #[test]
    fn the_bin_of_the_system_drive_can_be_read() {
        // The system drive is a fixed one; its bin is on unless the user turned it off, and
        // either way the answer is a number or a plain "no bin", never a panic.
        let windows = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
        let path = std::path::PathBuf::from(format!("{windows}\\x"));
        let room = recycle_limit(&path);
        println!("bin room on {windows}: {room:?}");
        // A drive that does not exist has no bin.
        assert_eq!(recycle_limit(Path::new("\\\\server\\share\\x")), None);
        assert_eq!(recycle_limit(Path::new("relative\\x")), None);
    }

    #[cfg(windows)]
    #[test]
    fn renders_a_thumbnail_for_a_text_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("note.txt");
        std::fs::write(&file, "hello").unwrap();
        let png = thumbnail_png(&file, 48).unwrap();
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    }
}
