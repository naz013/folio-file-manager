//! Fast disk scan: the sizes of a whole NTFS drive from its master file table (ADR 0004).
//!
//! `record` reads one table record, `volume` reads the table off a volume, and this module
//! turns the records into the folder totals and big files a scan stores. The parts that need
//! Windows (the raw volume, the elevated helper and its pipe) are in `helper` and `client`.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::io::{Read, Seek};

use serde::{Deserialize, Serialize};

use crate::scan::{Acc, BIG_MIN, CATEGORIES, Tree, category};

#[cfg(windows)]
pub mod client;
#[cfg(windows)]
pub mod helper;
pub mod record;
pub mod volume;

pub use volume::Failure;

/// What a fast scan came to.
pub enum Fast {
    Tree(Tree),
    Cancelled,
    /// It could not be done: the reason, for the note shown after the normal scan.
    Fallback(String),
}

/// Scans `root` from the drive's file table through the elevated helper, where the system has
/// one; reports through `progress(files, bytes)`.
pub fn scan(
    root: &std::path::Path,
    cancel: &std::sync::atomic::AtomicBool,
    progress: &dyn Fn(u64, u64),
) -> Fast {
    #[cfg(windows)]
    {
        client::scan(root, cancel, progress)
    }
    #[cfg(not(windows))]
    {
        let _ = (root, cancel, progress);
        Fast::Fallback("fast scan is only available on Windows".to_owned())
    }
}

/// How often a running read reports, in table records.
const PROGRESS_EVERY: u64 = 20_000;
/// Entry numbers below this are NTFS's own files (`$MFT`, `$Bitmap`...), except the root.
const FIRST_USER_ENTRY: u64 = 24;
const ROOT_ENTRY: u64 = 5;
/// NTFS paths always use this, whatever system the code is built on (the tests run on all).
const SEP: char = '\\';
/// A path through more folders than this is a loop or damage.
const MAX_DEPTH: usize = 1024;

/// What the helper says to Folio, one JSON object per line.
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "t", rename_all = "lowercase")]
pub enum Message {
    Progress {
        files: u64,
        bytes: u64,
    },
    /// A folder, its path relative to the folder asked for ("" for that folder itself).
    Dir {
        path: String,
        own: u64,
        files: u64,
        cats: [u64; CATEGORIES.len()],
    },
    /// A file of 1 MB or more: its path and its folder's, relative like `Dir`.
    Big {
        path: String,
        dir: String,
        size: u64,
        mtime: i64,
    },
    Done {
        files: u64,
        bytes: u64,
    },
    /// The read could not be done; Folio walks the drive instead.
    Failed {
        why: String,
    },
}

struct Node {
    parent: u64,
    name: String,
}

struct Late {
    entry: u64,
    parent: u64,
    name: String,
    mtime: i64,
}

struct BigFile {
    parent: u64,
    name: String,
    size: u64,
    mtime: i64,
}

/// Collects the records of a table and turns them into totals per folder.
#[derive(Default)]
pub struct Builder {
    dirs: HashMap<u64, Node>,
    own: HashMap<u64, Acc>,
    big: Vec<BigFile>,
    /// Files whose size is kept in an extension record, resolved at the end.
    late: Vec<Late>,
    extension_sizes: HashMap<u64, u64>,
}

impl Builder {
    fn add_file(&mut self, parent: u64, name: &str, size: u64, mtime: i64) {
        let acc = self.own.entry(parent).or_default();
        acc.own += size;
        acc.own_files += 1;
        acc.cats[category(OsStr::new(name))] += size;
        if size >= BIG_MIN {
            self.big.push(BigFile {
                parent,
                name: name.to_owned(),
                size,
                mtime,
            });
        }
    }

    /// Takes the record at table entry `entry`.
    pub fn feed(&mut self, entry: u64, rec: record::Record) {
        // NTFS's own files are not the user's, but the root folder is entry 5.
        if (entry < FIRST_USER_ENTRY && entry != ROOT_ENTRY) || !rec.in_use {
            return;
        }
        if rec.base != 0 {
            if let Some(size) = rec.size {
                self.extension_sizes.entry(rec.base).or_insert(size);
            }
            return;
        }
        let Some((parent, name)) = rec.name else {
            return;
        };
        if rec.link {
            return;
        }
        if rec.is_dir {
            self.dirs.insert(entry, Node { parent, name });
        } else if let Some(size) = rec.size {
            self.add_file(parent, &name, size, rec.mtime);
        } else if rec.has_list {
            self.late.push(Late {
                entry,
                parent,
                name,
                mtime: rec.mtime,
            });
        } else {
            self.add_file(parent, &name, 0, rec.mtime);
        }
    }

    /// The path of folder `entry` from the volume root, or `None` when its parents do not lead
    /// there (deleted, or a loop).
    fn path_of(
        &self,
        entry: u64,
        root: &str,
        memo: &mut HashMap<u64, Option<String>>,
    ) -> Option<String> {
        let mut chain = Vec::new();
        let mut at = entry;
        let mut base = loop {
            if let Some(known) = memo.get(&at) {
                break known.clone();
            }
            if at == ROOT_ENTRY {
                break Some(root.to_owned());
            }
            let node = self.dirs.get(&at)?;
            if chain.len() >= MAX_DEPTH {
                return None;
            }
            chain.push(at);
            at = node.parent;
        };
        for entry in chain.into_iter().rev() {
            base = base.map(|parent| {
                let name = &self.dirs[&entry].name;
                if parent.ends_with(SEP) {
                    format!("{parent}{name}")
                } else {
                    format!("{parent}{SEP}{name}")
                }
            });
            memo.insert(entry, base.clone());
        }
        base
    }

    /// Sends what lies under `within` (a folder path, as the walker would name it) to `out`,
    /// then returns the number of files and bytes sent. `volume` is the root of the drive.
    pub fn finish(
        mut self,
        volume: &str,
        within: &str,
        out: &mut dyn FnMut(Message) -> bool,
    ) -> Option<(u64, u64)> {
        for late in std::mem::take(&mut self.late) {
            let size = self.extension_sizes.get(&late.entry).copied().unwrap_or(0);
            self.add_file(late.parent, &late.name, size, late.mtime);
        }
        let inside = Inside::new(within);
        let skip = within
            .trim_end_matches(['\\', '/'])
            .split(['\\', '/'])
            .count();
        let mut memo: HashMap<u64, Option<String>> = HashMap::new();
        let mut shown: HashMap<u64, String> = HashMap::new();
        let (mut files, mut bytes) = (0u64, 0u64);
        let mut entries: Vec<u64> = self.dirs.keys().copied().collect();
        entries.push(ROOT_ENTRY);
        entries.sort_unstable();
        entries.dedup();
        for entry in entries {
            let Some(path) = self.path_of(entry, volume, &mut memo) else {
                continue;
            };
            if !inside.has(&path) {
                continue;
            }
            // Sent relative to the folder asked for, so Folio spells it as it was asked.
            let rel = path.split(SEP).skip(skip).collect::<Vec<_>>().join("\\");
            let acc = self.own.get(&entry).cloned().unwrap_or_default();
            files += acc.own_files;
            bytes += acc.own;
            if !out(Message::Dir {
                path: rel.clone(),
                own: acc.own,
                files: acc.own_files,
                cats: acc.cats,
            }) {
                return None;
            }
            shown.insert(entry, rel);
        }
        // The full paths are no longer needed.
        drop(memo);
        for big in &self.big {
            let Some(dir) = shown.get(&big.parent) else {
                continue;
            };
            let path = if dir.is_empty() {
                big.name.clone()
            } else {
                format!("{dir}{SEP}{}", big.name)
            };
            if !out(Message::Big {
                path,
                dir: dir.clone(),
                size: big.size,
                mtime: big.mtime,
            }) {
                return None;
            }
        }
        Some((files, bytes))
    }
}

/// Whether a path is the folder asked for or below it, the way the cache compares: without
/// regard to case.
struct Inside {
    exact: String,
    below: String,
}

/// A path as the cache compares them: lower case, `\` only, and no trailing `\` except on a
/// drive root.
fn fold(path: &str) -> String {
    let lower = path.to_lowercase().replace('/', "\\");
    let trimmed = lower.trim_end_matches(SEP);
    if trimmed.is_empty() || trimmed.ends_with(':') {
        format!("{trimmed}{SEP}")
    } else {
        trimmed.to_owned()
    }
}

impl Inside {
    fn new(folder: &str) -> Self {
        let exact = fold(folder);
        let below = if exact.ends_with(SEP) {
            exact.clone()
        } else {
            format!("{exact}{SEP}")
        };
        Self { exact, below }
    }

    fn has(&self, path: &str) -> bool {
        let k = fold(path);
        k == self.exact || k.starts_with(&self.below)
    }
}

/// Reads the whole table of `volume` and sends what is under `within` through `out`: progress
/// while reading, then every folder and big file, then the end. `stop` is asked between reads.
/// Returns false when stopped, or when `out` refused a message (nobody is listening).
pub fn summarize<R: Read + Seek>(
    volume: &mut R,
    volume_root: &str,
    within: &str,
    stop: &mut dyn FnMut() -> bool,
    out: &mut dyn FnMut(Message) -> bool,
) -> Result<bool, Failure> {
    let layout = volume::layout(volume)?;
    let mut builder = Builder::default();
    let mut seen = 0u64;
    let listening = std::cell::Cell::new(true);
    let (mut files, mut bytes) = (0u64, 0u64);
    let finished = volume::for_each_record(
        volume,
        &layout,
        || !listening.get() || stop(),
        |entry, raw| {
            seen += 1;
            if record::apply_fixups(raw)
                && let Some(rec) = record::parse(raw)
            {
                if !rec.is_dir && rec.in_use && rec.base == 0 {
                    files += 1;
                    bytes += rec.size.unwrap_or(0);
                }
                builder.feed(entry, rec);
            }
            if seen.is_multiple_of(PROGRESS_EVERY) {
                listening.set(out(Message::Progress { files, bytes }));
            }
        },
    )?;
    if !finished || !listening.get() {
        return Ok(false);
    }
    let Some((files, bytes)) = builder.finish(volume_root, within, out) else {
        return Ok(false);
    };
    Ok(out(Message::Done { files, bytes }))
}

#[cfg(test)]
mod tests {
    use super::record::testing::Spec;
    use super::volume::testing::image;
    use super::*;

    fn dir(parent: u64, name: &str) -> Spec<'_> {
        Spec {
            in_use: true,
            dir: true,
            parent,
            name,
            namespace: 1,
            ..Spec::default()
        }
    }

    fn file(parent: u64, name: &str, size: u64) -> Spec<'_> {
        Spec {
            in_use: true,
            parent,
            name,
            namespace: 1,
            size: Some(size),
            non_resident: size > 700,
            mtime_unix: 1_700_000_000,
            ..Spec::default()
        }
    }

    /// C:\ holds a.txt; C:\Users (24) holds b.mp4 (3 MB) and C:\Users\me (25) holds c.png and
    /// d.docx.
    fn table() -> Vec<Spec<'static>> {
        let mut v: Vec<Spec> = (0..24).map(|_| Spec::default()).collect();
        v[5] = dir(5, ".");
        v.push(dir(5, "Users")); // 24
        v.push(dir(24, "me")); // 25
        v.push(file(5, "a.txt", 100)); // 26
        v.push(file(24, "b.mp4", 3 << 20)); // 27
        v.push(file(25, "c.png", 500)); // 28
        v.push(file(25, "d.docx", 800)); // 29
        v
    }

    fn run(specs: &[Spec], within: &str) -> Vec<Message> {
        let mut img = image(specs);
        let mut got = Vec::new();
        let ok = summarize(&mut img, "C:\\", within, &mut || false, &mut |m| {
            got.push(m);
            true
        })
        .unwrap();
        assert!(ok);
        got
    }

    fn dirs(msgs: &[Message]) -> HashMap<String, (u64, u64)> {
        msgs.iter()
            .filter_map(|m| match m {
                Message::Dir {
                    path, own, files, ..
                } => Some((path.clone(), (*own, *files))),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_whole_drive_gives_every_folder_its_own_files() {
        let got = run(&table(), "C:\\");
        let d = dirs(&got);
        assert_eq!(d[""], (100, 1));
        assert_eq!(d["Users"], (3 << 20, 1));
        assert_eq!(d[r"Users\me"], (1300, 2));
        assert_eq!(
            got.last(),
            Some(&Message::Done {
                files: 4,
                bytes: 100 + (3 << 20) + 1300
            })
        );
    }

    #[test]
    fn a_folder_asked_for_gives_only_what_is_below_it_whatever_the_case() {
        let got = run(&table(), "c:\\users");
        let d = dirs(&got);
        assert_eq!(d.len(), 2);
        assert!(d.contains_key("") && d.contains_key("me"));
        assert_eq!(
            got.last(),
            Some(&Message::Done {
                files: 3,
                bytes: (3 << 20) + 1300
            })
        );
    }

    #[test]
    fn a_name_that_only_begins_like_the_folder_is_not_inside_it() {
        let mut v = table();
        v.push(dir(5, "Users2")); // 30
        v.push(file(30, "x.bin", 50)); // 31
        let d = dirs(&run(&v, "C:\\Users"));
        assert!(!d.keys().any(|p| p.contains("Users2")));
    }

    #[test]
    fn big_files_carry_their_path_folder_size_and_time() {
        let got = run(&table(), "C:\\");
        let big: Vec<_> = got
            .iter()
            .filter_map(|m| match m {
                Message::Big {
                    path,
                    dir,
                    size,
                    mtime,
                } => Some((path.clone(), dir.clone(), *size, *mtime)),
                _ => None,
            })
            .collect();
        assert_eq!(
            big,
            vec![(
                r"Users\b.mp4".to_owned(),
                "Users".to_owned(),
                3 << 20,
                1_700_000_000
            )]
        );
    }

    #[test]
    fn files_are_sorted_into_categories_by_name() {
        let got = run(&table(), "C:\\");
        let cats = got.iter().find_map(|m| match m {
            Message::Dir { path, cats, .. } if path == r"Users\me" => Some(*cats),
            _ => None,
        });
        let cats = cats.unwrap();
        assert_eq!(cats[1], 500); // Images
        assert_eq!(cats[0], 800); // Documents
    }

    #[test]
    fn deleted_entries_links_and_ntfs_system_files_are_left_out() {
        let mut v = table();
        v.push(Spec {
            in_use: false,
            ..file(5, "deleted.bin", 4000)
        });
        v.push(Spec {
            reparse: Some(0xA000_000C),
            ..file(5, "link", 4000)
        });
        v.push(Spec {
            reparse: Some(0xA000_0003),
            ..dir(5, "junction")
        });
        // A table file with a user-looking name: still not counted.
        v[3] = file(5, "$Volume", 77);
        let d = dirs(&run(&v, "C:\\"));
        assert_eq!(d[""], (100, 1));
        assert!(!d.contains_key("junction"));
    }

    #[test]
    fn a_folder_whose_parent_is_gone_is_dropped_with_its_files() {
        let mut v = table();
        v.push(dir(900, "orphan")); // 30, parent not in the table
        v.push(file(30, "lost.bin", 600)); // 31
        let got = run(&v, "C:\\");
        assert!(!dirs(&got).keys().any(|p| p.contains("orphan")));
        assert_eq!(
            got.last(),
            Some(&Message::Done {
                files: 4,
                bytes: 100 + (3 << 20) + 1300
            })
        );
    }

    #[test]
    fn a_folder_loop_does_not_hang() {
        let mut v = table();
        v.push(dir(31, "a")); // 30 -> 31
        v.push(dir(30, "b")); // 31 -> 30
        let got = run(&v, "C:\\");
        assert!(matches!(got.last(), Some(Message::Done { .. })));
    }

    #[test]
    fn a_file_whose_size_is_in_an_extension_record_gets_it() {
        let mut v = table();
        v.push(Spec {
            list: true,
            size: None,
            ..file(5, "frag.iso", 0)
        }); // 30
        v.push(Spec {
            in_use: true,
            base: 30,
            non_resident: true,
            size: Some(2 << 20),
            ..Spec::default()
        }); // 31
        let got = run(&v, "C:\\");
        assert_eq!(dirs(&got)[""], (100 + (2 << 20), 2));
        assert!(
            got.iter()
                .any(|m| matches!(m, Message::Big { path, .. } if path == "frag.iso"))
        );
    }

    #[test]
    fn a_hard_linked_file_is_counted_once_under_its_long_name() {
        let mut v = table();
        v.push(Spec {
            namespace: 2,
            extra_names: vec![(24, "Real Name.txt", 1)],
            ..file(5, "REALNA~1.TXT", 50)
        });
        let d = dirs(&run(&v, "C:\\"));
        assert_eq!(d[""], (100, 1));
        assert_eq!(d["Users"], ((3 << 20) + 50, 2));
    }

    #[test]
    fn nobody_listening_stops_the_read() {
        let mut img = image(&table());
        let ok = summarize(&mut img, "C:\\", "C:\\", &mut || false, &mut |_| false).unwrap();
        assert!(!ok);
    }

    #[test]
    fn messages_survive_the_pipe_as_json_lines() {
        let m = Message::Dir {
            path: "C:\\a b".into(),
            own: 5,
            files: 2,
            cats: [1, 0, 0, 0, 0, 0, 4],
        };
        let line = serde_json::to_string(&m).unwrap();
        assert!(!line.contains('\n'));
        assert_eq!(serde_json::from_str::<Message>(&line).unwrap(), m);
        let done = serde_json::to_string(&Message::Done { files: 1, bytes: 2 }).unwrap();
        assert_eq!(done, r#"{"t":"done","files":1,"bytes":2}"#);
    }

    /// Needs an administrator shell: `cargo test --lib real_drive -- --ignored`. Makes a few
    /// files under the temp folder and reads them back from the drive's own file table.
    #[cfg(windows)]
    #[test]
    #[ignore = "needs an administrator shell"]
    fn real_drive_gives_the_sizes_of_files_just_made() {
        let tmp = tempfile::tempdir().unwrap();
        let canonical = tmp.path().canonicalize().unwrap();
        let root = canonical
            .to_string_lossy()
            .trim_start_matches(r"\\?\")
            .to_owned();
        std::fs::create_dir_all(format!(r"{root}\a\b")).unwrap();
        let sizes = [
            (r"\x.txt", 0usize),
            (r"\a\y.bin", 10),
            (r"\a\b\z.mp4", 5000),
            (r"\a\b\big.iso", 2 << 20),
        ];
        for (name, size) in sizes {
            std::fs::write(format!("{root}{name}"), vec![7u8; size]).unwrap();
        }
        let drive = &root[..2];
        let mut volume = std::fs::OpenOptions::new()
            .read(true)
            .open(format!(r"\\.\{drive}"))
            .expect("run this in an administrator shell");
        let mut got = Vec::new();
        let ok = summarize(
            &mut volume,
            &format!(r"{drive}\"),
            &root,
            &mut || false,
            &mut |m| {
                got.push(m);
                true
            },
        )
        .unwrap();
        assert!(ok);
        let total: usize = sizes.iter().map(|(_, s)| s).sum();
        assert_eq!(
            got.last(),
            Some(&Message::Done {
                files: 4,
                bytes: total as u64
            })
        );
        assert_eq!(dirs(&got).len(), 3);
    }
}
