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

/// How a text file's bytes are read as characters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    Utf8,
    Utf16Le,
    Utf16Be,
    /// A legacy code page: every byte is one character.
    Windows1252,
}

impl Encoding {
    /// The name a viewer is told and may ask for.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Utf8 => "utf-8",
            Self::Utf16Le => "utf-16le",
            Self::Utf16Be => "utf-16be",
            Self::Windows1252 => "windows-1252",
        }
    }

    /// The encoding a viewer asked for by name; `None` is not a name Folio knows.
    pub fn from_label(label: &str) -> Option<Self> {
        [Self::Utf8, Self::Utf16Le, Self::Utf16Be, Self::Windows1252]
            .into_iter()
            .find(|e| e.label() == label)
    }

    const fn wide(self) -> bool {
        matches!(self, Self::Utf16Le | Self::Utf16Be)
    }
}

/// The byte-order mark at the start of a file: the encoding it names and its length.
fn bom_of(head: &[u8]) -> Option<(Encoding, u64)> {
    if head.starts_with(&[0xEF, 0xBB, 0xBF]) {
        Some((Encoding::Utf8, 3))
    } else if head.starts_with(&[0xFF, 0xFE]) {
        Some((Encoding::Utf16Le, 2))
    } else if head.starts_with(&[0xFE, 0xFF]) {
        Some((Encoding::Utf16Be, 2))
    } else {
        None
    }
}

/// How much of the start of a file decides its encoding when it has no mark.
const SNIFF: usize = 64 * 1024;

/// Guesses the encoding of a file with no mark from its start: UTF-16 when the zero bytes sit
/// in every other place (Latin text in UTF-16 is that), Windows-1252 when it is not valid
/// UTF-8, UTF-8 otherwise. `None` for a file with other zero bytes: that is binary.
fn sniff(head: &[u8]) -> Option<Encoding> {
    let window = &head[..head.len().min(8192)];
    let (mut even, mut odd) = (0usize, 0usize);
    for (i, &b) in window.iter().enumerate() {
        if b == 0 {
            if i % 2 == 0 {
                even += 1;
            } else {
                odd += 1;
            }
        }
    }
    let pairs = window.len() / 2;
    if even + odd > 0 {
        // A fifth of the units have a zero high byte, and almost none a zero low one.
        if odd * 5 >= pairs && even * 50 <= odd {
            return Some(Encoding::Utf16Le);
        }
        if even * 5 >= pairs && odd * 50 <= even {
            return Some(Encoding::Utf16Be);
        }
        return None;
    }
    match std::str::from_utf8(head) {
        // Invalid bytes. A character cut by the end of the window is not one, unless the window
        // is the whole file: then a last lone lead byte (`caf\xE9`) is what it looks like.
        Err(e) if e.error_len().is_some() || head.len() < SNIFF => Some(Encoding::Windows1252),
        _ => Some(Encoding::Utf8),
    }
}

/// Windows-1252's characters for the bytes 0x80 to 0x9F; the rest match Latin-1.
const CP1252_HIGH: [char; 32] = [
    '\u{20AC}', '\u{81}', '\u{201A}', '\u{0192}', '\u{201E}', '\u{2026}', '\u{2020}', '\u{2021}',
    '\u{02C6}', '\u{2030}', '\u{0160}', '\u{2039}', '\u{0152}', '\u{8D}', '\u{017D}', '\u{8F}',
    '\u{90}', '\u{2018}', '\u{2019}', '\u{201C}', '\u{201D}', '\u{2022}', '\u{2013}', '\u{2014}',
    '\u{02DC}', '\u{2122}', '\u{0161}', '\u{203A}', '\u{0153}', '\u{9D}', '\u{017E}', '\u{0178}',
];

fn decode_1252(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&b| match b {
            0x80..=0x9F => CP1252_HIGH[usize::from(b - 0x80)],
            _ => char::from(b),
        })
        .collect()
}

/// The UTF-16 unit at byte `i` of `bytes`, if there are two bytes there.
fn unit_at(bytes: &[u8], i: usize, big: bool) -> Option<u16> {
    let c: [u8; 2] = bytes.get(i..i + 2)?.try_into().ok()?;
    Some(if big {
        u16::from_be_bytes(c)
    } else {
        u16::from_le_bytes(c)
    })
}

/// Decodes whole UTF-16 units; a lone half of a pair or a stray last byte becomes U+FFFD.
fn decode_16(bytes: &[u8], big: bool) -> String {
    let units = (0..bytes.len() / 2).filter_map(|i| unit_at(bytes, i * 2, big));
    let mut out: String = char::decode_utf16(units)
        .map(|r| r.unwrap_or(char::REPLACEMENT_CHARACTER))
        .collect();
    if bytes.len() % 2 == 1 {
        out.push(char::REPLACEMENT_CHARACTER);
    }
    out
}

/// Which bytes of a UTF-16 piece are whole characters: it starts after the second half of a pair
/// when `inside` (the piece does not start at the start of the text), and ends before the first
/// half of a pair the length cut, unless it is the end of the file (which keeps a last odd byte).
fn bounds_16(buf: &[u8], inside: bool, at_end: bool, big: bool) -> (usize, usize) {
    let skip = if inside && unit_at(buf, 0, big).is_some_and(|u| (0xDC00..0xE000).contains(&u)) {
        2
    } else {
        0
    };
    if at_end {
        return (skip, buf.len());
    }
    let mut end = skip + (buf.len() - skip) / 2 * 2;
    if end >= skip + 2 && unit_at(buf, end - 2, big).is_some_and(|u| (0xD800..0xDC00).contains(&u))
    {
        end -= 2;
    }
    (skip, end)
}

fn not_text() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "This file is not text")
}

/// The start of a text file, for Quick Look.
#[derive(Debug, serde::Serialize)]
pub struct TextPreview {
    pub text: String,
    /// The file goes on past `text`.
    pub truncated: bool,
    /// The encoding the text was read in (`utf-8`, `utf-16le`, `utf-16be`, `windows-1252`).
    pub encoding: &'static str,
    /// The file starts with a byte-order mark (which is not in `text`).
    pub bom: bool,
}

/// Reads the first part of `path` as text, in `encoding` (found from the file when `None`).
/// Fails for anything that is not a regular file and for files that look binary (a NUL byte in
/// the first 8 KiB, unless the encoding is UTF-16), so the caller can show something else.
pub fn read_text_preview(path: &Path, encoding: Option<Encoding>) -> io::Result<TextPreview> {
    let r = read_piece(path, 0, PREVIEW_LIMIT as u64, encoding, 8192)?;
    Ok(TextPreview {
        truncated: r.end < r.size,
        text: r.text,
        encoding: r.encoding,
        bom: r.bom,
    })
}

/// The most one `read_text_range` returns.
pub const RANGE_LIMIT: u64 = 4 * 1024 * 1024;
/// The least it is asked for, so a page is never smaller than a character.
const RANGE_MIN: u64 = 4096;

/// A piece of a text file, cut at character boundaries.
#[derive(Debug, serde::Serialize, PartialEq, Eq)]
pub struct TextRange {
    pub text: String,
    /// Where the text starts in the file, in bytes (moved on from the asked offset if that was
    /// inside a character, or inside a byte-order mark).
    pub start: u64,
    /// Where the next piece starts: `start` plus the bytes `text` came from.
    pub end: u64,
    /// The file's size in bytes.
    pub size: u64,
    /// The encoding the text was read in; see `TextPreview::encoding`.
    pub encoding: &'static str,
    /// The file starts with a byte-order mark.
    pub bom: bool,
}

/// Reads up to `length` bytes of `path` from byte `offset`, as text in `encoding` (found from
/// the file when `None`; a byte-order mark always wins over a guess, but not over a choice).
/// The start moves on past the rest of a character the offset fell in, and the end moves back
/// before one the length cut, so a piece never holds half a character and the next piece starts
/// where this one ends. Offsets are bytes in every encoding. Fails like `read_text_preview`: for
/// a file in the cloud, one that is not a file, one that holds NUL bytes (binary).
pub fn read_text_range(
    path: &Path,
    offset: u64,
    length: u64,
    encoding: Option<Encoding>,
) -> io::Result<TextRange> {
    read_piece(path, offset, length, encoding, usize::MAX)
}

/// The piece itself. `nul_window` is how many bytes are checked for NULs (binary) when the
/// encoding is not UTF-16.
fn read_piece(
    path: &Path,
    offset: u64,
    length: u64,
    requested: Option<Encoding>,
    nul_window: usize,
) -> io::Result<TextRange> {
    if cloud_only(path) {
        return Err(io::Error::other(CLOUD_ONLY));
    }
    let mut file = fs::File::open(path)?;
    let meta = file.metadata()?;
    if !meta.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "This is not a file",
        ));
    }
    let size = meta.len();
    // The mark and the guess both come from the start of the file, whatever piece is asked for.
    let mut head = Vec::new();
    (&mut file).take(4).read_to_end(&mut head)?;
    let mark = bom_of(&head);
    let (enc, bom_len) = match (requested, mark) {
        // A mark of the encoding that was asked for is skipped; another encoding keeps the bytes.
        (Some(e), Some((m, n))) if e == m => (e, n),
        (Some(e), _) => (e, 0),
        (None, Some((m, n))) => (m, n),
        (None, None) => {
            file.seek(SeekFrom::Start(0))?;
            head.clear();
            (&mut file).take(SNIFF as u64).read_to_end(&mut head)?;
            (sniff(&head).ok_or_else(not_text)?, 0)
        }
    };
    let length = length.clamp(RANGE_MIN, RANGE_LIMIT);
    let mut at = offset.min(size);
    // Text starts after the mark. In UTF-16 a piece starts on a unit.
    if at < bom_len {
        at = bom_len.min(size);
    }
    if enc.wide() && (at - bom_len) % 2 == 1 {
        at = (at + 1).min(size);
    }
    file.seek(SeekFrom::Start(at))?;
    let mut buf = Vec::new();
    file.take(length).read_to_end(&mut buf)?;
    if !enc.wide() && buf.iter().take(nul_window).any(|&b| b == 0) {
        return Err(not_text());
    }
    let at_end = |used: usize| at + (used as u64) >= size;
    let (start, text, used) = match enc {
        Encoding::Windows1252 => (at, decode_1252(&buf), buf.len()),
        Encoding::Utf8 => {
            // An offset inside a character: skip its remaining bytes (at most three).
            let skip = if at > bom_len {
                buf.iter()
                    .take(3)
                    .take_while(|&&b| b & 0b1100_0000 == 0b1000_0000)
                    .count()
            } else {
                0
            };
            let mut body = &buf[skip..];
            // A length that cut a character: leave it for the next piece, unless it is the end.
            if !at_end(buf.len())
                && let Err(e) = std::str::from_utf8(body)
                && e.error_len().is_none()
            {
                body = &body[..e.valid_up_to()];
            }
            (
                at + skip as u64,
                String::from_utf8_lossy(body).into_owned(),
                body.len(),
            )
        }
        Encoding::Utf16Le | Encoding::Utf16Be => {
            let big = enc == Encoding::Utf16Be;
            let (skip, end) = bounds_16(&buf, at > bom_len, at_end(buf.len()), big);
            let body = &buf[skip..end];
            (at + skip as u64, decode_16(body, big), body.len())
        }
    };
    Ok(TextRange {
        text,
        start,
        end: start + used as u64,
        size,
        encoding: enc.label(),
        bom: bom_len > 0,
    })
}

/// The most one `read_bytes_range` returns.
pub const BYTES_LIMIT: u64 = 1024 * 1024;

/// A piece of a file as bytes, for a viewer of binary files.
#[derive(Debug, PartialEq, Eq)]
pub struct BytesRange {
    /// Where the bytes start in the file (the asked offset, or the end of the file if that is
    /// beyond it).
    pub start: u64,
    /// The file's size in bytes.
    pub size: u64,
    pub data: Vec<u8>,
}

impl BytesRange {
    /// What goes to the page: the start and the size as 8 bytes each (little endian), then the
    /// bytes. The end is `start` plus the number of bytes.
    pub fn pack(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(16 + self.data.len());
        out.extend_from_slice(&self.start.to_le_bytes());
        out.extend_from_slice(&self.size.to_le_bytes());
        out.extend_from_slice(&self.data);
        out
    }
}

/// Reads up to `length` bytes of `path` from byte `offset`, whatever they are. Only ranges are
/// ever read (at most `BYTES_LIMIT`), never the whole of a file, and an offset beyond the end
/// gives no bytes. Fails for a file only in the cloud, and for anything that is not a regular
/// file, like `read_text_preview`.
pub fn read_bytes_range(path: &Path, offset: u64, length: u64) -> io::Result<BytesRange> {
    if cloud_only(path) {
        return Err(io::Error::other(CLOUD_ONLY));
    }
    let mut file = fs::File::open(path)?;
    let meta = file.metadata()?;
    if !meta.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "This is not a file",
        ));
    }
    let size = meta.len();
    let start = offset.min(size);
    file.seek(SeekFrom::Start(start))?;
    let mut data = Vec::new();
    file.take(length.clamp(1, BYTES_LIMIT))
        .read_to_end(&mut data)?;
    Ok(BytesRange { start, size, data })
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

    /// All of `path` read in pieces of `piece` bytes, the way a page pages through it.
    fn paged(path: &Path, piece: u64) -> (String, usize) {
        let mut all = String::new();
        let mut at = 0;
        let mut calls = 0;
        loop {
            let r = read_text_range(path, at, piece, None).unwrap();
            calls += 1;
            assert_eq!(r.start, at, "each piece starts where the last one ended");
            all.push_str(&r.text);
            at = r.end;
            if at >= r.size {
                return (all, calls);
            }
            assert!(calls < 10_000, "no progress");
        }
    }

    #[test]
    fn pieces_of_a_file_join_into_the_file_even_across_multibyte_characters() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("big.txt");
        // 2-, 3- and 4-byte characters, so a cut falls inside one at some piece size.
        let text = "a\u{e9}\u{20ac}\u{1f600}z\n".repeat(3000);
        fs::write(&file, &text).unwrap();
        for piece in [4096, 4097, 5000, 7777, 100_000] {
            let (all, calls) = paged(&file, piece);
            assert_eq!(all, text, "piece {piece}");
            assert!(calls >= 1);
        }
    }

    #[test]
    fn bytes_are_read_as_they_are_in_ranges_and_never_past_the_limit() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("b.bin");
        let bytes: Vec<u8> = (0..=255u8).cycle().take(10_000).collect();
        fs::write(&file, &bytes).unwrap();
        // Binary bytes, NUL included, come back untouched, from any offset.
        let r = read_bytes_range(&file, 250, 12).unwrap();
        assert_eq!((r.start, r.size), (250, 10_000));
        assert_eq!(r.data, bytes[250..262]);
        // The last piece stops at the end of the file.
        let r = read_bytes_range(&file, 9_990, 100).unwrap();
        assert_eq!(r.data, bytes[9_990..]);
        // An offset beyond the end gives nothing, and says where the end is.
        let r = read_bytes_range(&file, 99_999, 10).unwrap();
        assert_eq!((r.start, r.data.len()), (10_000, 0));
        // No more than the limit, however much is asked, and at least a byte.
        let big = dir.path().join("big.bin");
        fs::write(&big, vec![7u8; usize::try_from(BYTES_LIMIT).unwrap() + 5]).unwrap();
        assert_eq!(
            read_bytes_range(&big, 0, u64::MAX).unwrap().data.len() as u64,
            BYTES_LIMIT
        );
        assert_eq!(read_bytes_range(&file, 0, 0).unwrap().data.len(), 1);
        // Not a file.
        assert!(read_bytes_range(dir.path(), 0, 10).is_err());
        assert!(read_bytes_range(&dir.path().join("missing"), 0, 10).is_err());
    }

    #[test]
    fn bytes_are_packed_with_their_start_and_the_file_size_in_front() {
        let r = BytesRange {
            start: 0x0102_0304_0506,
            size: 0x0A0B_0C0D_0E0F,
            data: vec![9, 8, 7],
        };
        let packed = r.pack();
        assert_eq!(packed.len(), 19);
        assert_eq!(
            u64::from_le_bytes(packed[0..8].try_into().unwrap()),
            0x0102_0304_0506
        );
        assert_eq!(
            u64::from_le_bytes(packed[8..16].try_into().unwrap()),
            0x0A0B_0C0D_0E0F
        );
        assert_eq!(&packed[16..], &[9, 8, 7]);
    }

    #[test]
    fn a_range_starting_inside_a_character_moves_on_to_the_next_one() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("t.txt");
        fs::write(&file, "x\u{20ac}y").unwrap();
        // Bytes: x, then the three of the euro sign (1..4), then y.
        let r = read_text_range(&file, 2, 4096, None).unwrap();
        assert_eq!((r.text.as_str(), r.start, r.end, r.size), ("y", 4, 5, 5));
    }

    #[test]
    fn a_range_past_the_end_is_empty_and_the_size_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("t.txt");
        fs::write(&file, "hello").unwrap();
        let r = read_text_range(&file, 999, 4096, None).unwrap();
        assert_eq!((r.text.as_str(), r.start, r.end, r.size), ("", 5, 5, 5));
        let r = read_text_range(&file, 0, 4096, None).unwrap();
        assert_eq!((r.text.as_str(), r.end, r.size), ("hello", 5, 5));
    }

    #[test]
    fn a_range_is_limited_and_binary_files_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("t.txt");
        fs::write(
            &file,
            vec![b'a'; usize::try_from(RANGE_LIMIT).unwrap() + 10],
        )
        .unwrap();
        let r = read_text_range(&file, 0, u64::MAX, None).unwrap();
        assert_eq!(r.end, RANGE_LIMIT, "no more than the limit in one piece");
        let bin = dir.path().join("b.bin");
        let mut bytes = vec![b'a'; 100];
        bytes.push(0);
        fs::write(&bin, bytes).unwrap();
        assert!(read_text_range(&bin, 0, 4096, None).is_err());
        assert!(
            read_text_range(dir.path(), 0, 4096, None).is_err(),
            "not a file"
        );
    }

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
        assert!(
            read_text_preview(&file, None).is_ok(),
            "a local file still reads"
        );
        mark_offline(&file);
        let err = read_text_preview(&file, None).unwrap_err();
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
        let p = read_text_preview(&small, None).unwrap();
        assert_eq!((p.text.as_str(), p.truncated), ("hello\nworld", false));

        let big = dir.path().join("big.log");
        fs::write(&big, "x".repeat(PREVIEW_LIMIT + 10)).unwrap();
        let p = read_text_preview(&big, None).unwrap();
        assert_eq!((p.text.len(), p.truncated), (PREVIEW_LIMIT, true));
    }

    #[test]
    fn text_preview_refuses_binary_files() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("a.bin");
        fs::write(&bin, [0x89, b'P', 0, 0, 1, 2]).unwrap();
        let err = read_text_preview(&bin, None).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn text_preview_accepts_a_nul_after_the_first_8_kib_and_trims_a_split_character() {
        let dir = tempfile::tempdir().unwrap();
        let late_nul = dir.path().join("late.txt");
        let mut bytes = vec![b'a'; 9000];
        bytes.push(0);
        fs::write(&late_nul, &bytes).unwrap();
        assert!(read_text_preview(&late_nul, None).is_ok());

        // A two-byte character straddling the limit is dropped, not shown broken.
        let split = dir.path().join("split.txt");
        let mut text = "a".repeat(PREVIEW_LIMIT - 1).into_bytes();
        text.extend("é".as_bytes());
        text.extend(b"tail");
        fs::write(&split, &text).unwrap();
        let p = read_text_preview(&split, None).unwrap();
        assert!(p.truncated);
        assert!(!p.text.contains('\u{FFFD}'), "no broken character");
    }

    #[test]
    fn text_preview_refuses_a_folder() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read_text_preview(dir.path(), None).is_err());
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

    // ---- encodings ----

    fn utf16(text: &str, big: bool, bom: bool) -> Vec<u8> {
        let mut out = Vec::new();
        if bom {
            out.extend_from_slice(if big { &[0xFE, 0xFF] } else { &[0xFF, 0xFE] });
        }
        for u in text.encode_utf16() {
            out.extend_from_slice(&if big {
                u.to_be_bytes()
            } else {
                u.to_le_bytes()
            });
        }
        out
    }

    /// The whole file read in pieces of `piece` bytes in `enc`, each starting where the last ended.
    fn paged_in(path: &Path, piece: u64, enc: Option<Encoding>) -> (String, TextRange) {
        let mut all = String::new();
        let mut at = 0;
        let mut first = None;
        for _ in 0..10_000 {
            let r = read_text_range(path, at, piece, enc).unwrap();
            if first.is_none() {
                // The first piece may start after a byte-order mark; never before `at`.
                assert!(r.start >= at);
            } else {
                assert_eq!(r.start, at, "each piece starts where the last one ended");
            }
            all.push_str(&r.text);
            at = r.end;
            let done = at >= r.size;
            first.get_or_insert(r);
            if done {
                return (all, first.unwrap());
            }
        }
        panic!("no progress");
    }

    const MIXED: &str = "a\u{e9}\u{20ac}\u{1f600}z\n";

    #[test]
    fn utf16_with_a_mark_is_found_and_pieces_never_cut_a_character() {
        let dir = tempfile::tempdir().unwrap();
        let text = MIXED.repeat(3000);
        for big in [false, true] {
            let file = dir.path().join(format!("u16-{big}.txt"));
            fs::write(&file, utf16(&text, big, true)).unwrap();
            let label = if big { "utf-16be" } else { "utf-16le" };
            // Odd sizes put cuts inside units and between the halves of a pair.
            for piece in [4096, 4097, 4099, 5001, 100_000] {
                let (all, first) = paged_in(&file, piece, None);
                assert_eq!(all, text, "{label}, piece {piece}");
                assert_eq!((first.encoding, first.bom, first.start), (label, true, 2));
            }
            let p = read_text_preview(&file, None).unwrap();
            assert_eq!((p.encoding, p.bom), (label, true));
            assert!(p.text.starts_with('a'), "the mark is not in the text");
        }
    }

    #[test]
    fn a_range_that_starts_inside_a_utf16_unit_or_pair_moves_on() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("u.txt");
        let text = "\u{1f600}".repeat(4000);
        fs::write(&file, utf16(&text, false, true)).unwrap();
        // Byte 3 is inside the first unit; byte 4 is the second half of the first pair.
        let r = read_text_range(&file, 3, 4096, None).unwrap();
        assert_eq!(r.start, 6, "next whole pair");
        assert!(r.text.chars().all(|c| c == '\u{1f600}'));
        let r = read_text_range(&file, 4, 4096, None).unwrap();
        assert_eq!(r.start, 6);
        assert!(r.text.chars().all(|c| c == '\u{1f600}'));
        // Inside the mark.
        let r = read_text_range(&file, 1, 4096, None).unwrap();
        assert_eq!(r.start, 2);
    }

    #[test]
    fn utf16_without_a_mark_is_guessed_for_latin_text_and_never_called_binary() {
        let dir = tempfile::tempdir().unwrap();
        let text = "Hello, world\r\n".repeat(500);
        for big in [false, true] {
            let file = dir.path().join(format!("nm-{big}.txt"));
            fs::write(&file, utf16(&text, big, false)).unwrap();
            let (all, first) = paged_in(&file, 4096, None);
            assert_eq!(all, text);
            assert_eq!(first.encoding, if big { "utf-16be" } else { "utf-16le" });
            assert!(!first.bom);
        }
    }

    #[test]
    fn a_choice_wins_over_the_guess_but_a_mark_of_that_encoding_is_still_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("c.txt");
        fs::write(&file, utf16("hi \u{20ac}", false, true)).unwrap();
        let r = read_text_range(&file, 0, 4096, Some(Encoding::Utf16Le)).unwrap();
        assert_eq!((r.text.as_str(), r.bom, r.start), ("hi \u{20ac}", true, 2));
        // Read as Windows-1252, the same bytes are what they are, mark and NULs included.
        let r = read_text_preview(&file, Some(Encoding::Windows1252)).unwrap_err();
        assert_eq!(
            r.kind(),
            io::ErrorKind::InvalidData,
            "NUL bytes are not Windows-1252 text"
        );
    }

    #[test]
    fn utf8_with_a_mark_hides_the_mark_and_reports_it() {
        let dir = tempfile::tempdir().unwrap();
        let text = MIXED.repeat(3000);
        let file = dir.path().join("bom8.txt");
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(text.as_bytes());
        fs::write(&file, &bytes).unwrap();
        for piece in [4096, 4097, 5000, 100_000] {
            let (all, first) = paged_in(&file, piece, None);
            assert_eq!(all, text, "piece {piece}");
            assert_eq!((first.encoding, first.bom, first.start), ("utf-8", true, 3));
        }
        let r = read_text_range(&file, 1, 4096, None).unwrap();
        assert_eq!(r.start, 3, "an offset inside the mark moves past it");
    }

    #[test]
    fn a_file_that_is_not_utf8_and_has_no_mark_reads_as_windows_1252() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("w.txt");
        // caf\xe9, a euro sign (0x80), curly quotes (0x93 0x94) and a byte 1252 leaves open (0x81).
        let bytes: Vec<u8> = b"caf\xe9 \x80 \x93q\x94 \x81\n".repeat(1000);
        fs::write(&file, &bytes).unwrap();
        let want = "caf\u{e9} \u{20ac} \u{201c}q\u{201d} \u{81}\n".repeat(1000);
        for piece in [4096, 4099, 100_000] {
            let (all, first) = paged_in(&file, piece, None);
            assert_eq!(all, want, "piece {piece}");
            assert_eq!((first.encoding, first.bom), ("windows-1252", false));
        }
        let p = read_text_preview(&file, None).unwrap();
        assert!(!p.text.contains('\u{fffd}'));
        // The same bytes asked for as UTF-8 give replacement characters, as before.
        let r = read_text_range(&file, 0, 4096, Some(Encoding::Utf8)).unwrap();
        assert_eq!(r.encoding, "utf-8");
        assert!(r.text.contains('\u{fffd}'));
    }

    #[test]
    fn a_short_latin_file_ending_in_a_byte_that_could_start_a_utf8_character_is_windows_1252() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("short.txt");
        // 0xE9 leads a three-byte sequence, and nothing follows it.
        fs::write(&file, b"caf\xe9").unwrap();
        let r = read_text_range(&file, 0, 4096, None).unwrap();
        assert_eq!((r.text.as_str(), r.encoding), ("caf\u{e9}", "windows-1252"));
        // A long UTF-8 file whose sniff window ends inside a character stays UTF-8.
        let text = "\u{20ac}".repeat(40_000);
        let long = dir.path().join("long.txt");
        fs::write(&long, &text).unwrap();
        assert_eq!(
            read_text_range(&long, 0, 4096, None).unwrap().encoding,
            "utf-8"
        );
    }

    #[test]
    fn a_big_endian_range_starting_inside_a_unit_or_pair_moves_on() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("be.txt");
        fs::write(&file, utf16(&"\u{1f600}".repeat(4000), true, true)).unwrap();
        for (offset, start) in [(1, 2), (3, 6), (4, 6), (6, 6)] {
            let r = read_text_range(&file, offset, 4096, None).unwrap();
            assert_eq!(
                (r.start, r.encoding),
                (start, "utf-16be"),
                "offset {offset}"
            );
            assert!(r.text.chars().all(|c| c == '\u{1f600}'));
        }
    }

    #[test]
    fn valid_utf8_stays_utf8_and_names_it() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("p.txt");
        fs::write(&file, "caf\u{e9}").unwrap();
        let r = read_text_range(&file, 0, 4096, None).unwrap();
        assert_eq!(
            (r.text.as_str(), r.encoding, r.bom),
            ("caf\u{e9}", "utf-8", false)
        );
        let p = read_text_preview(&file, None).unwrap();
        assert_eq!((p.encoding, p.truncated), ("utf-8", false));
    }

    #[test]
    fn a_utf16_file_with_an_odd_last_byte_ends_with_a_replacement_character() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("odd.txt");
        let mut bytes = utf16("ab", false, true);
        bytes.push(0x41);
        fs::write(&file, &bytes).unwrap();
        let r = read_text_range(&file, 0, 4096, None).unwrap();
        assert_eq!(r.text, "ab\u{fffd}");
        assert_eq!(r.end, r.size);
    }

    #[test]
    fn a_utf16_preview_says_when_it_stopped_and_drops_half_a_pair() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("long.txt");
        let text = "\u{1f600}".repeat(100_000);
        fs::write(&file, utf16(&text, true, true)).unwrap();
        let p = read_text_preview(&file, None).unwrap();
        assert!(p.truncated);
        assert!(p.text.chars().all(|c| c == '\u{1f600}'), "no half pairs");
    }

    #[test]
    fn encodings_are_named_and_an_unknown_name_is_refused() {
        for e in [
            Encoding::Utf8,
            Encoding::Utf16Le,
            Encoding::Utf16Be,
            Encoding::Windows1252,
        ] {
            assert_eq!(Encoding::from_label(e.label()), Some(e));
        }
        assert_eq!(Encoding::from_label("latin1"), None);
        assert_eq!(Encoding::from_label(""), None);
    }

    #[test]
    fn binary_files_are_still_refused_when_nothing_says_utf16() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("b.bin");
        let bytes: Vec<u8> = (0..=255u8).cycle().take(10_000).collect();
        fs::write(&file, &bytes).unwrap();
        assert!(read_text_range(&file, 0, 4096, None).is_err());
        assert!(read_text_preview(&file, None).is_err());
    }
}
