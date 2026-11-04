//! Thumbnails on Linux. GTK's own picture loaders (gdk-pixbuf) draw the pictures the desktop
//! can show: PNG, JPEG, GIF, WebP and the rest of what is installed. Everything else gets none,
//! and the page shows the file's icon instead.

use gdk_pixbuf::Pixbuf;
use std::path::Path;

pub fn thumbnail_png(path: &Path, size: u32) -> Result<Vec<u8>, String> {
    // A pipe or device would block a read; only plain files are drawn.
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if !meta.is_file() {
        return Err("not a plain file".into());
    }
    let edge = i32::try_from(size).map_err(|e| e.to_string())?;
    // A picture that already fits is left at its own size: scaling up would only blur it.
    let fits = Pixbuf::file_info(path).is_some_and(|(_, w, h)| w <= edge && h <= edge);
    let pixbuf = if fits {
        Pixbuf::from_file(path)
    } else {
        // Scaled while it is decoded, so a large picture is never held whole.
        Pixbuf::from_file_at_scale(path, edge, edge, true)
    }
    .map_err(|e| e.to_string())?;
    pixbuf
        .save_to_bufferv("png", &[])
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draws_a_png_within_the_box() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("wide.png");
        let wide = crate::platform::encode_png(40, 20, &[200; 40 * 20 * 4]).unwrap();
        std::fs::write(&file, wide).unwrap();
        let png = thumbnail_png(&file, 16).unwrap();
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        let small = Pixbuf::from_read(std::io::Cursor::new(png)).unwrap();
        assert!(small.width() <= 16 && small.height() <= 16);
        assert_eq!(small.width(), 16, "the long side fills the box");
    }

    #[test]
    fn a_small_picture_is_not_blown_up() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("icon.png");
        let icon = crate::platform::encode_png(8, 8, &[200; 8 * 8 * 4]).unwrap();
        std::fs::write(&file, icon).unwrap();
        let png = thumbnail_png(&file, 64).unwrap();
        let small = Pixbuf::from_read(std::io::Cursor::new(png)).unwrap();
        assert_eq!((small.width(), small.height()), (8, 8));
    }

    #[test]
    fn a_text_file_and_a_folder_get_none() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("note.txt");
        std::fs::write(&file, "hello").unwrap();
        assert!(thumbnail_png(&file, 48).is_err());
        assert!(thumbnail_png(dir.path(), 48).is_err());
    }
}
