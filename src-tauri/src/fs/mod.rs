//! Directory listing and file metadata.

use serde::Serialize;
use std::fs;
use std::io;
use std::path::Path;
use std::time::UNIX_EPOCH;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    /// Size in bytes; 0 for directories (folder sizes come from the scanner later).
    pub size: u64,
    /// Last modification time in milliseconds since the Unix epoch.
    pub modified: Option<u64>,
    pub hidden: bool,
}

/// Lists a directory, folders first, then case-insensitive by name.
///
/// Entries whose metadata cannot be read (e.g. broken links, permission errors)
/// are skipped rather than failing the whole listing.
pub fn list_dir(dir: &Path) -> io::Result<Vec<Entry>> {
    let mut entries: Vec<Entry> = fs::read_dir(dir)?
        .filter_map(Result::ok)
        .filter_map(|e| {
            let meta = e.metadata().ok()?;
            let name = e.file_name().to_string_lossy().into_owned();
            Some(Entry {
                hidden: is_hidden(&name, &meta),
                path: e.path().to_string_lossy().into_owned(),
                is_dir: meta.is_dir(),
                size: if meta.is_dir() { 0 } else { meta.len() },
                modified: meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|d| d.as_millis() as u64),
                name,
            })
        })
        .collect();

    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(entries)
}

#[cfg(windows)]
fn is_hidden(name: &str, meta: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
    name.starts_with('.') || meta.file_attributes() & FILE_ATTRIBUTE_HIDDEN != 0
}

#[cfg(not(windows))]
fn is_hidden(name: &str, _meta: &fs::Metadata) -> bool {
    name.starts_with('.')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_folders_first_then_by_name() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("b.txt"), "hello").unwrap();
        fs::write(tmp.path().join("A.txt"), "").unwrap();
        fs::create_dir(tmp.path().join("zeta")).unwrap();
        fs::write(tmp.path().join(".dotfile"), "").unwrap();

        let entries = list_dir(tmp.path()).unwrap();
        let names: Vec<_> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["zeta", ".dotfile", "A.txt", "b.txt"]);

        let b = entries.iter().find(|e| e.name == "b.txt").unwrap();
        assert_eq!(b.size, 5);
        assert!(!b.is_dir);
        assert!(b.modified.is_some());
        assert!(
            entries
                .iter()
                .find(|e| e.name == ".dotfile")
                .unwrap()
                .hidden
        );
    }

    #[test]
    fn missing_directory_is_an_error() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(list_dir(&tmp.path().join("nope")).is_err());
    }
}
