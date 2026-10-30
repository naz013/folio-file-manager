//! Per-OS integrations. Each function has a portable signature; unsupported
//! platforms return an error and the UI falls back to built-in visuals.

use std::path::Path;

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
