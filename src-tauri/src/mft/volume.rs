//! Reading the master file table off an NTFS volume (or an image of one).
//!
//! The volume is any `Read + Seek`, so the layout logic is tested on a made-up image; on a real
//! drive it is the raw volume handle, which wants reads that are a whole number of sectors at a
//! sector boundary. Everything here reads clusters at cluster offsets, so that holds.

use std::io::{self, Read, Seek, SeekFrom};

use super::record::{self, Run};

/// How much of the table is read at a time.
const CHUNK: u64 = 4 << 20;

/// Why a volume could not be read this way.
#[derive(Debug)]
pub enum Failure {
    NotNtfs,
    /// A layout this reader does not handle: the scan falls back to walking.
    Unsupported(&'static str),
    Io(io::Error),
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotNtfs => f.write_str("the drive is not NTFS"),
            Self::Unsupported(why) => write!(
                f,
                "the drive's file table is laid out in a way fast scan does not read ({why})"
            ),
            Self::Io(e) => write!(f, "the drive could not be read: {e}"),
        }
    }
}

impl From<io::Error> for Failure {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

/// Where the table is and how big its records are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Geometry {
    pub cluster: u64,
    pub mft_offset: u64,
    pub record: usize,
}

/// The table's own layout: where its records are on disk, and how long it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    pub geometry: Geometry,
    pub runs: Vec<Run>,
    pub size: u64,
}

/// Reads the boot sector and the table's first record, which says where the rest of it is.
pub fn layout<R: Read + Seek>(volume: &mut R) -> Result<Layout, Failure> {
    // One whole-sector read: a raw volume refuses the small probing reads `read_to_end` makes.
    let mut boot = vec![0u8; 4096];
    volume.seek(SeekFrom::Start(0))?;
    let mut have = 0;
    while have < boot.len() {
        let n = volume.read(&mut boot[have..])?;
        if n == 0 {
            break;
        }
        have += n;
    }
    boot.truncate(have);
    if boot.len() < 0x48 || &boot[3..11] != b"NTFS    " {
        return Err(Failure::NotNtfs);
    }
    let sector = u64::from(u16::from_le_bytes([boot[0x0B], boot[0x0C]]));
    let per_cluster = u64::from(boot[0x0D]);
    let cluster = sector * per_cluster;
    if sector < 512 || per_cluster == 0 || cluster > 2 << 20 {
        return Err(Failure::Unsupported("odd cluster size"));
    }
    let mft_lcn = u64::from_le_bytes(boot[0x30..0x38].try_into().unwrap_or_default());
    #[expect(
        clippy::cast_possible_wrap,
        reason = "the byte is a signed count, as the format says"
    )]
    let per_record = boot[0x40] as i8;
    let record = if per_record > 0 {
        u64::from(per_record.unsigned_abs()) * cluster
    } else if (9..=16).contains(&per_record.unsigned_abs()) {
        1u64 << per_record.unsigned_abs()
    } else {
        return Err(Failure::Unsupported("odd record size"));
    };
    let record = usize::try_from(record).map_err(|_| Failure::Unsupported("odd record size"))?;
    let geometry = Geometry {
        cluster,
        mft_offset: mft_lcn
            .checked_mul(cluster)
            .ok_or(Failure::Unsupported("odd table position"))?,
        record,
    };

    let mut first = vec![0u8; record];
    volume.seek(SeekFrom::Start(geometry.mft_offset))?;
    volume.read_exact(&mut first)?;
    if !record::apply_fixups(&mut first) {
        return Err(Failure::Unsupported("the table's first record is damaged"));
    }
    let own = record::parse(&first).ok_or(Failure::Unsupported("no table record"))?;
    if own.has_list {
        return Err(Failure::Unsupported(
            "the table is split over extension records",
        ));
    }
    let (runs, size) =
        record::data_runs(&first).ok_or(Failure::Unsupported("no data runs for the table"))?;
    Ok(Layout {
        geometry,
        runs,
        size,
    })
}

/// Calls `each(entry number, record bytes)` for every record of the table, fix-ups not yet
/// applied. `stop` is asked between reads; true ends the read early. Returns whether the whole
/// table was read.
pub fn for_each_record<R, S, F>(
    volume: &mut R,
    layout: &Layout,
    mut stop: S,
    mut each: F,
) -> Result<bool, Failure>
where
    R: Read + Seek,
    S: FnMut() -> bool,
    F: FnMut(u64, &mut [u8]),
{
    let Geometry {
        cluster, record, ..
    } = layout.geometry;
    let mut pending: Vec<u8> = Vec::new();
    let mut index = 0u64;
    let mut chunk = vec![0u8; usize::try_from(CHUNK).unwrap_or(4 << 20)];
    let limit = layout.size;
    for run in &layout.runs {
        let total = run.clusters.saturating_mul(cluster);
        let mut done = 0u64;
        while done < total {
            if stop() {
                return Ok(false);
            }
            let take = (total - done).min(CHUNK);
            let part =
                &mut chunk[..usize::try_from(take).map_err(|_| Failure::Unsupported("chunk"))?];
            match run.lcn {
                Some(lcn) => {
                    let at = lcn
                        .checked_mul(cluster)
                        .and_then(|b| b.checked_add(done))
                        .ok_or(Failure::Unsupported("odd table position"))?;
                    volume.seek(SeekFrom::Start(at))?;
                    volume.read_exact(part)?;
                }
                // A hole in the table: nothing there.
                None => part.fill(0),
            }
            pending.extend_from_slice(part);
            done += take;
            let mut used = 0;
            while pending.len() - used >= record {
                if index * record as u64 >= limit {
                    return Ok(true);
                }
                each(index, &mut pending[used..used + record]);
                used += record;
                index += 1;
            }
            pending.drain(..used);
        }
    }
    Ok(true)
}

#[cfg(test)]
pub mod testing {
    use super::super::record::testing::{Spec, record};
    use std::io::Cursor;

    /// A made-up NTFS image: 512-byte sectors, 2-sector clusters (1 KiB) and 1 KiB records. The
    /// table starts at cluster 4 and is split into two runs with a gap between, so reading
    /// across a break is exercised. `specs[i]` is entry `i` of the table (entries 0 to 4 and the
    /// root at 5 are the caller's to fill in).
    pub fn image(specs: &[Spec]) -> Cursor<Vec<u8>> {
        let cluster = 1024usize;
        let first_run_records = specs.len().div_ceil(2).max(1);
        let second_run_records = specs.len() - first_run_records;
        let (lcn1, lcn2) = (4usize, 4 + first_run_records + 10);
        let mut img = vec![0u8; (lcn2 + second_run_records + 2) * cluster];
        img[3..11].copy_from_slice(b"NTFS    ");
        img[0x0B..0x0D].copy_from_slice(&512u16.to_le_bytes());
        img[0x0D] = 2;
        img[0x30..0x38].copy_from_slice(&u64::try_from(lcn1).unwrap().to_le_bytes());
        img[0x40] = 1; // one cluster per record
        let mut list = vec![
            0x11,
            u8::try_from(first_run_records).unwrap(),
            u8::try_from(lcn1).unwrap(),
        ];
        if second_run_records > 0 {
            let delta = i64::try_from(lcn2).unwrap() - i64::try_from(lcn1).unwrap();
            list.extend([
                0x11,
                u8::try_from(second_run_records).unwrap(),
                u8::try_from(delta).unwrap(),
            ]);
        }
        list.push(0);
        for (i, spec) in specs.iter().enumerate() {
            let mut raw = if i == 0 {
                record(&Spec {
                    in_use: true,
                    name: "$MFT",
                    namespace: 3,
                    parent: 5,
                    size: Some((specs.len() * cluster) as u64),
                    runs: Some(list.clone()),
                    ..Spec::default()
                })
            } else {
                record(spec)
            };
            let at = if i < first_run_records {
                (lcn1 + i) * cluster
            } else {
                (lcn2 + i - first_run_records) * cluster
            };
            img[at..at + cluster].copy_from_slice(&raw);
            raw.clear();
        }
        Cursor::new(img)
    }
}

#[cfg(test)]
mod tests {
    use super::testing::image;
    use super::*;
    use crate::mft::record::testing::Spec;

    fn file(parent: u64, name: &str, size: u64) -> Spec<'_> {
        Spec {
            in_use: true,
            parent,
            name,
            namespace: 1,
            size: Some(size),
            ..Spec::default()
        }
    }

    fn specs() -> Vec<Spec<'static>> {
        let mut v: Vec<Spec> = (0..24).map(|_| Spec::default()).collect();
        v[5] = Spec {
            in_use: true,
            dir: true,
            parent: 5,
            name: ".",
            namespace: 1,
            ..Spec::default()
        };
        v.push(file(5, "a.txt", 10));
        v.push(file(5, "b.txt", 20));
        v
    }

    #[test]
    fn the_layout_comes_from_the_boot_sector_and_the_first_record() {
        let mut img = image(&specs());
        let found = layout(&mut img).unwrap();
        assert_eq!(found.geometry.cluster, 1024);
        assert_eq!(found.geometry.record, 1024);
        assert_eq!(found.geometry.mft_offset, 4096);
        assert_eq!(found.runs.len(), 2);
        assert_eq!(found.size, 26 * 1024);
    }

    #[test]
    fn every_record_is_read_across_the_break_between_runs() {
        let mut img = image(&specs());
        let found = layout(&mut img).unwrap();
        let mut names = Vec::new();
        let done = for_each_record(
            &mut img,
            &found,
            || false,
            |i, raw| {
                if record::apply_fixups(raw)
                    && let Some(r) = record::parse(raw)
                    && let Some((_, name)) = r.name
                {
                    names.push((i, name));
                }
            },
        )
        .unwrap();
        assert!(done);
        assert_eq!(
            names,
            vec![
                (0, "$MFT".to_owned()),
                (5, ".".to_owned()),
                (24, "a.txt".to_owned()),
                (25, "b.txt".to_owned())
            ]
        );
    }

    #[test]
    fn a_stop_request_ends_the_read_early() {
        let mut img = image(&specs());
        let found = layout(&mut img).unwrap();
        let mut seen = 0;
        let done = for_each_record(&mut img, &found, || true, |_, _| seen += 1).unwrap();
        assert!(!done);
        assert_eq!(seen, 0);
    }

    #[test]
    fn a_volume_that_is_not_ntfs_is_said_to_be() {
        let mut fat = io::Cursor::new(vec![0u8; 4096]);
        assert!(matches!(layout(&mut fat), Err(Failure::NotNtfs)));
        let mut tiny = io::Cursor::new(vec![0u8; 10]);
        assert!(matches!(layout(&mut tiny), Err(Failure::NotNtfs)));
    }

    #[test]
    fn a_damaged_first_record_is_unsupported_rather_than_a_panic() {
        let mut img = image(&specs());
        img.get_mut()[4096 + 510] ^= 0xFF;
        assert!(matches!(layout(&mut img), Err(Failure::Unsupported(_))));
    }
}
