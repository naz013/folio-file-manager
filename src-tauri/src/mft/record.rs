//! One NTFS file record (an entry of the master file table), read from its raw bytes.
//!
//! Only what a size scan needs is taken: whether the entry is in use and a folder, its name and
//! parent, its size and modification time, and whether it is a link. Everything is read with
//! bounds checks, because the bytes come from a disk that may be damaged.

/// A record starts with this.
const SIGNATURE: &[u8; 4] = b"FILE";
/// The fix-up stride: the last two bytes of every 512 are replaced by a check value on disk.
const SECTOR: usize = 512;

const FLAG_IN_USE: u16 = 0x1;
const FLAG_DIR: u16 = 0x2;

const ATTR_STANDARD_INFORMATION: u32 = 0x10;
const ATTR_LIST: u32 = 0x20;
const ATTR_FILE_NAME: u32 = 0x30;
const ATTR_DATA: u32 = 0x80;
const ATTR_REPARSE_POINT: u32 = 0xC0;
const ATTR_END: u32 = 0xFFFF_FFFF;

/// Reparse tags of symbolic links and junctions: the scan does not follow these.
const TAG_SYMLINK: u32 = 0xA000_000C;
const TAG_MOUNT_POINT: u32 = 0xA000_0003;

/// Seconds between 1601 (FILETIME) and 1970 (Unix).
const FILETIME_TO_UNIX: u64 = 11_644_473_600;

/// What a file record says, for the parts a scan uses.
#[expect(
    clippy::struct_excessive_bools,
    reason = "each flag is a separate fact the record states"
)]
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Record {
    /// Entry number of the base record when this is an extension record, 0 for a base record.
    pub base: u64,
    pub in_use: bool,
    pub is_dir: bool,
    /// A symbolic link or junction.
    pub link: bool,
    /// The record has an attribute list: some of its attributes live in extension records.
    pub has_list: bool,
    /// Modification time, seconds since 1970 (0 when unknown or earlier).
    pub mtime: i64,
    /// Size of the unnamed data stream, where this record holds it.
    pub size: Option<u64>,
    /// Parent entry number and name (the Win32 name where there are several).
    pub name: Option<(u64, String)>,
}

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn u64_at(b: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(b.get(at..at + 8)?.try_into().ok()?))
}

/// The entry number in a 64-bit file reference (the top 16 bits are a sequence number).
const fn entry_of(reference: u64) -> u64 {
    reference & 0x0000_FFFF_FFFF_FFFF
}

/// Puts back the bytes the fix-up array replaced. False when a sector's check value is wrong:
/// the record was only partly written, and is not to be trusted.
pub fn apply_fixups(buf: &mut [u8]) -> bool {
    let (Some(offset), Some(count)) = (u16_at(buf, 4), u16_at(buf, 6)) else {
        return false;
    };
    let (offset, count) = (usize::from(offset), usize::from(count));
    if count == 0 || offset + count * 2 > buf.len() || count - 1 > buf.len() / SECTOR {
        return false;
    }
    let check = [buf[offset], buf[offset + 1]];
    for i in 1..count {
        let end = i * SECTOR;
        if end > buf.len() || buf[end - 2..end] != check {
            return false;
        }
        let saved = offset + i * 2;
        buf[end - 2] = buf[saved];
        buf[end - 1] = buf[saved + 1];
    }
    true
}

/// Seconds since 1970 for a FILETIME (100 ns ticks since 1601); 0 before the epoch.
fn unix_of(filetime: u64) -> i64 {
    (filetime / 10_000_000)
        .checked_sub(FILETIME_TO_UNIX)
        .map_or(0, |s| i64::try_from(s).unwrap_or(i64::MAX))
}

/// One attribute of a record: its type, whether it has a name, and where its parts are.
struct Attr<'a> {
    kind: u32,
    named: bool,
    resident: bool,
    /// The whole attribute, header included.
    bytes: &'a [u8],
}

impl Attr<'_> {
    /// The value of a resident attribute.
    fn value(&self) -> Option<&[u8]> {
        if !self.resident {
            return None;
        }
        let len = usize::try_from(u32_at(self.bytes, 0x10)?).ok()?;
        let at = usize::from(u16_at(self.bytes, 0x14)?);
        self.bytes.get(at..at.checked_add(len)?)
    }
}

/// The attributes of a record whose fix-ups are applied, stopping at the first damaged one.
fn attributes(buf: &[u8]) -> impl Iterator<Item = Attr<'_>> {
    let mut at = u16_at(buf, 0x14).map_or(usize::MAX, usize::from);
    std::iter::from_fn(move || {
        let kind = u32_at(buf, at)?;
        if kind == ATTR_END {
            return None;
        }
        let len = usize::try_from(u32_at(buf, at + 4)?).ok()?;
        if len < 0x18 || len % 8 != 0 {
            return None;
        }
        let bytes = buf.get(at..at.checked_add(len)?)?;
        at += len;
        Some(Attr {
            kind,
            named: *bytes.get(9)? != 0,
            resident: *bytes.get(8)? == 0,
            bytes,
        })
    })
}

/// A file name attribute's parent, name and namespace (0 POSIX, 1 Win32, 2 DOS, 3 both).
fn file_name(value: &[u8]) -> Option<(u64, String, u8)> {
    let parent = entry_of(u64_at(value, 0)?);
    let len = usize::from(*value.get(0x40)?);
    let namespace = *value.get(0x41)?;
    let raw = value.get(0x42..0x42 + len * 2)?;
    let units: Vec<u16> = raw
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes(*c))
        .collect();
    Some((parent, String::from_utf16_lossy(&units), namespace))
}

/// Reads a record whose fix-ups are applied. `None` for what is not a file record.
pub fn parse(buf: &[u8]) -> Option<Record> {
    if buf.get(..4)? != SIGNATURE {
        return None;
    }
    let flags = u16_at(buf, 0x16)?;
    let mut rec = Record {
        base: entry_of(u64_at(buf, 0x20)?),
        in_use: flags & FLAG_IN_USE != 0,
        is_dir: flags & FLAG_DIR != 0,
        ..Record::default()
    };
    // The DOS name is only the fallback: a 8.3 alias names the same file as the real one.
    let mut best_namespace = u8::MAX;
    for attr in attributes(buf) {
        match attr.kind {
            ATTR_STANDARD_INFORMATION => {
                if let Some(modified) = attr.value().and_then(|v| u64_at(v, 8)) {
                    rec.mtime = unix_of(modified);
                }
            }
            ATTR_LIST => rec.has_list = true,
            ATTR_FILE_NAME => {
                if let Some((parent, name, namespace)) = attr.value().and_then(file_name) {
                    let rank = u8::from(namespace == 2);
                    if best_namespace == u8::MAX || rank < best_namespace {
                        best_namespace = rank;
                        rec.name = Some((parent, name));
                    }
                }
            }
            ATTR_DATA if !attr.named => {
                if attr.resident {
                    rec.size = u32_at(attr.bytes, 0x10).map(u64::from);
                } else if u64_at(attr.bytes, 0x10) == Some(0) {
                    // Only the first extent says how big the whole stream is.
                    rec.size = u64_at(attr.bytes, 0x30);
                }
            }
            ATTR_REPARSE_POINT => {
                let tag = attr.value().and_then(|v| u32_at(v, 0));
                rec.link = matches!(tag, Some(TAG_SYMLINK | TAG_MOUNT_POINT));
            }
            _ => {}
        }
    }
    Some(rec)
}

/// A piece of a non-resident attribute: `lcn` is the first cluster, `None` for a hole.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Run {
    pub lcn: Option<u64>,
    pub clusters: u64,
}

/// Reads a run list: pairs of (length, offset from the previous run) in as few bytes as fit.
pub fn runs(list: &[u8]) -> Option<Vec<Run>> {
    let mut out = Vec::new();
    let mut at = 0;
    let mut lcn: i64 = 0;
    while let Some(&head) = list.get(at) {
        if head == 0 {
            break;
        }
        let (len_bytes, off_bytes) = (usize::from(head & 0x0F), usize::from(head >> 4));
        if len_bytes == 0 || len_bytes > 8 || off_bytes > 8 {
            return None;
        }
        at += 1;
        let clusters = le_unsigned(list.get(at..at + len_bytes)?);
        at += len_bytes;
        if off_bytes == 0 {
            out.push(Run {
                lcn: None,
                clusters,
            });
        } else {
            let delta = le_signed(list.get(at..at + off_bytes)?);
            at += off_bytes;
            lcn = lcn.checked_add(delta)?;
            out.push(Run {
                lcn: Some(u64::try_from(lcn).ok()?),
                clusters,
            });
        }
    }
    Some(out)
}

fn le_unsigned(b: &[u8]) -> u64 {
    b.iter().rev().fold(0, |n, &x| (n << 8) | u64::from(x))
}

fn le_signed(b: &[u8]) -> i64 {
    let negative = b.last().is_some_and(|&x| x & 0x80 != 0);
    let mut n: u64 = if negative { u64::MAX } else { 0 };
    for &x in b.iter().rev() {
        n = (n << 8) | u64::from(x);
    }
    #[expect(
        clippy::cast_possible_wrap,
        reason = "sign-extended from the stored width on purpose"
    )]
    let signed = n as i64;
    signed
}

/// The run list and real size of the unnamed data stream of a record (the MFT's own record).
pub fn data_runs(buf: &[u8]) -> Option<(Vec<Run>, u64)> {
    let attr = attributes(buf).find(|a| a.kind == ATTR_DATA && !a.named && !a.resident)?;
    let at = usize::from(u16_at(attr.bytes, 0x20)?);
    let size = u64_at(attr.bytes, 0x30)?;
    Some((runs(attr.bytes.get(at..)?)?, size))
}

/// Builders of raw records, for tests of this module and of those that read them.
#[cfg(test)]
pub mod testing {
    use super::{SECTOR, entry_of};

    /// What a test record holds.
    #[expect(
        clippy::struct_excessive_bools,
        reason = "each flag is a separate fact the record states"
    )]
    #[derive(Default)]
    pub struct Spec<'a> {
        pub in_use: bool,
        pub dir: bool,
        pub base: u64,
        pub parent: u64,
        pub name: &'a str,
        /// 0 POSIX, 1 Win32, 2 DOS, 3 both.
        pub namespace: u8,
        pub size: Option<u64>,
        /// The data is non-resident (a real size, as for any file that is not tiny).
        pub non_resident: bool,
        pub mtime_unix: u64,
        pub reparse: Option<u32>,
        pub list: bool,
        pub extra_names: Vec<(u64, &'a str, u8)>,
        pub runs: Option<Vec<u8>>,
    }

    fn attr(kind: u32, resident: Option<&[u8]>, body: &[u8]) -> Vec<u8> {
        let mut a = Vec::new();
        if let Some(value) = resident {
            let len = 0x18 + value.len();
            let padded = len.next_multiple_of(8);
            a.extend(kind.to_le_bytes());
            a.extend(u32::try_from(padded).unwrap().to_le_bytes());
            a.extend([0, 0, 0, 0]); // resident, no name, name offset
            a.extend([0, 0, 0, 0]); // flags, instance
            a.extend(u32::try_from(value.len()).unwrap().to_le_bytes());
            a.extend(0x18u16.to_le_bytes());
            a.extend([0, 0]);
            a.extend(value);
            a.resize(padded, 0);
        } else {
            // Non-resident: a 0x40-byte header, then `body` as the run list.
            a.resize(0x40, 0);
            a[0..4].copy_from_slice(&kind.to_le_bytes());
            a[8] = 1;
            a.extend(body);
            a.resize(a.len().next_multiple_of(8), 0);
            let len = u32::try_from(a.len()).unwrap();
            a[4..8].copy_from_slice(&len.to_le_bytes());
        }
        a
    }

    fn file_name_value(parent: u64, name: &str, namespace: u8, dir: bool) -> Vec<u8> {
        let units: Vec<u16> = name.encode_utf16().collect();
        let mut v = vec![0u8; 0x42];
        v[0..8].copy_from_slice(&(parent | (7 << 48)).to_le_bytes());
        let flags: u32 = if dir { 0x1000_0000 } else { 0 };
        v[0x38..0x3C].copy_from_slice(&flags.to_le_bytes());
        v[0x40] = u8::try_from(units.len()).unwrap();
        v[0x41] = namespace;
        for u in units {
            v.extend(u.to_le_bytes());
        }
        v
    }

    /// A 1024-byte record with fix-ups applied the way the disk stores them.
    pub fn record(spec: &Spec) -> Vec<u8> {
        let mut attrs = Vec::new();
        let ft = (spec.mtime_unix + 11_644_473_600) * 10_000_000;
        let mut si = vec![0u8; 0x30];
        si[8..16].copy_from_slice(&ft.to_le_bytes());
        attrs.extend(attr(0x10, Some(&si), &[]));
        if spec.list {
            attrs.extend(attr(0x20, Some(&[0u8; 8]), &[]));
        }
        if !spec.name.is_empty() {
            let v = file_name_value(spec.parent, spec.name, spec.namespace, spec.dir);
            attrs.extend(attr(0x30, Some(&v), &[]));
        }
        for (parent, name, ns) in &spec.extra_names {
            let v = file_name_value(*parent, name, *ns, spec.dir);
            attrs.extend(attr(0x30, Some(&v), &[]));
        }
        if let Some(tag) = spec.reparse {
            attrs.extend(attr(0xC0, Some(&tag.to_le_bytes()), &[]));
        }
        if spec.non_resident || spec.runs.is_some() {
            let runs = spec.runs.clone().unwrap_or_else(|| vec![0]);
            let mut a = attr(0x80, None, &runs);
            let size = spec.size.unwrap_or(0).to_le_bytes();
            a[0x20..0x22].copy_from_slice(&0x40u16.to_le_bytes());
            for at in [0x28, 0x30, 0x38] {
                a[at..at + 8].copy_from_slice(&size);
            }
            attrs.extend(a);
        } else if let Some(size) = spec.size {
            let data = vec![0u8; usize::try_from(size).unwrap()];
            attrs.extend(attr(0x80, Some(&data), &[]));
        }
        attrs.extend(0xFFFF_FFFFu32.to_le_bytes());

        let mut r = vec![0u8; 1024];
        r[0..4].copy_from_slice(b"FILE");
        r[4..6].copy_from_slice(&0x30u16.to_le_bytes()); // fix-up array
        r[6..8].copy_from_slice(&3u16.to_le_bytes()); // check value + 2 sectors
        r[0x14..0x16].copy_from_slice(&0x38u16.to_le_bytes());
        let flags = u16::from(spec.in_use) | if spec.dir { 2 } else { 0 };
        r[0x16..0x18].copy_from_slice(&flags.to_le_bytes());
        r[0x20..0x28].copy_from_slice(&(entry_of(spec.base)).to_le_bytes());
        r[0x38..0x38 + attrs.len()].copy_from_slice(&attrs);
        // Fix-ups: the check value replaces the last two bytes of each sector.
        let check = [0x34, 0x12];
        r[0x30..0x32].copy_from_slice(&check);
        for i in 1..3 {
            let end = i * SECTOR;
            r[0x30 + i * 2] = r[end - 2];
            r[0x30 + i * 2 + 1] = r[end - 1];
            r[end - 2..end].copy_from_slice(&check);
        }
        r
    }
}

#[cfg(test)]
mod tests {
    use super::testing::{Spec, record};
    use super::*;

    fn read(spec: &Spec) -> Record {
        let mut raw = record(spec);
        assert!(apply_fixups(&mut raw));
        parse(&raw).unwrap()
    }

    #[test]
    fn a_small_file_has_its_name_parent_size_and_time() {
        let r = read(&Spec {
            in_use: true,
            parent: 5,
            name: "notes.txt",
            namespace: 1,
            size: Some(120),
            mtime_unix: 1_700_000_000,
            ..Spec::default()
        });
        assert!(r.in_use && !r.is_dir && !r.link);
        assert_eq!(r.name, Some((5, "notes.txt".to_owned())));
        assert_eq!(r.size, Some(120));
        assert_eq!(r.mtime, 1_700_000_000);
    }

    #[test]
    fn a_large_file_takes_its_real_size_from_the_non_resident_header() {
        let r = read(&Spec {
            in_use: true,
            parent: 40,
            name: "movie.mkv",
            namespace: 1,
            size: Some(5_000_000_000),
            non_resident: true,
            ..Spec::default()
        });
        assert_eq!(r.size, Some(5_000_000_000));
    }

    #[test]
    fn a_folder_is_a_folder_and_a_deleted_entry_is_not_in_use() {
        let dir = read(&Spec {
            in_use: true,
            dir: true,
            parent: 5,
            name: "Users",
            namespace: 3,
            ..Spec::default()
        });
        assert!(dir.is_dir && dir.in_use);
        let gone = read(&Spec {
            in_use: false,
            parent: 5,
            name: "old.txt",
            namespace: 1,
            ..Spec::default()
        });
        assert!(!gone.in_use);
    }

    #[test]
    fn the_long_name_wins_over_the_dos_alias_whichever_comes_first() {
        let r = read(&Spec {
            in_use: true,
            parent: 5,
            name: "PROGRA~1",
            namespace: 2,
            extra_names: vec![(5, "Program Files", 1)],
            ..Spec::default()
        });
        assert_eq!(r.name, Some((5, "Program Files".to_owned())));
        let only_dos = read(&Spec {
            in_use: true,
            parent: 5,
            name: "ABC~1.TXT",
            namespace: 2,
            ..Spec::default()
        });
        assert_eq!(only_dos.name.unwrap().1, "ABC~1.TXT");
    }

    #[test]
    fn symlinks_and_junctions_are_links_and_other_reparse_points_are_not() {
        let link = |tag| {
            read(&Spec {
                in_use: true,
                parent: 5,
                name: "l",
                namespace: 1,
                reparse: Some(tag),
                ..Spec::default()
            })
            .link
        };
        assert!(link(0xA000_000C));
        assert!(link(0xA000_0003));
        // A OneDrive placeholder is a file like any other.
        assert!(!link(0x9000_601A));
    }

    #[test]
    fn an_extension_record_names_its_base_and_a_list_is_noticed() {
        let ext = read(&Spec {
            in_use: true,
            base: 77,
            size: Some(9),
            ..Spec::default()
        });
        assert_eq!(ext.base, 77);
        let listed = read(&Spec {
            in_use: true,
            parent: 5,
            name: "frag.bin",
            namespace: 1,
            list: true,
            ..Spec::default()
        });
        assert!(listed.has_list && listed.size.is_none());
    }

    #[test]
    fn a_torn_record_is_refused() {
        let mut raw = record(&Spec {
            in_use: true,
            parent: 5,
            name: "x",
            namespace: 1,
            ..Spec::default()
        });
        raw[510] ^= 0xFF;
        assert!(!apply_fixups(&mut raw));
    }

    #[test]
    fn what_is_not_a_record_is_not_parsed_and_short_input_does_not_panic() {
        assert!(parse(&[0u8; 1024]).is_none());
        assert!(parse(b"FIL").is_none());
        let mut raw = record(&Spec {
            in_use: true,
            parent: 5,
            name: "x",
            namespace: 1,
            ..Spec::default()
        });
        // Damage the first attribute's length: reading stops, nothing panics.
        raw[0x38 + 4..0x38 + 8].copy_from_slice(&0xFFFF_FFF0u32.to_le_bytes());
        let _ = parse(&raw);
        for cut in [0, 7, 0x20, 0x39, 100] {
            let _ = parse(&raw[..cut]);
            let _ = apply_fixups(&mut raw[..cut].to_vec());
        }
    }

    #[test]
    fn run_lists_follow_relative_offsets_and_mark_holes() {
        // 0x21: 1 length byte, 2 offset bytes. 8 clusters at 0x1000, then 4 more 0x100 earlier,
        // then a hole of 16 clusters.
        let list = [
            0x21, 0x08, 0x00, 0x10, 0x21, 0x04, 0x00, 0xFF, 0x01, 0x10, 0x00,
        ];
        assert_eq!(
            runs(&list).unwrap(),
            vec![
                Run {
                    lcn: Some(0x1000),
                    clusters: 8
                },
                Run {
                    lcn: Some(0x0F00),
                    clusters: 4
                },
                Run {
                    lcn: None,
                    clusters: 16
                },
            ]
        );
        assert!(runs(&[0x09, 1]).is_none());
        // A run that would start before the volume is refused.
        assert!(runs(&[0x11, 0x01, 0x80]).is_none());
    }

    #[test]
    fn data_runs_of_the_mft_record_give_runs_and_size() {
        let mut raw = record(&Spec {
            in_use: true,
            parent: 5,
            name: "$MFT",
            namespace: 3,
            size: Some(8192),
            runs: Some(vec![0x11, 0x08, 0x20, 0x00]),
            ..Spec::default()
        });
        assert!(apply_fixups(&mut raw));
        let (found, size) = data_runs(&raw).unwrap();
        assert_eq!(size, 8192);
        assert_eq!(
            found,
            vec![Run {
                lcn: Some(0x20),
                clusters: 8
            }]
        );
    }
}
