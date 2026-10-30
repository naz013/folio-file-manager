//! Windows shell integration.

use std::ffi::c_void;
use std::mem::size_of;
use std::path::Path;

use windows::Win32::Foundation::SIZE;
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC,
    DeleteObject, GetDIBits, GetObjectW, HBITMAP, HGDIOBJ,
};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
use windows::Win32::UI::Shell::{
    IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_RESIZETOFIT,
};
use windows::core::HSTRING;

pub fn thumbnail_png(path: &Path, size: u32) -> Result<Vec<u8>, String> {
    let (width, height, rgba) = shell_image(path, size).map_err(|e| e.to_string())?;
    super::encode_png(width, height, &rgba)
}

/// Asks the shell for the item's thumbnail (falls back to its icon) and returns RGBA pixels.
fn shell_image(path: &Path, size: u32) -> windows::core::Result<(u32, u32, Vec<u8>)> {
    unsafe {
        // Safe to call repeatedly; each blocking-pool thread needs COM initialized once.
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let factory: IShellItemImageFactory =
            SHCreateItemFromParsingName(&HSTRING::from(path.as_os_str()), None)?;
        let side = size as i32;
        let bitmap = factory.GetImage(SIZE { cx: side, cy: side }, SIIGBF_RESIZETOFIT)?;
        let pixels = bitmap_rgba(bitmap);
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        pixels
    }
}

unsafe fn bitmap_rgba(bitmap: HBITMAP) -> windows::core::Result<(u32, u32, Vec<u8>)> {
    unsafe {
        let mut info = BITMAP::default();
        if GetObjectW(
            HGDIOBJ(bitmap.0),
            size_of::<BITMAP>() as i32,
            Some(&mut info as *mut _ as *mut c_void),
        ) == 0
        {
            return Err(windows::core::Error::from_thread());
        }
        let (width, height) = (info.bmWidth as u32, info.bmHeight.unsigned_abs());

        let mut header = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width as i32,
                biHeight: -(height as i32), // top-down rows
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bgra = vec![0u8; (width * height * 4) as usize];
        let dc = CreateCompatibleDC(None);
        let lines = GetDIBits(
            dc,
            bitmap,
            0,
            height,
            Some(bgra.as_mut_ptr() as *mut c_void),
            &mut header,
            DIB_RGB_COLORS,
        );
        let _ = DeleteDC(dc);
        if lines == 0 {
            return Err(windows::core::Error::from_thread());
        }
        Ok((width, height, bgra_premultiplied_to_rgba(bgra)))
    }
}

/// Shell bitmaps are BGRA with premultiplied alpha; photo thumbnails often carry no alpha at all.
fn bgra_premultiplied_to_rgba(mut px: Vec<u8>) -> Vec<u8> {
    let has_alpha = px.as_chunks::<4>().0.iter().any(|p| p[3] != 0);
    for p in px.as_chunks_mut::<4>().0 {
        p.swap(0, 2);
        if !has_alpha {
            p[3] = 255;
        } else if p[3] != 0 && p[3] != 255 {
            let a = p[3] as u32;
            for c in &mut p[..3] {
                *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
    px
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_bgra_and_unpremultiplies() {
        let out = bgra_premultiplied_to_rgba(vec![10, 20, 30, 255, 0, 0, 64, 128]);
        assert_eq!(&out[..4], &[30, 20, 10, 255]);
        assert_eq!(&out[4..], &[128, 0, 0, 128]);
    }

    #[test]
    fn opaque_when_bitmap_has_no_alpha() {
        let out = bgra_premultiplied_to_rgba(vec![1, 2, 3, 0]);
        assert_eq!(out, [3, 2, 1, 255]);
    }
}
