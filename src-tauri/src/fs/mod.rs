//! Directory listing and file metadata.

use serde::ser::{Serialize, SerializeTuple, Serializer};
use std::fs;
use std::io;
use std::path::Path;
use std::time::UNIX_EPOCH;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub is_dir: bool,
    /// Size in bytes; 0 for directories (folder sizes come from the scanner later).
    pub size: u64,
    /// Last modification time in milliseconds since the Unix epoch.
    pub modified: Option<u64>,
    pub hidden: bool,
}

const FLAG_DIR: u8 = 1;
const FLAG_HIDDEN: u8 = 2;

/// Serialized as a compact tuple `[name, flags, size, modified]` to keep large
/// listings small over IPC (decoded in src/lib/api/fs.ts). The UI rebuilds paths
/// from the folder path, so they are not sent per entry.
impl Serialize for Entry {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let flags =
            if self.is_dir { FLAG_DIR } else { 0 } | if self.hidden { FLAG_HIDDEN } else { 0 };
        let mut t = s.serialize_tuple(4)?;
        t.serialize_element(&self.name)?;
        t.serialize_element(&flags)?;
        t.serialize_element(&self.size)?;
        t.serialize_element(&self.modified)?;
        t.end()
    }
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

    // Lowercase each name once instead of on every comparison.
    entries.sort_by_cached_key(|e| (!e.is_dir, e.name.to_lowercase()));
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

    #[test]
    fn serializes_as_compact_tuple() {
        let e = Entry {
            name: "a.txt".into(),
            is_dir: false,
            size: 5,
            modified: Some(7),
            hidden: true,
        };
        assert_eq!(serde_json::to_string(&e).unwrap(), r#"["a.txt",2,5,7]"#);
    }
}
