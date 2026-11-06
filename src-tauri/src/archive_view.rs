//! What is inside a zip or tar archive, for Quick Look: names, sizes and dates, read from the
//! archive's own index without unpacking anything. Nothing is extracted or written here.
//!
//! A zip's index keeps one entry per name, so a hostile zip that repeats a name shows it once (the
//! last one); a tar lists every member, repeats included.
//!
//! Names are the archive's own text and may be anything, so they come out with control, direction
//! and zero-width characters written as `‹U+XXXX›` rather than acted on or hidden.

use std::cell::Cell;
use std::fmt::Write as _;
use std::fs::File;
use std::io::{self, BufReader, Read, Seek, SeekFrom};
use std::path::Path;
use std::time::{Duration, Instant};

use flate2::read::GzDecoder;
use serde::Serialize;
use zip::ZipArchive;

use crate::civil::civil_from_days;
use crate::platform::clock;

/// Entries listed at most; an archive with more says how many there are (when it can know).
pub const MAX_ENTRIES: usize = 20_000;
/// The most of a compressed tar that is decompressed while looking: past this the list stops.
const MAX_SCAN: u64 = 512 << 20;
/// The longest name kept, in characters.
const NAME_LIMIT: usize = 1024;
/// How long a tar is read for: a little under the time Quick Look waits for the answer.
const SCAN_TIME: Duration = Duration::from_secs(10);

/// What was found for one entry.
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    /// The name with `/` between parts, safe to show as text.
    pub path: String,
    pub is_dir: bool,
    /// The size once unpacked, in bytes.
    pub size: u64,
    /// `YYYY-MM-DD HH:MM` in local time, if the archive says.
    pub modified: Option<String>,
    pub encrypted: bool,
}

/// How far reading the archive got.
#[derive(Debug, Serialize, PartialEq, Eq, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    Ok,
    /// Some entries are password protected: their names are listed, their contents can't be read.
    Encrypted,
    /// The archive is cut off or broken; what could be read before that is listed.
    Damaged,
    /// Not a kind of archive Folio lists.
    Unsupported,
}

/// An archive's list of entries.
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Listing {
    /// `zip`, `tar` or `tar.gz`; empty if not recognised.
    pub format: &'static str,
    pub status: Status,
    /// What to tell the person when the status is not `Ok`.
    pub message: Option<String>,
    pub entries: Vec<Entry>,
    /// How many entries the archive has, if known; more than `entries` when it was cut short.
    pub total: Option<u64>,
    /// There are entries that are not listed.
    pub truncated: bool,
}

impl Listing {
    fn failed(format: &'static str, status: Status, message: &str) -> Self {
        Self {
            format,
            status,
            message: Some(message.to_owned()),
            entries: Vec::new(),
            total: None,
            truncated: false,
        }
    }
}

/// A character that would hide in a name: control, direction marks and overrides, zero-width and
/// joiner characters, soft hyphen, the byte-order mark.
fn hides(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{00AD}'
                | '\u{061C}'
                | '\u{180E}'
                | '\u{200B}'..='\u{200F}'
                | '\u{202A}'..='\u{202E}'
                | '\u{2060}'..='\u{2064}'
                | '\u{2066}'..='\u{2069}'
                | '\u{FEFF}'
        )
}

/// A name as text for the page: backslashes as `/`, hidden characters written out, not too long.
pub fn display_name(raw: &str) -> String {
    let mut out = String::new();
    for c in raw.chars().take(NAME_LIMIT) {
        match c {
            '\\' => out.push('/'),
            c if hides(c) => {
                // Writing to a String can't fail.
                let _ = write!(out, "\u{2039}U+{:04X}\u{203A}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out
}

/// A time as the person's wall clock shows it: `YYYY-MM-DD HH:MM`.
fn clock_text(year: i64, month: u8, day: u8, hour: i64, minute: i64) -> String {
    format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}")
}

/// A tar's seconds since 1970 (UTC) as local wall-clock text; none for an absurd value.
fn unix_text(utc: u64) -> Option<String> {
    let utc = i64::try_from(utc).ok().filter(|s| *s < 253_402_300_800)?;
    let local = utc + clock::offset_at_utc(utc);
    let (year, month, day) = civil_from_days(local.div_euclid(86_400));
    let rest = local.rem_euclid(86_400);
    Some(clock_text(year, month, day, rest / 3600, rest % 3600 / 60))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Zip,
    Tar,
    TarGz,
}

impl Kind {
    const fn label(self) -> &'static str {
        match self {
            Self::Zip => "zip",
            Self::Tar => "tar",
            Self::TarGz => "tar.gz",
        }
    }
}

/// What kind of archive `head` (the first bytes) is, by its signature; the name only decides when
/// there is none, so a damaged archive is called damaged and not "not an archive".
fn kind_of(head: &[u8], name: &str) -> Option<Kind> {
    if head.starts_with(b"PK\x03\x04") || head.starts_with(b"PK\x05\x06") {
        return Some(Kind::Zip);
    }
    if head.starts_with(&[0x1F, 0x8B]) {
        return Some(Kind::TarGz);
    }
    if head.len() >= 262 && &head[257..262] == b"ustar" {
        return Some(Kind::Tar);
    }
    let ends = |tail: &str| {
        let (name, tail) = (name.as_bytes(), tail.as_bytes());
        name.len() >= tail.len() && name[name.len() - tail.len()..].eq_ignore_ascii_case(tail)
    };
    if ends(".zip") {
        Some(Kind::Zip)
    } else if ends(".tar.gz") || ends(".tgz") {
        Some(Kind::TarGz)
    } else if ends(".tar") {
        Some(Kind::Tar)
    } else {
        None
    }
}

/// The entries of the archive at `path`. Fails only when the file can't be opened or is not a
/// file; an archive that is encrypted, damaged or of another kind is a listing that says so.
pub fn list(path: &Path) -> io::Result<Listing> {
    let mut file = File::open(path)?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "This is not a file",
        ));
    }
    let mut head = Vec::new();
    (&mut file).take(512).read_to_end(&mut head)?;
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
    let Some(kind) = kind_of(&head, name.as_deref().unwrap_or("")) else {
        return Ok(Listing::failed(
            "",
            Status::Unsupported,
            "Folio lists the contents of zip and tar archives only",
        ));
    };
    let file = File::open(path)?;
    Ok(match kind {
        Kind::Zip => list_zip(file),
        Kind::Tar => list_tar_plain(file, SCAN_TIME),
        Kind::TarGz => list_tar(
            BufReader::new(GzDecoder::new(BufReader::new(file))),
            kind,
            MAX_SCAN,
            SCAN_TIME,
        ),
    })
}

/// The name of an entry as text; bytes that are not valid text come out as replacement characters.
fn name_text(name: zip::result::ZipResult<std::borrow::Cow<'_, str>>, raw: &[u8]) -> String {
    name.map_or_else(
        |_| String::from_utf8_lossy(raw).into_owned(),
        std::borrow::Cow::into_owned,
    )
}

/// More entries than this in a zip, or an index larger than this, and it is not listed: reading
/// the whole index would cost memory and time out of proportion to a look inside.
const MAX_ZIP_INDEX: u32 = 32 << 20;

/// Whether the index of the zip is too big to read, judged from its end-of-central-directory
/// record alone (the last bytes of the file): a zip64 marker, which comes with more than 65,535
/// entries or an index over 4 GB, or an index over `MAX_ZIP_INDEX`. A file without the record is
/// left to the reader, which says it is damaged.
fn zip_index_too_big(file: &mut File) -> io::Result<bool> {
    let len = file.metadata()?.len();
    let tail_len = len.min(66 * 1024);
    file.seek(SeekFrom::Start(len - tail_len))?;
    let mut tail = Vec::new();
    file.take(tail_len).read_to_end(&mut tail)?;
    let Some(at) = tail.windows(4).rposition(|w| w == b"PK\x05\x06") else {
        return Ok(false);
    };
    let record = &tail[at..];
    if record.len() < 22 {
        return Ok(false);
    }
    let entries = u16::from_le_bytes([record[10], record[11]]);
    let index = u32::from_le_bytes([record[12], record[13], record[14], record[15]]);
    Ok(entries == u16::MAX || index == u32::MAX || index > MAX_ZIP_INDEX)
}

fn list_zip(mut file: File) -> Listing {
    let label = Kind::Zip.label();
    if zip_index_too_big(&mut file).unwrap_or(false) {
        return Listing {
            format: label,
            status: Status::Ok,
            message: Some(
                "This zip has too many entries to list here (more than 65,535, or a very large index)"
                    .to_owned(),
            ),
            entries: Vec::new(),
            total: None,
            truncated: true,
        };
    }
    let Ok(zip) = ZipArchive::new(BufReader::new(file)) else {
        return Listing::failed(
            label,
            Status::Damaged,
            "This zip is damaged or cut off, so its contents can't be listed",
        );
    };
    let total = zip.len();
    let mut entries = Vec::new();
    let mut encrypted = false;
    let mut broken = false;
    for i in 0..total.min(MAX_ENTRIES) {
        // From the index already read: nothing more is read from the file, and nothing is
        // decompressed or decrypted.
        let Ok(entry) = zip.by_index_data(i) else {
            broken = true;
            break;
        };
        encrypted |= entry.encrypted();
        let modified = entry.last_modified().map(|d| {
            clock_text(
                i64::from(d.year()),
                d.month(),
                d.day(),
                i64::from(d.hour()),
                i64::from(d.minute()),
            )
        });
        entries.push(Entry {
            path: display_name(&name_text(entry.name(), entry.name_raw())),
            is_dir: entry.is_dir(),
            size: entry.size(),
            modified,
            encrypted: entry.encrypted(),
        });
    }
    let (status, message) = if broken {
        (
            Status::Damaged,
            Some("This zip is damaged: the list stops where it can't be read".to_owned()),
        )
    } else if encrypted {
        (
            Status::Encrypted,
            Some("Some files are password protected: their names are listed, but Folio can't open them".to_owned()),
        )
    } else {
        (Status::Ok, None)
    };
    Listing {
        format: label,
        status,
        message,
        total: Some(total as u64),
        truncated: total > entries.len() && !broken,
        entries,
    }
}

/// Counts the bytes read through it, and ends the stream when `left` runs out or the time is up
/// (a slow or hostile share must not keep the thread busy after the page has given up).
struct Capped<'a, R> {
    inner: R,
    left: &'a Cell<u64>,
    until: Instant,
}

impl<R: Read> Read for Capped<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let left = self.left.get();
        if left == 0 {
            return Ok(0);
        }
        if Instant::now() >= self.until {
            self.left.set(0);
            return Ok(0);
        }
        let room = usize::try_from(left).map_or(buf.len(), |l| l.min(buf.len()));
        let n = self.inner.read(&mut buf[..room])?;
        self.left.set(left - n as u64);
        Ok(n)
    }
}

/// What reading the entries of a tar came to.
struct Scan {
    entries: Vec<Entry>,
    /// An entry could not be read.
    broken: bool,
    /// The list stops before the archive does (too many entries, or out of time).
    more: bool,
}

/// The entries `iter` gives, up to `MAX_ENTRIES` and until `until`.
fn scan_tar<R: Read>(iter: tar::Entries<'_, R>, until: Instant) -> Scan {
    let mut scan = Scan {
        entries: Vec::new(),
        broken: false,
        more: false,
    };
    for item in iter {
        let Ok(entry) = item else {
            scan.broken = true;
            break;
        };
        if scan.entries.len() >= MAX_ENTRIES || Instant::now() >= until {
            scan.more = true;
            break;
        }
        let kind = entry.header().entry_type();
        // Extended-header and long-name records are not files.
        if kind.is_pax_global_extensions()
            || kind.is_pax_local_extensions()
            || kind.is_gnu_longname()
            || kind.is_gnu_longlink()
        {
            continue;
        }
        let raw = entry.path_bytes();
        let name = String::from_utf8_lossy(&raw);
        scan.entries.push(Entry {
            path: display_name(name.trim_end_matches('/')),
            is_dir: kind.is_dir(),
            size: entry.size(),
            modified: entry.header().mtime().ok().and_then(unix_text),
            encrypted: false,
        });
    }
    scan
}

fn tar_listing(label: &'static str, scan: Scan) -> Listing {
    let Scan {
        entries,
        broken,
        more,
    } = scan;
    Listing {
        format: label,
        status: if broken { Status::Damaged } else { Status::Ok },
        message: broken
            .then(|| "This archive is damaged: the list stops where it can't be read".to_owned()),
        total: (!more && !broken).then_some(entries.len() as u64),
        truncated: more,
        entries,
    }
}

fn not_a_tar(label: &'static str) -> Listing {
    Listing::failed(
        label,
        Status::Damaged,
        "This archive is damaged or not a tar file, so its contents can't be listed",
    )
}

/// A plain tar: the reader can seek, so the bodies of the files are stepped over, not read.
fn list_tar_plain(file: File, time: Duration) -> Listing {
    let label = Kind::Tar.label();
    let mut archive = tar::Archive::new(BufReader::new(file));
    let Ok(iter) = archive.entries_with_seek() else {
        return not_a_tar(label);
    };
    tar_listing(label, scan_tar(iter, Instant::now() + time))
}

/// A compressed tar has to be decompressed to be read through: no more than `budget` bytes of it,
/// and no longer than `time`.
fn list_tar<R: Read>(reader: R, kind: Kind, budget: u64, time: Duration) -> Listing {
    let label = kind.label();
    let left = Cell::new(budget);
    let until = Instant::now() + time;
    let mut archive = tar::Archive::new(Capped {
        inner: reader,
        left: &left,
        until,
    });
    let Ok(iter) = archive.entries() else {
        return not_a_tar(label);
    };
    let mut scan = scan_tar(iter, until);
    // A scan that ran into the cap or the time limit is a long archive, not a cut-off one, and
    // not a complete list whether or not it stopped on an entry boundary.
    if left.get() == 0 {
        scan.broken = false;
        scan.more = true;
    }
    tar_listing(label, scan)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    use zip::{CompressionMethod, ZipWriter};

    fn zip_with(path: &Path, files: &[(&str, &[u8])]) {
        let mut zip = ZipWriter::new(File::create(path).unwrap());
        let options = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Stored)
            .last_modified_time(zip::DateTime::from_date_and_time(2024, 3, 5, 14, 7, 0).unwrap());
        for (name, bytes) in files {
            if name.ends_with('/') {
                zip.add_directory(*name, options).unwrap();
            } else {
                zip.start_file(*name, options).unwrap();
                zip.write_all(bytes).unwrap();
            }
        }
        zip.finish().unwrap();
    }

    fn tar_bytes(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut builder = tar::Builder::new(Vec::new());
        for (name, bytes) in files {
            let mut header = tar::Header::new_gnu();
            header.set_size(bytes.len() as u64);
            header.set_mtime(1_700_000_000);
            header.set_mode(0o644);
            header.set_cksum();
            builder.append_data(&mut header, name, *bytes).unwrap();
        }
        builder.into_inner().unwrap()
    }

    fn paths(l: &Listing) -> Vec<&str> {
        l.entries.iter().map(|e| e.path.as_str()).collect()
    }

    #[test]
    fn a_zip_is_listed_with_names_sizes_dates_and_folders() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.zip");
        zip_with(
            &p,
            &[
                ("docs/", b""),
                ("docs/readme.txt", b"hello"),
                ("top.bin", &[0u8; 300]),
            ],
        );
        let l = list(&p).unwrap();
        assert_eq!(
            (l.format, l.status, l.truncated),
            ("zip", Status::Ok, false)
        );
        assert_eq!(l.total, Some(3));
        assert_eq!(paths(&l), ["docs/", "docs/readme.txt", "top.bin"]);
        assert!(l.entries[0].is_dir && !l.entries[1].is_dir);
        assert_eq!((l.entries[1].size, l.entries[2].size), (5, 300));
        let when = l.entries[1].modified.as_deref().unwrap();
        assert_eq!(when, "2024-03-05 14:07");
    }

    #[test]
    fn a_tar_and_a_tar_gz_are_listed() {
        let dir = tempfile::tempdir().unwrap();
        let bytes = tar_bytes(&[("a/b.txt", b"hello"), ("c.txt", b"x")]);
        let plain = dir.path().join("a.tar");
        std::fs::write(&plain, &bytes).unwrap();
        let l = list(&plain).unwrap();
        assert_eq!((l.format, l.status), ("tar", Status::Ok));
        assert_eq!(paths(&l), ["a/b.txt", "c.txt"]);
        assert_eq!(l.entries[0].size, 5);
        assert_eq!(l.total, Some(2));
        let when = l.entries[0].modified.as_deref().unwrap();
        assert!(when.starts_with("2023-11-1"), "{when}");

        let gz = dir.path().join("a.tar.gz");
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        enc.write_all(&bytes).unwrap();
        std::fs::write(&gz, enc.finish().unwrap()).unwrap();
        let l = list(&gz).unwrap();
        assert_eq!((l.format, l.status), ("tar.gz", Status::Ok));
        assert_eq!(paths(&l), ["a/b.txt", "c.txt"]);
    }

    #[test]
    fn names_cannot_hide_characters_or_pose_as_other_paths() {
        assert_eq!(display_name("a\\b\\c.txt"), "a/b/c.txt");
        assert_eq!(
            display_name("photo\u{202E}gpj.exe"),
            "photo\u{2039}U+202E\u{203A}gpj.exe"
        );
        assert_eq!(
            display_name("a\u{200B}b\u{7}c\u{FEFF}"),
            "a\u{2039}U+200B\u{203A}b\u{2039}U+0007\u{203A}c\u{2039}U+FEFF\u{203A}"
        );
        assert_eq!(
            display_name("line\nbreak"),
            "line\u{2039}U+000A\u{203A}break"
        );
        assert_eq!(display_name(&"x".repeat(5000)).chars().count(), NAME_LIMIT);
        // Through a real archive: the name comes out escaped.
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("evil.zip");
        zip_with(
            &p,
            &[("../../etc/passwd", b"x"), ("doc\u{202E}fdp.exe", b"y")],
        );
        let l = list(&p).unwrap();
        assert_eq!(l.entries[1].path, "doc\u{2039}U+202E\u{203A}fdp.exe");
        // A climbing name is shown as the text it is, and nothing is created.
        assert_eq!(l.entries[0].path, "../../etc/passwd");
        // (Not `exists()` on the climbing path: on Linux it resolves to the real /etc/passwd.)
        let left: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .map(|e| e.file_name())
            .collect();
        assert_eq!(left, [std::ffi::OsString::from("evil.zip")]);
    }

    #[test]
    fn nothing_is_extracted_or_written() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.zip");
        zip_with(&p, &[("x/y.txt", b"hello")]);
        let before: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .map(|e| e.file_name())
            .collect();
        let _ = list(&p).unwrap();
        let after: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .map(|e| e.file_name())
            .collect();
        assert_eq!(before, after);
    }

    #[test]
    fn an_encrypted_zip_says_so_and_still_lists_the_names() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("secret.zip");
        zip_with(&p, &[("a.txt", b"hello"), ("b.txt", b"world")]);
        // Set the "encrypted" bit in every central directory header.
        let mut bytes = std::fs::read(&p).unwrap();
        let mut at = 0;
        while let Some(i) = bytes[at..].windows(4).position(|w| w == b"PK\x01\x02") {
            let h = at + i;
            bytes[h + 8] |= 1;
            at = h + 4;
        }
        std::fs::write(&p, &bytes).unwrap();
        let l = list(&p).unwrap();
        assert_eq!(l.status, Status::Encrypted);
        assert!(l.message.as_deref().unwrap().contains("password"));
        assert_eq!(paths(&l), ["a.txt", "b.txt"]);
        assert!(l.entries.iter().all(|e| e.encrypted));
    }

    #[test]
    fn a_damaged_archive_says_so_instead_of_failing() {
        let dir = tempfile::tempdir().unwrap();
        // A zip cut off in the middle: no central directory.
        let whole = dir.path().join("w.zip");
        zip_with(&whole, &[("a.txt", &[1u8; 5000]), ("b.txt", &[2u8; 5000])]);
        let bytes = std::fs::read(&whole).unwrap();
        let cut = dir.path().join("cut.zip");
        std::fs::write(&cut, &bytes[..bytes.len() / 2]).unwrap();
        let l = list(&cut).unwrap();
        assert_eq!(l.status, Status::Damaged);
        assert!(l.message.is_some());
        // Words with a zip's name.
        let fake = dir.path().join("fake.zip");
        std::fs::write(&fake, b"not an archive at all").unwrap();
        assert_eq!(list(&fake).unwrap().status, Status::Damaged);
        // A tar cut inside an entry still lists what came before.
        let tar = tar_bytes(&[("a.txt", &[1u8; 3000]), ("b.txt", &[2u8; 3000])]);
        let cut_tar = dir.path().join("cut.tar");
        std::fs::write(&cut_tar, &tar[..tar.len() - 4000]).unwrap();
        let l = list(&cut_tar).unwrap();
        assert!(
            matches!(l.status, Status::Damaged | Status::Ok),
            "{:?}",
            l.status
        );
        assert_eq!(l.entries.first().map(|e| e.path.as_str()), Some("a.txt"));
        // Garbage that claims to be a gzip.
        let junk = dir.path().join("junk.tgz");
        let mut garbage = vec![0x1F, 0x8B];
        garbage.extend((0..300u16).map(|n| (n * 7 % 251) as u8));
        std::fs::write(&junk, garbage).unwrap();
        assert_ne!(list(&junk).unwrap().status, Status::Ok);
    }

    #[test]
    fn another_kind_of_file_is_not_listed() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.7z");
        std::fs::write(&p, b"7z\xBC\xAF\x27\x1C and the rest").unwrap();
        let l = list(&p).unwrap();
        assert_eq!(l.status, Status::Unsupported);
        assert_eq!(l.entries, []);
        assert!(list(dir.path()).is_err(), "a folder is not an archive");
        assert!(list(&dir.path().join("missing.zip")).is_err());
    }

    #[test]
    fn a_long_archive_lists_the_first_entries_and_says_how_many_there_are() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("many.zip");
        let names: Vec<String> = (0..MAX_ENTRIES + 5).map(|i| format!("f{i}.txt")).collect();
        let files: Vec<(&str, &[u8])> = names.iter().map(|n| (n.as_str(), &b""[..])).collect();
        zip_with(&p, &files);
        let l = list(&p).unwrap();
        assert_eq!(l.entries.len(), MAX_ENTRIES);
        assert!(l.truncated);
        assert_eq!(l.total, Some(MAX_ENTRIES as u64 + 5));
    }

    #[test]
    fn a_scan_stopped_by_its_budget_or_time_says_the_list_is_not_complete() {
        let files: Vec<(String, Vec<u8>)> = (0..6u8)
            .map(|i| (format!("f{i}.bin"), vec![i; 2000]))
            .collect();
        let refs: Vec<(&str, &[u8])> = files
            .iter()
            .map(|(n, b)| (n.as_str(), b.as_slice()))
            .collect();
        let bytes = tar_bytes(&refs);
        // A budget that ends part way through: some entries, then "more", never "complete".
        let l = list_tar(&bytes[..], Kind::Tar, 3000, Duration::from_secs(60));
        assert_eq!(l.status, Status::Ok);
        assert!(l.truncated && l.total.is_none(), "{l:?}");
        assert!(!l.entries.is_empty() && l.entries.len() < 6);
        // Time already up: the same.
        let l = list_tar(&bytes[..], Kind::Tar, u64::MAX, Duration::ZERO);
        assert!(l.truncated && l.total.is_none(), "{l:?}");
        // Plenty of both: the whole list, complete.
        let l = list_tar(&bytes[..], Kind::Tar, u64::MAX, Duration::from_secs(60));
        assert!(!l.truncated);
        assert_eq!((l.entries.len(), l.total), (6, Some(6)));
    }

    #[test]
    fn a_tar_entry_larger_than_its_header_field_holds_still_gets_its_size() {
        // A plain header holds a size as octal text; the entry knows the size it really has.
        let bytes = tar_bytes(&[("a.bin", &[7u8; 700])]);
        let l = list_tar(&bytes[..], Kind::Tar, u64::MAX, Duration::from_secs(60));
        assert_eq!(l.entries[0].size, 700);
    }

    #[test]
    fn a_zip_with_more_entries_than_a_look_inside_is_worth_says_so_without_reading_its_index() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("huge.zip");
        let names: Vec<String> = (0..66_000).map(|i| format!("{i}")).collect();
        let files: Vec<(&str, &[u8])> = names.iter().map(|n| (n.as_str(), &b""[..])).collect();
        zip_with(&p, &files);
        let l = list(&p).unwrap();
        assert_eq!(l.status, Status::Ok);
        assert!(
            l.truncated && l.entries.is_empty() && l.total.is_none(),
            "{l:?}"
        );
        assert!(l.message.as_deref().unwrap().contains("too many"));
        // A zip just under the limit is still listed.
        let small = dir.path().join("small.zip");
        zip_with(&small, &[("a.txt", b"x")]);
        assert_eq!(list(&small).unwrap().entries.len(), 1);
    }

    #[test]
    fn a_plain_tar_is_listed_by_stepping_over_the_bodies_of_its_files() {
        let dir = tempfile::tempdir().unwrap();
        let big = vec![9u8; 2 << 20];
        let bytes = tar_bytes(&[("a.bin", &big), ("b.txt", b"hi")]);
        let p = dir.path().join("a.tar");
        std::fs::write(&p, &bytes).unwrap();
        let l = list(&p).unwrap();
        assert_eq!((l.status, l.truncated), (Status::Ok, false));
        assert_eq!(paths(&l), ["a.bin", "b.txt"]);
        assert_eq!(l.entries[0].size, 2 << 20);
    }
}
