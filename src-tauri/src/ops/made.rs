//! Writing a file a tool made (a merged PDF, an edited picture). The bytes go to a
//! temporary name of the writer's own first and are only given their real name once all
//! of them are on disk, so a failure leaves nothing half-written at the real name, and
//! only a temporary file this module made is ever removed.

use std::fs;
use std::io::{self, ErrorKind, Write};
use std::path::{Path, PathBuf};

use super::PARTIAL;

/// Writes `bytes` to a new temporary file in `dir` named after `name`, and returns it.
pub fn stage(dir: &Path, name: &str, bytes: &[u8]) -> io::Result<PathBuf> {
    let (temp, mut file) = (0..100)
        .find_map(|n| {
            let temp = dir.join(format!("{name}.{}-{n}{PARTIAL}", std::process::id()));
            match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp)
            {
                Ok(file) => Some(Ok((temp, file))),
                Err(e) if e.kind() == ErrorKind::AlreadyExists => None,
                Err(e) => Some(Err(e)),
            }
        })
        .unwrap_or_else(|| Err(io::Error::from(ErrorKind::AlreadyExists)))?;
    if let Err(e) = file.write_all(bytes).and_then(|()| file.sync_all()) {
        drop(file);
        let _ = fs::remove_file(&temp);
        return Err(e);
    }
    Ok(temp)
}

/// Gives the staged `temp` its real name `dir/name` (or the next free `name (2).ext`…).
/// If that fails the complete file stays at `temp` and the error says where.
pub fn finish(temp: &Path, dir: &Path, name: &str) -> io::Result<PathBuf> {
    super::finish_copy(temp, dir, name)
}

/// Writes `bytes` as `dir/name` (or the next free name) and returns the path.
pub fn write_new(dir: &Path, name: &str, bytes: &[u8]) -> io::Result<PathBuf> {
    let temp = stage(dir, name, bytes)?;
    finish(&temp, dir, name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_partial_that_was_there_before_is_never_removed() {
        let dir = tempfile::tempdir().unwrap();
        // A finished copy left at a partial name (see `finish_copy`) is somebody else's file.
        let theirs = dir.path().join(format!("out.pdf{PARTIAL}"));
        fs::write(&theirs, "mine").unwrap();
        let made = write_new(dir.path(), "out.pdf", b"new").unwrap();
        assert_eq!(made, dir.path().join("out.pdf"));
        assert_eq!(fs::read(&theirs).unwrap(), b"mine");
        assert_eq!(fs::read(&made).unwrap(), b"new");
    }

    #[test]
    fn a_new_file_never_replaces_one_and_leaves_no_temporary_behind() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("out.pdf"), "mine").unwrap();
        let made = write_new(dir.path(), "out.pdf", b"new").unwrap();
        assert_eq!(made, dir.path().join("out (2).pdf"));
        assert_eq!(fs::read(dir.path().join("out.pdf")).unwrap(), b"mine");
        let names: Vec<String> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(names.iter().all(|n| !n.ends_with(PARTIAL)), "{names:?}");
    }
}
