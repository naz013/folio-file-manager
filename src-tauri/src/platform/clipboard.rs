//! The Windows clipboard as Explorer uses it for files: a `CF_HDROP` list of paths and a
//! "Preferred `DropEffect`" saying whether they are to be copied or moved. Folio writes it on
//! its own cut and copy, so Explorer can paste what was marked, and reads it to pick up
//! files that were cut or copied in Explorer. It never touches the clipboard otherwise.

use std::ffi::{OsString, c_void};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, GetClipboardSequenceNumber,
    IsClipboardFormatAvailable, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{
    GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock,
};
use windows::core::w;

const CF_HDROP: u32 = 15;
const DROPEFFECT_COPY: u32 = 1;
const DROPEFFECT_MOVE: u32 = 2;
/// Size of the `DROPFILES` header that precedes the paths.
const HEADER: usize = 20;

/// The clipboard's sequence number right after Folio wrote it, and the last one read from
/// someone else, so each foreign copy is picked up once.
static OWN: AtomicU32 = AtomicU32::new(0);
static SEEN: AtomicU32 = AtomicU32::new(0);

/// A `DROPFILES` block: the header, then the wide paths, each ended by a NUL, and a final NUL.
fn build_dropfiles(paths: &[PathBuf]) -> Vec<u8> {
    let mut bytes = Vec::new();
    // pFiles (where the paths start), pt (x, y), fNC, fWide.
    bytes.extend(20u32.to_le_bytes());
    bytes.extend(0i32.to_le_bytes());
    bytes.extend(0i32.to_le_bytes());
    bytes.extend(0u32.to_le_bytes());
    bytes.extend(1u32.to_le_bytes());
    for path in paths {
        for unit in path.as_os_str().encode_wide().chain(std::iter::once(0)) {
            bytes.extend(unit.to_le_bytes());
        }
    }
    bytes.extend(0u16.to_le_bytes());
    bytes
}

/// The paths in a `DROPFILES` block; empty if it isn't one this can read.
fn parse_dropfiles(bytes: &[u8]) -> Vec<PathBuf> {
    let word = |at: usize| {
        bytes
            .get(at..at + 4)
            .and_then(|b| <[u8; 4]>::try_from(b).ok())
            .map(u32::from_le_bytes)
    };
    let (Some(start), Some(wide)) = (word(0), word(16)) else {
        return Vec::new();
    };
    let start = start as usize;
    if wide == 0 || start < HEADER || start > bytes.len() {
        // ANSI lists are rare and the code page is unknown: leave them.
        return Vec::new();
    }
    let (pairs, _) = bytes[start..].as_chunks::<2>();
    let units: Vec<u16> = pairs.iter().map(|c| u16::from_le_bytes(*c)).collect();
    units
        .split(|&u| u == 0)
        .take_while(|s| !s.is_empty())
        .map(|s| PathBuf::from(OsString::from_wide(s)))
        .collect()
}

/// Holds the clipboard open for as long as it lives; another program may have it for a
/// moment, so opening is retried.
struct Open;

impl Open {
    fn new() -> Result<Self, String> {
        for _ in 0..10 {
            // SAFETY: no window handle is needed; the clipboard is closed again on drop.
            if unsafe { OpenClipboard(None) }.is_ok() {
                return Ok(Self);
            }
            std::thread::sleep(Duration::from_millis(15));
        }
        Err("The clipboard is in use by another program".into())
    }
}

impl Drop for Open {
    fn drop(&mut self) {
        // SAFETY: only a successfully opened clipboard makes an `Open`.
        let _ = unsafe { CloseClipboard() };
    }
}

/// Gives the clipboard `bytes` as `format`. On success the clipboard owns the memory.
fn put(format: u32, bytes: &[u8]) -> Result<(), String> {
    // SAFETY: the block is allocated here, filled while locked, and either handed to the
    // clipboard (which then owns it) or freed.
    unsafe {
        let block = GlobalAlloc(GMEM_MOVEABLE, bytes.len()).map_err(|e| e.to_string())?;
        let dst = GlobalLock(block);
        if dst.is_null() {
            let _ = GlobalFree(Some(block));
            return Err("Out of memory".into());
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), dst.cast::<u8>(), bytes.len());
        let _ = GlobalUnlock(block);
        if let Err(e) = SetClipboardData(format, Some(HANDLE(block.0))) {
            let _ = GlobalFree(Some(block));
            return Err(e.to_string());
        }
    }
    Ok(())
}

/// The bytes of `format` on the clipboard, which must be open.
fn get(format: u32) -> Option<Vec<u8>> {
    // SAFETY: the clipboard is open (the caller holds an `Open`); the handle belongs to it
    // and is only read while locked, within the size the system reports.
    unsafe {
        let handle = GetClipboardData(format).ok()?;
        let block = HGLOBAL(handle.0.cast::<c_void>());
        let size = GlobalSize(block);
        let src = GlobalLock(block);
        if src.is_null() {
            return None;
        }
        let bytes = std::slice::from_raw_parts(src.cast::<u8>(), size).to_vec();
        let _ = GlobalUnlock(block);
        Some(bytes)
    }
}

fn effect_format() -> u32 {
    // SAFETY: a constant, NUL-terminated name.
    unsafe { RegisterClipboardFormatW(w!("Preferred DropEffect")) }
}

/// Puts `paths` on the system clipboard for Explorer to paste: moved if `cut`, else copied.
pub fn write_files(paths: &[PathBuf], cut: bool) -> Result<(), String> {
    if paths.is_empty() {
        return Ok(());
    }
    let open = Open::new()?;
    // SAFETY: the clipboard is open.
    unsafe { EmptyClipboard() }.map_err(|e| e.to_string())?;
    // Whatever happens next, the clipboard now holds Folio's data, not a foreign copy.
    // SAFETY: a plain query, made while the clipboard is held so no one can write in between.
    OWN.store(unsafe { GetClipboardSequenceNumber() }, Ordering::Relaxed);
    put(CF_HDROP, &build_dropfiles(paths))?;
    let effect = if cut {
        DROPEFFECT_MOVE
    } else {
        DROPEFFECT_COPY
    };
    put(effect_format(), &effect.to_le_bytes())?;
    drop(open);
    Ok(())
}

/// Files that someone else (Explorer, usually) put on the clipboard since the last look, as
/// (paths, whether they were cut). Each clipboard is returned once, and what Folio wrote
/// itself is never returned.
pub fn take_files() -> Option<(Vec<PathBuf>, bool)> {
    // SAFETY: a plain query.
    if unsafe { IsClipboardFormatAvailable(CF_HDROP) }.is_err() {
        return None;
    }
    let _open = Open::new().ok()?;
    // Read while the clipboard is held, so the number matches the data read below.
    // SAFETY: a plain query.
    let seq = unsafe { GetClipboardSequenceNumber() };
    if seq == OWN.load(Ordering::Relaxed) || seq == SEEN.load(Ordering::Relaxed) {
        return None;
    }
    let paths = parse_dropfiles(&get(CF_HDROP)?);
    let cut = get(effect_format())
        .and_then(|b| <[u8; 4]>::try_from(b.get(..4)?).ok())
        .map(u32::from_le_bytes)
        .is_some_and(|e| e & DROPEFFECT_MOVE != 0 && e & DROPEFFECT_COPY == 0);
    SEEN.store(seq, Ordering::Relaxed);
    (!paths.is_empty()).then_some((paths, cut))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_list_round_trips_with_unicode_and_network_names() {
        let paths = vec![
            PathBuf::from(r"C:\Users\me\Documents\café ☕.txt"),
            PathBuf::from(r"\\server\share\folder\a b.png"),
            PathBuf::from(r"D:\x"),
        ];
        assert_eq!(parse_dropfiles(&build_dropfiles(&paths)), paths);
    }

    #[test]
    fn the_block_has_the_header_explorer_expects() {
        let bytes = build_dropfiles(&[PathBuf::from(r"C:\a")]);
        assert_eq!(u32::from_le_bytes(bytes[0..4].try_into().unwrap()), 20);
        assert_eq!(
            u32::from_le_bytes(bytes[16..20].try_into().unwrap()),
            1,
            "wide"
        );
        // "C:\a" and its NUL, then the closing NUL.
        assert_eq!(bytes.len(), 20 + (4 + 1) * 2 + 2);
    }

    #[test]
    fn a_block_that_cannot_be_read_gives_no_paths() {
        assert_eq!(parse_dropfiles(&[]), Vec::<PathBuf>::new());
        assert_eq!(parse_dropfiles(&[0; 10]), Vec::<PathBuf>::new());
        // An ANSI list (fWide = 0) is left alone.
        let mut ansi = build_dropfiles(&[PathBuf::from("C:/a")]);
        ansi[16] = 0;
        assert_eq!(parse_dropfiles(&ansi), Vec::<PathBuf>::new());
        // A start offset past the end.
        let mut bad = build_dropfiles(&[PathBuf::from("C:/a")]);
        bad[0..4].copy_from_slice(&9999u32.to_le_bytes());
        assert_eq!(parse_dropfiles(&bad), Vec::<PathBuf>::new());
    }

    /// Uses the real clipboard, which would disturb whoever is at the keyboard:
    /// `cargo test -- --ignored clipboard_round_trip`.
    #[test]
    #[ignore = "overwrites the system clipboard"]
    fn clipboard_round_trip() {
        let paths = vec![PathBuf::from(r"C:\Windows\notepad.exe")];
        write_files(&paths, true).unwrap();
        // Folio's own write is never picked up again...
        assert_eq!(take_files(), None);
        // ...but the same data is there for another program.
        let _open = Open::new().unwrap();
        assert_eq!(parse_dropfiles(&get(CF_HDROP).unwrap()), paths);
    }
}
