//! Quick Look in the core: the start of a text file, and a `preview://` protocol that
//! serves the one file being previewed to the webview (with byte ranges, so audio, video
//! and PDF can seek). Nothing else is readable through the protocol: the page names the
//! file it is about to show, and naming another one replaces it.

use std::borrow::Cow;
use std::fs;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::http::{Request, Response, StatusCode, header};

pub const SCHEME: &str = "preview";

/// The file the webview may currently read through `preview://`.
#[derive(Default)]
pub struct Current(Mutex<Option<PathBuf>>);

impl Current {
    /// Makes `path` the only readable file (or none).
    pub fn set(&self, path: Option<PathBuf>) {
        *self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = path;
    }

    fn is(&self, path: &Path) -> bool {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_deref()
            == Some(path)
    }
}

/// What Quick Look says for a file that is only in the cloud (a `OneDrive` placeholder, say).
/// The page matches on this text, so it is one string in both places.
pub const CLOUD_ONLY: &str = "Only in the cloud";

/// Whether reading `path` would download it. Showing a file is something the person chose; the
/// neighbours Quick Look moves over on the way are not. A file whose attributes cannot be read
/// is not counted: opening it fails on its own.
pub fn cloud_only(path: &Path) -> bool {
    crate::cleanup::might_download(path) && fs::metadata(path).is_ok()
}

/// How much of a text file Quick Look reads: enough to read, never the whole of a huge log.
const PREVIEW_LIMIT: usize = 256 * 1024;
/// The most one ranged response carries, and the most a response without a range does.
const CHUNK: u64 = 8 * 1024 * 1024;
const WHOLE_FILE_LIMIT: u64 = 64 * 1024 * 1024;

/// The start of a text file, for Quick Look.
#[derive(Debug, serde::Serialize)]
pub struct TextPreview {
    pub text: String,
    /// The file goes on past `text`.
    pub truncated: bool,
}

/// Reads the first part of `path` as text. Fails for anything that is not a regular file and
/// for files that look binary (a NUL byte in the first 8 KiB), so the caller can show
/// something else.
pub fn read_text_preview(path: &Path) -> io::Result<TextPreview> {
    if cloud_only(path) {
        return Err(io::Error::other(CLOUD_ONLY));
    }
    let file = fs::File::open(path)?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "This is not a file",
        ));
    }
    let mut buf = Vec::new();
    file.take(PREVIEW_LIMIT as u64 + 1).read_to_end(&mut buf)?;
    let truncated = buf.len() > PREVIEW_LIMIT;
    buf.truncate(PREVIEW_LIMIT);
    if buf.iter().take(8192).any(|&b| b == 0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "This file is not text",
        ));
    }
    if truncated {
        // The cut may fall inside a character; drop the broken end rather than show it.
        if let Err(e) = std::str::from_utf8(&buf)
            && e.error_len().is_none()
        {
            buf.truncate(e.valid_up_to());
        }
    }
    Ok(TextPreview {
        text: String::from_utf8_lossy(&buf).into_owned(),
        truncated,
    })
}

fn content_type(path: &Path) -> &'static str {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        "avif" => "image/avif",
        "pdf" => "application/pdf",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" | "oga" | "opus" => "audio/ogg",
        "m4a" | "aac" => "audio/mp4",
        "flac" => "audio/flac",
        "mp4" | "m4v" => "video/mp4",
        "webm" => "video/webm",
        "mov" => "video/quicktime",
        "ogv" => "video/ogg",
        _ => "application/octet-stream",
    }
}

/// The byte range a `Range: bytes=a-b` header asks for, within a file of `len` bytes.
fn parse_range(header: &str, len: u64) -> Option<(u64, u64)> {
    let spec = header.strip_prefix("bytes=")?;
    let (from, to) = spec.split_once('-')?;
    let (start, end) = if from.is_empty() {
        // "-N": the last N bytes.
        let n: u64 = to.parse().ok()?;
        (len.saturating_sub(n), len.checked_sub(1)?)
    } else {
        let start: u64 = from.parse().ok()?;
        let end = if to.is_empty() {
            len.checked_sub(1)?
        } else {
            to.parse::<u64>().ok()?.min(len.checked_sub(1)?)
        };
        (start, end)
    };
    (start <= end).then_some((start, end))
}

pub fn status(code: StatusCode) -> Response<Vec<u8>> {
    Response::builder().status(code).body(Vec::new()).unwrap()
}

/// Answers a `preview://` request. Only the file named last through `Current::set` is
/// served, whole or by range.
pub fn handle(current: &Current, request: &Request<Vec<u8>>) -> Response<Vec<u8>> {
    let raw = request.uri().path().trim_start_matches('/');
    let Ok(decoded) = percent_encoding::percent_decode_str(raw).decode_utf8() else {
        return status(StatusCode::BAD_REQUEST);
    };
    let path = PathBuf::from(Cow::into_owned(decoded));
    if !current.is(&path) {
        return status(StatusCode::FORBIDDEN);
    }
    // 409: the file is not here to read. The page has been told before it asked.
    if cloud_only(&path) {
        return status(StatusCode::CONFLICT);
    }
    let Ok(mut file) = fs::File::open(&path) else {
        return status(StatusCode::NOT_FOUND);
    };
    let len = match file.metadata() {
        Ok(m) if m.is_file() => m.len(),
        _ => return status(StatusCode::NOT_FOUND),
    };
    let range = request
        .headers()
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok());
    let (start, end, partial) = match range.and_then(|h| parse_range(h, len)) {
        Some((s, e)) => (s, e.min(s + CHUNK - 1), true),
        None if range.is_some() => return status(StatusCode::RANGE_NOT_SATISFIABLE),
        None => (0, len.saturating_sub(1).min(WHOLE_FILE_LIMIT - 1), false),
    };
    let size = if len == 0 { 0 } else { end - start + 1 };
    let mut body = vec![0; usize::try_from(size).unwrap_or(0)];
    if file.seek(SeekFrom::Start(start)).is_err() || file.read_exact(&mut body).is_err() {
        return status(StatusCode::INTERNAL_SERVER_ERROR);
    }
    let mut response = Response::builder()
        .header(header::CONTENT_TYPE, content_type(&path))
        .header(header::ACCEPT_RANGES, "bytes")
        .header("Access-Control-Allow-Origin", "*");
    if partial {
        response = response
            .status(StatusCode::PARTIAL_CONTENT)
            .header(header::CONTENT_RANGE, format!("bytes {start}-{end}/{len}"));
    }
    response.body(body).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Marks `path` as a file the sync provider has not downloaded. Only OFFLINE can be set from
    /// user mode; the RECALL_* bits `OneDrive` uses are read the same way.
    #[cfg(windows)]
    fn mark_offline(path: &Path) {
        use windows::Win32::Storage::FileSystem::{FILE_ATTRIBUTE_OFFLINE, SetFileAttributesW};
        use windows::core::HSTRING;
        // SAFETY: the path is an owned HSTRING that outlives the call.
        unsafe { SetFileAttributesW(&HSTRING::from(path.as_os_str()), FILE_ATTRIBUTE_OFFLINE) }
            .unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn a_cloud_only_file_is_not_read_for_a_text_preview() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("note.txt");
        fs::write(&file, "hello").unwrap();
        assert!(read_text_preview(&file).is_ok(), "a local file still reads");
        mark_offline(&file);
        let err = read_text_preview(&file).unwrap_err();
        assert_eq!(err.to_string(), CLOUD_ONLY);
    }

    #[cfg(windows)]
    #[test]
    fn a_cloud_only_file_is_not_served_through_the_protocol() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.png");
        fs::write(&file, b"0123456789").unwrap();
        let current = Current::default();
        current.set(Some(file.clone()));
        let encoded: String = percent_encoding::utf8_percent_encode(
            &file.to_string_lossy(),
            percent_encoding::NON_ALPHANUMERIC,
        )
        .to_string();
        let get = || {
            let request = Request::builder()
                .uri(format!("preview://localhost/{encoded}"))
                .body(Vec::new())
                .unwrap();
            handle(&current, &request).status()
        };
        assert_eq!(get(), StatusCode::OK);
        mark_offline(&file);
        assert_eq!(get(), StatusCode::CONFLICT);
    }

    #[test]
    fn a_missing_file_is_not_called_cloud_only() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!cloud_only(&dir.path().join("gone.txt")));
    }

    #[test]
    fn text_preview_reads_the_start_and_says_when_it_stopped() {
        let dir = tempfile::tempdir().unwrap();
        let small = dir.path().join("a.txt");
        fs::write(&small, "hello\nworld").unwrap();
        let p = read_text_preview(&small).unwrap();
        assert_eq!((p.text.as_str(), p.truncated), ("hello\nworld", false));

        let big = dir.path().join("big.log");
        fs::write(&big, "x".repeat(PREVIEW_LIMIT + 10)).unwrap();
        let p = read_text_preview(&big).unwrap();
        assert_eq!((p.text.len(), p.truncated), (PREVIEW_LIMIT, true));
    }

    #[test]
    fn text_preview_refuses_binary_files() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("a.bin");
        fs::write(&bin, [0x89, b'P', 0, 0, 1, 2]).unwrap();
        let err = read_text_preview(&bin).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn text_preview_accepts_a_nul_after_the_first_8_kib_and_trims_a_split_character() {
        let dir = tempfile::tempdir().unwrap();
        let late_nul = dir.path().join("late.txt");
        let mut bytes = vec![b'a'; 9000];
        bytes.push(0);
        fs::write(&late_nul, &bytes).unwrap();
        assert!(read_text_preview(&late_nul).is_ok());

        // A two-byte character straddling the limit is dropped, not shown broken.
        let split = dir.path().join("split.txt");
        let mut text = "a".repeat(PREVIEW_LIMIT - 1).into_bytes();
        text.extend("é".as_bytes());
        text.extend(b"tail");
        fs::write(&split, &text).unwrap();
        let p = read_text_preview(&split).unwrap();
        assert!(p.truncated);
        assert!(!p.text.contains('\u{FFFD}'), "no broken character");
    }

    #[test]
    fn text_preview_refuses_a_folder() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read_text_preview(dir.path()).is_err());
    }

    #[test]
    fn ranges_are_clamped_to_the_file() {
        assert_eq!(parse_range("bytes=0-9", 100), Some((0, 9)));
        assert_eq!(parse_range("bytes=90-", 100), Some((90, 99)));
        assert_eq!(parse_range("bytes=-10", 100), Some((90, 99)));
        assert_eq!(parse_range("bytes=50-500", 100), Some((50, 99)));
        assert_eq!(parse_range("bytes=60-50", 100), None);
        assert_eq!(parse_range("bytes=0-", 0), None);
        assert_eq!(parse_range("lines=1-2", 100), None);
    }

    #[test]
    fn only_the_named_file_is_served() {
        let dir = tempfile::tempdir().unwrap();
        let shown = dir.path().join("a.png");
        let other = dir.path().join("b.png");
        fs::write(&shown, b"0123456789").unwrap();
        fs::write(&other, b"secret").unwrap();
        let current = Current::default();
        current.set(Some(shown.clone()));
        let get = |path: &Path, range: Option<&str>| {
            let encoded: String = percent_encoding::utf8_percent_encode(
                &path.to_string_lossy(),
                percent_encoding::NON_ALPHANUMERIC,
            )
            .to_string();
            let mut req = Request::builder().uri(format!("http://preview.localhost/{encoded}"));
            if let Some(r) = range {
                req = req.header(header::RANGE, r);
            }
            handle(&current, &req.body(Vec::new()).unwrap())
        };

        let whole = get(&shown, None);
        assert_eq!((whole.status(), whole.body().len()), (StatusCode::OK, 10));
        let part = get(&shown, Some("bytes=2-4"));
        assert_eq!(part.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(part.body().as_slice(), b"234");
        assert_eq!(get(&other, None).status(), StatusCode::FORBIDDEN);

        current.set(Some(other.clone()));
        assert_eq!(get(&shown, None).status(), StatusCode::FORBIDDEN);
        assert_eq!(get(&other, None).body().as_slice(), b"secret");
        current.set(None);
        assert_eq!(get(&other, None).status(), StatusCode::FORBIDDEN);
    }
}
