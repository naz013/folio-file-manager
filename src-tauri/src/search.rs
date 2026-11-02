//! Searching a folder and everything below it by name and size. Matches are handed over in
//! batches while the walk goes on, and the walk stops as soon as it is cancelled.

use crate::fs::{self, Entry};
use serde::Serialize;
use std::fs as stdfs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// A search stops after this many matches: more than a list can usefully show.
pub const MAX_HITS: usize = 5_000;

/// Matches are sent when this many are ready, or when [`FLUSH_EVERY`] has passed.
const BATCH: usize = 64;
const FLUSH_EVERY: Duration = Duration::from_millis(80);

/// What to look for. Empty `text` matches every name, so a size range can be searched alone.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Query {
    /// Part of a name, or a pattern with `*` (any run of characters) and `?` (any one
    /// character) that has to match the whole name. Case does not matter.
    pub text: String,
    /// Files smaller than this are left out. Folders have no size, so a size limit leaves
    /// them out too.
    pub min_size: Option<u64>,
    pub max_size: Option<u64>,
    pub hidden: bool,
}

/// One match, with what a listing row needs.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Hit {
    pub path: String,
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified: Option<u64>,
    pub hidden: bool,
}

/// How a search ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Outcome {
    /// Items looked at, matching or not.
    pub scanned: u64,
    pub cancelled: bool,
    /// Stopped at [`MAX_HITS`]: there are more matches than were sent.
    pub truncated: bool,
}

/// Whether `name` (already lower case) matches the lower-case `pattern` as a whole.
fn wildcard(pattern: &[char], name: &[char]) -> bool {
    let (mut p, mut n) = (0, 0);
    // Where the last `*` was, and how much of the name it has taken so far.
    let mut star: Option<(usize, usize)> = None;
    while n < name.len() {
        match pattern.get(p) {
            Some('*') => {
                star = Some((p, n));
                p += 1;
            }
            Some(&c) if c == '?' || c == name[n] => {
                p += 1;
                n += 1;
            }
            _ => match star {
                Some((sp, sn)) => {
                    star = Some((sp, sn + 1));
                    p = sp + 1;
                    n = sn + 1;
                }
                None => return false,
            },
        }
    }
    pattern[p..].iter().all(|&c| c == '*')
}

/// A query ready to test names against.
struct Matcher {
    text: Vec<char>,
    plain: String,
    pattern: bool,
    min_size: Option<u64>,
    max_size: Option<u64>,
}

impl Matcher {
    fn new(q: &Query) -> Self {
        let plain = q.text.trim().to_lowercase();
        Self {
            text: plain.chars().collect(),
            pattern: plain.contains(['*', '?']),
            plain,
            min_size: q.min_size,
            max_size: q.max_size,
        }
    }

    fn name_fits(&self, name: &str) -> bool {
        if self.plain.is_empty() {
            return true;
        }
        let lower = name.to_lowercase();
        if self.pattern {
            wildcard(&self.text, &lower.chars().collect::<Vec<_>>())
        } else {
            lower.contains(&self.plain)
        }
    }

    fn needs_size(&self) -> bool {
        self.min_size.is_some() || self.max_size.is_some()
    }

    fn size_fits(&self, entry: &Entry) -> bool {
        if !self.needs_size() {
            return true;
        }
        !entry.is_dir
            && self.min_size.is_none_or(|m| entry.size >= m)
            && self.max_size.is_none_or(|m| entry.size <= m)
    }
}

/// Searches `root` and everything below it, handing matches to `send` in batches. Links are
/// never followed, so a loop can't trap the walk, and a folder that can't be read is skipped.
/// `send` returns false when nobody is listening any more, which stops the walk like a cancel.
pub fn search(
    root: &Path,
    query: &Query,
    cancel: &AtomicBool,
    send: &mut dyn FnMut(Vec<Hit>) -> bool,
) -> Outcome {
    let matcher = Matcher::new(query);
    let mut out = Outcome {
        scanned: 0,
        cancelled: false,
        truncated: false,
    };
    let mut found = 0_usize;
    let mut batch: Vec<Hit> = Vec::new();
    let mut last = Instant::now();
    let mut folders: Vec<PathBuf> = vec![root.to_path_buf()];

    'walk: while let Some(folder) = folders.pop() {
        let Ok(read) = stdfs::read_dir(&folder) else {
            continue;
        };
        for item in read {
            if cancel.load(Ordering::Relaxed) {
                out.cancelled = true;
                break 'walk;
            }
            let Ok(item) = item else { continue };
            out.scanned += 1;
            // Matches are rare in a big tree: don't hold one back until the next.
            if out.scanned.is_multiple_of(256) && !batch.is_empty() && last.elapsed() >= FLUSH_EVERY
            {
                if !send(std::mem::take(&mut batch)) {
                    out.cancelled = true;
                    break 'walk;
                }
                last = Instant::now();
            }
            let name = item.file_name().to_string_lossy().into_owned();
            let Ok(kind) = item.file_type() else { continue };
            let path = item.path();
            // Whether to descend decides what is needed from the disk: the hidden flag on
            // Windows and the size both come with the item's metadata.
            let is_dir = kind.is_dir();
            let named = matcher.name_fits(&name);
            if !is_dir && !named {
                continue;
            }
            let Ok(meta) = item.metadata() else { continue };
            let entry = fs::entry_from(name, &meta);
            if entry.hidden && !query.hidden {
                continue;
            }
            if is_dir {
                folders.push(path.clone());
            }
            if !named || !matcher.size_fits(&entry) {
                continue;
            }
            batch.push(Hit {
                path: path.to_string_lossy().into_owned(),
                name: entry.name,
                is_dir: entry.is_dir,
                size: entry.size,
                modified: entry.modified,
                hidden: entry.hidden,
            });
            found += 1;
            if found >= MAX_HITS {
                out.truncated = true;
                break 'walk;
            }
            if batch.len() >= BATCH || last.elapsed() >= FLUSH_EVERY {
                if !send(std::mem::take(&mut batch)) {
                    out.cancelled = true;
                    break 'walk;
                }
                last = Instant::now();
            }
        }
    }
    if !batch.is_empty() && !send(batch) {
        out.cancelled = true;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{create_dir_all, write};

    fn run(root: &Path, query: &Query) -> (Vec<String>, Outcome) {
        let mut names = Vec::new();
        let out = search(root, query, &AtomicBool::new(false), &mut |hits| {
            names.extend(hits.into_iter().map(|h| h.name));
            true
        });
        names.sort();
        (names, out)
    }

    fn q(text: &str) -> Query {
        Query {
            text: text.into(),
            ..Query::default()
        }
    }

    fn tree() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        create_dir_all(tmp.path().join("a/b")).unwrap();
        write(tmp.path().join("Report.TXT"), "x").unwrap();
        write(tmp.path().join("a/report-2.txt"), "12345").unwrap();
        write(tmp.path().join("a/b/notes.md"), "").unwrap();
        write(tmp.path().join("a/b/big.bin"), vec![0_u8; 2000]).unwrap();
        tmp
    }

    #[test]
    fn wildcards_match_the_whole_name() {
        let w = |p: &str, n: &str| {
            wildcard(
                &p.chars().collect::<Vec<_>>(),
                &n.chars().collect::<Vec<_>>(),
            )
        };
        assert!(w("*.txt", "a.txt"));
        assert!(w("a*c*e", "abcde"));
        assert!(w("a?c", "abc"));
        assert!(w("*", ""));
        assert!(!w("a?c", "ac"));
        assert!(!w("*.txt", "a.txt.bak"));
        assert!(!w("a*b", "abc"));
    }

    #[test]
    fn finds_names_in_subfolders_ignoring_case() {
        let tmp = tree();
        let (names, out) = run(tmp.path(), &q("REPORT"));
        assert_eq!(names, ["Report.TXT", "report-2.txt"]);
        assert!(!out.cancelled && !out.truncated);
    }

    #[test]
    fn a_pattern_matches_the_whole_name() {
        let tmp = tree();
        assert_eq!(run(tmp.path(), &q("*.md")).0, ["notes.md"]);
        assert_eq!(run(tmp.path(), &q("report")).0.len(), 2);
        assert_eq!(run(tmp.path(), &q("report?")).0, Vec::<String>::new());
    }

    #[test]
    fn folders_match_by_name() {
        let tmp = tree();
        assert_eq!(run(tmp.path(), &q("b")).0, ["b", "big.bin"]);
    }

    #[test]
    fn size_limits_leave_out_folders_and_other_sizes() {
        let tmp = tree();
        let query = Query {
            min_size: Some(3),
            max_size: Some(1000),
            ..Query::default()
        };
        assert_eq!(run(tmp.path(), &query).0, ["report-2.txt"]);
        let big = Query {
            min_size: Some(1000),
            ..Query::default()
        };
        assert_eq!(run(tmp.path(), &big).0, ["big.bin"]);
    }

    #[test]
    fn hidden_items_and_what_is_below_them_are_left_out_unless_asked_for() {
        let tmp = tree();
        create_dir_all(tmp.path().join(".git")).unwrap();
        write(tmp.path().join(".git/report-3.txt"), "").unwrap();
        write(tmp.path().join(".report-4.txt"), "").unwrap();
        assert_eq!(run(tmp.path(), &q("report")).0.len(), 2);
        let all = Query {
            hidden: true,
            ..q("report")
        };
        assert_eq!(run(tmp.path(), &all).0.len(), 4);
    }

    #[test]
    fn a_cancelled_search_stops_and_says_so() {
        let tmp = tree();
        let cancel = AtomicBool::new(true);
        let mut sent = 0;
        let out = search(tmp.path(), &q(""), &cancel, &mut |h| {
            sent += h.len();
            true
        });
        assert!(out.cancelled);
        assert_eq!(sent, 0);
    }

    #[test]
    fn a_closed_listener_stops_the_walk() {
        let tmp = tempfile::tempdir().unwrap();
        for i in 0..200 {
            write(tmp.path().join(format!("f{i}.txt")), "").unwrap();
        }
        let mut calls = 0;
        let out = search(tmp.path(), &q("f"), &AtomicBool::new(false), &mut |_| {
            calls += 1;
            false
        });
        assert!(out.cancelled);
        assert_eq!(calls, 1);
    }

    #[test]
    fn results_are_capped() {
        let tmp = tempfile::tempdir().unwrap();
        for i in 0..(MAX_HITS + 10) {
            write(tmp.path().join(format!("f{i}")), "").unwrap();
        }
        let (names, out) = run(tmp.path(), &q("f"));
        assert_eq!(names.len(), MAX_HITS);
        assert!(out.truncated);
    }

    #[cfg(unix)]
    #[test]
    fn links_are_not_followed() {
        let tmp = tree();
        std::os::unix::fs::symlink(tmp.path(), tmp.path().join("a/loop")).unwrap();
        let (names, _) = run(tmp.path(), &q("notes"));
        assert_eq!(names, ["notes.md"]);
    }
}
