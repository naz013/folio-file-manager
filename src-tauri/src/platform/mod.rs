//! Per-OS integrations. Each function has a portable signature; unsupported
//! platforms return an error and the UI falls back to built-in visuals.

use std::path::Path;

#[cfg(windows)]
mod clipboard;
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
pub fn thumbnail_png(path: &Path, size: u32) -> Result<Vec<u8>, String> {
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
    fn renders_a_thumbnail_for_a_text_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("note.txt");
        std::fs::write(&file, "hello").unwrap();
        let png = thumbnail_png(&file, 48).unwrap();
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    }
}
