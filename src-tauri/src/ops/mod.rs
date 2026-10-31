//! Mutating file operations. Deletes always go to the OS trash (ADR 0003).
//!
//! Conflicts are currently resolved as "keep both" (auto-renamed destination).
//! The interactive conflict dialog and the undo journal arrive in 0.2.

use std::fs;
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};

mod cross_volume;
mod noclobber;

pub fn trash(paths: &[PathBuf]) -> Result<(), trash::Error> {
    trash::delete_all(paths)
}

/// Renames `path` within its folder. Returns the new path.
pub fn rename(path: &Path, new_name: &str) -> io::Result<PathBuf> {
    validate_name(new_name)?;
    let dir = path
        .parent()
        .ok_or_else(|| io::Error::new(ErrorKind::InvalidInput, "Cannot rename a root"))?;
    let target = dir.join(new_name);
    if target == path {
        return Ok(target);
    }
    let renamed = match fs::symlink_metadata(&target) {
        Err(e) if e.kind() == ErrorKind::NotFound => noclobber::rename(path, &target),
        Err(e) => Err(e),
        Ok(_) if is_other_spelling(dir, path, new_name) => rename_in_place(dir, path, &target),
        Ok(_) => return Err(already_exists(new_name)),
    };
    renamed.map_err(|e| match e.kind() {
        // Taken after the check above.
        ErrorKind::AlreadyExists => already_exists(new_name),
        _ => e,
    })?;
    Ok(target)
}

/// True when `new_name` in `dir` resolves to `path` itself only because the volume
/// ignores letter case: a case-only rename. It isn't when the folder holds an entry
/// spelled exactly `new_name` (a different item, or a hard link to this one), or when
/// the identity can't be read; then the rename is refused.
fn is_other_spelling(dir: &Path, path: &Path, new_name: &str) -> bool {
    let spelled_exactly = fs::read_dir(dir).map_or(true, |entries| {
        entries
            .flatten()
            .any(|e| e.file_name() == std::ffi::OsStr::new(new_name))
    });
    !spelled_exactly && noclobber::same_file(path, &dir.join(new_name)).unwrap_or(false)
}

/// Gives `path` the name `target`, which already resolves to it. Some systems (macOS,
/// case-folding Linux) refuse a no-replace rename onto the item's own other spelling,
/// so that goes through a temporary name, without ever replacing anything.
fn rename_in_place(dir: &Path, path: &Path, target: &Path) -> io::Result<()> {
    match noclobber::rename(path, target) {
        Err(e) if e.kind() == ErrorKind::AlreadyExists => {}
        result => return result,
    }
    // Named after the item, so it can still be recognised if it's ever left behind,
    // unless that would pass the usual 255-character name limit.
    let named = format!(".{}.folio-rename", file_name(target)?);
    let temp_name = if named.len() <= 255 {
        named
    } else {
        ".folio-rename".to_owned()
    };
    let temp = noclobber::place(dir, &temp_name, |t| noclobber::rename(path, t))?;
    let Err(e) = noclobber::rename(&temp, target) else {
        return Ok(());
    };
    // Put the item back under its old name; if even that fails, say where it is.
    noclobber::rename(&temp, path).map_err(|_| {
        // Not `e.kind()`: `rename` would turn AlreadyExists into a message that hides
        // where the item went.
        io::Error::other(format!("{e}. The item is now named \"{}\"", temp.display()))
    })?;
    Err(e)
}

fn already_exists(name: &str) -> io::Error {
    io::Error::new(
        ErrorKind::AlreadyExists,
        format!("\"{name}\" already exists"),
    )
}

/// Creates a new folder with a unique name based on `name` inside `parent`.
pub fn create_folder(parent: &Path, name: &str) -> io::Result<PathBuf> {
    validate_name(name)?;
    noclobber::place(parent, name, |t| fs::create_dir(t))
}

/// Copies each source into `dest_dir`. Returns the created paths.
pub fn copy_into(sources: &[PathBuf], dest_dir: &Path) -> io::Result<Vec<PathBuf>> {
    let mut created = Vec::with_capacity(sources.len());
    for src in sources {
        ensure_not_into_itself(src, dest_dir)?;
        created.push(copy_new(src, dest_dir)?);
    }
    Ok(created)
}

/// The result of [`move_into`].
#[derive(Debug, Default)]
pub struct Moved {
    /// Where each source ended up.
    pub paths: Vec<PathBuf>,
    /// Source paths a cross-volume move had to leave: they changed during the move, or
    /// couldn't be removed. Each one still has a complete copy.
    pub left_behind: Vec<PathBuf>,
}

/// Moves each source into `dest_dir`. Items already in `dest_dir` are left untouched.
///
/// # Errors
/// Stops at the first source that fails. The error also mentions anything earlier
/// sources had to leave behind, so it isn't lost.
pub fn move_into(sources: &[PathBuf], dest_dir: &Path) -> io::Result<Moved> {
    let mut moved = Moved::default();
    for src in sources {
        if let Err(e) = move_one(src, dest_dir, &mut moved) {
            return Err(match moved.left_behind.len() {
                0 => e,
                n => io::Error::new(
                    e.kind(),
                    format!("{e}. Earlier, {n} item(s) stayed in the original folder"),
                ),
            });
        }
    }
    Ok(moved)
}

fn move_one(src: &Path, dest_dir: &Path, moved: &mut Moved) -> io::Result<()> {
    if src.parent() == Some(dest_dir) {
        moved.paths.push(src.to_path_buf());
        return Ok(());
    }
    ensure_not_into_itself(src, dest_dir)?;
    let name = file_name(src)?;
    let target = match noclobber::place(dest_dir, &name, |t| noclobber::rename(src, t)) {
        Ok(target) => target,
        Err(e) if is_cross_device(&e) => {
            let (target, left) = cross_volume::move_by_copy(src, dest_dir, &name)?;
            moved.left_behind.extend(left);
            target
        }
        Err(e) => return Err(e),
    };
    moved.paths.push(target);
    Ok(())
}

/// Returns `dir/name`, or `dir/name (2).ext`, `(3)`… if it already exists. The name can
/// be taken before it's used, so writers go through [`noclobber::place`].
pub fn unique_destination(dir: &Path, name: &str) -> PathBuf {
    // Not `exists()`: it follows links, so a dangling link would look free.
    let free = |p: &Path| fs::symlink_metadata(p).is_err();
    let candidate = dir.join(name);
    if free(&candidate) {
        return candidate;
    }
    let (stem, ext) = split_name(name);
    (2..=u64::MAX)
        .map(|n| dir.join(format!("{stem} ({n}){ext}")))
        .find(|p| free(p))
        .expect("a folder cannot hold u64::MAX entries")
}

fn split_name(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    }
}

fn validate_name(name: &str) -> io::Result<()> {
    let invalid = name.trim().is_empty()
        || name == "."
        || name == ".."
        || name.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|'])
        || (cfg!(windows) && !is_valid_on_windows(name));
    if invalid {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!("\"{name}\" is not a valid name"),
        ));
    }
    Ok(())
}

/// Windows drops a trailing dot or space and maps device names (`CON`, `nul.txt`) to
/// devices, so such a name would silently mean something else.
fn is_valid_on_windows(name: &str) -> bool {
    const DEVICES: [&str; 4] = ["CON", "PRN", "AUX", "NUL"];
    let stem = name.split('.').next().unwrap_or(name).trim_end();
    let numbered = |prefix: &str| {
        stem.len() == 4
            && stem[..3].eq_ignore_ascii_case(prefix)
            && stem.as_bytes()[3].is_ascii_digit()
            && stem.as_bytes()[3] != b'0'
    };
    let device =
        DEVICES.iter().any(|d| stem.eq_ignore_ascii_case(d)) || numbered("COM") || numbered("LPT");
    !(device || name.ends_with(['.', ' ']))
}

fn file_name(path: &Path) -> io::Result<String> {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .ok_or_else(|| io::Error::new(ErrorKind::InvalidInput, "Path has no file name"))
}

fn ensure_not_into_itself(src: &Path, dest_dir: &Path) -> io::Result<()> {
    if src.is_dir() && dest_dir.starts_with(src) {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "Cannot copy or move a folder into itself",
        ));
    }
    Ok(())
}

/// Copies `src` into `dest_dir` under its own name, or the next free one. The name is
/// claimed first (the folder, or the file itself), so nothing is written to a name
/// someone else took meanwhile.
fn copy_new(src: &Path, dest_dir: &Path) -> io::Result<PathBuf> {
    let name = file_name(src)?;
    if src.is_dir() {
        let target = noclobber::place(dest_dir, &name, |t| fs::create_dir(t))?;
        copy_contents(src, &target)?;
        Ok(target)
    } else {
        noclobber::place(dest_dir, &name, |t| noclobber::copy_file(src, t))
    }
}

fn copy_contents(src: &Path, dst: &Path) -> io::Result<()> {
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        copy_recursive(&entry.path(), &dst.join(entry.file_name()))?;
    }
    Ok(())
}

fn copy_recursive(src: &Path, dst: &Path) -> io::Result<()> {
    if src.is_dir() {
        fs::create_dir(dst)?;
        copy_contents(src, dst)
    } else {
        noclobber::copy_file(src, dst)
    }
}

fn is_cross_device(e: &io::Error) -> bool {
    // ERROR_NOT_SAME_DEVICE on Windows, EXDEV on Unix.
    e.kind() == ErrorKind::CrossesDevices
        || matches!(e.raw_os_error(), Some(17) if cfg!(windows))
        || matches!(e.raw_os_error(), Some(18) if cfg!(unix))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn unique_destination_appends_counter_before_extension() {
        let dir = tmp();
        fs::write(dir.path().join("a.txt"), "").unwrap();
        fs::write(dir.path().join("a (2).txt"), "").unwrap();
        assert_eq!(
            unique_destination(dir.path(), "a.txt"),
            dir.path().join("a (3).txt")
        );
        assert_eq!(
            unique_destination(dir.path(), "b.txt"),
            dir.path().join("b.txt")
        );
    }

    #[test]
    fn unique_destination_handles_folders_and_dotfiles() {
        let dir = tmp();
        fs::create_dir(dir.path().join("New folder")).unwrap();
        fs::write(dir.path().join(".env"), "").unwrap();
        assert_eq!(
            unique_destination(dir.path(), "New folder"),
            dir.path().join("New folder (2)")
        );
        assert_eq!(
            unique_destination(dir.path(), ".env"),
            dir.path().join(".env (2)")
        );
    }

    #[test]
    fn rename_rejects_existing_and_invalid_names() {
        let dir = tmp();
        let a = dir.path().join("a.txt");
        fs::write(&a, "").unwrap();
        fs::write(dir.path().join("b.txt"), "").unwrap();
        assert!(rename(&a, "b.txt").is_err());
        assert!(rename(&a, "x/y").is_err());
        assert!(rename(&a, "  ").is_err());
        let c = rename(&a, "c.txt").unwrap();
        assert!(c.exists() && !a.exists());
    }

    #[test]
    fn rename_allows_case_only_change() {
        let dir = tmp();
        let a = dir.path().join("readme.md");
        fs::write(&a, "").unwrap();
        let r = rename(&a, "README.md").unwrap();
        assert_eq!(r.file_name().unwrap(), "README.md");
    }

    #[test]
    fn create_folder_picks_unique_name() {
        let dir = tmp();
        let first = create_folder(dir.path(), "New folder").unwrap();
        let second = create_folder(dir.path(), "New folder").unwrap();
        assert!(first.is_dir() && second.is_dir());
        assert_eq!(second, dir.path().join("New folder (2)"));
    }

    #[test]
    fn copy_into_copies_trees_and_keeps_both_on_conflict() {
        let dir = tmp();
        let src = dir.path().join("src");
        fs::create_dir_all(src.join("nested")).unwrap();
        fs::write(src.join("nested").join("f.txt"), "data").unwrap();
        let dest = dir.path().join("dest");
        fs::create_dir(&dest).unwrap();

        let first = copy_into(std::slice::from_ref(&src), &dest).unwrap();
        let second = copy_into(std::slice::from_ref(&src), &dest).unwrap();
        assert_eq!(first[0], dest.join("src"));
        assert_eq!(second[0], dest.join("src (2)"));
        assert_eq!(
            fs::read_to_string(dest.join("src (2)").join("nested").join("f.txt")).unwrap(),
            "data"
        );
        assert!(src.exists());
    }

    #[test]
    fn copy_into_refuses_folder_into_itself() {
        let dir = tmp();
        let src = dir.path().join("src");
        fs::create_dir_all(src.join("inner")).unwrap();
        assert!(copy_into(std::slice::from_ref(&src), &src.join("inner")).is_err());
    }

    #[test]
    fn move_into_moves_and_skips_same_folder() {
        let dir = tmp();
        let f = dir.path().join("f.txt");
        fs::write(&f, "x").unwrap();
        let dest = dir.path().join("dest");
        fs::create_dir(&dest).unwrap();

        let same = move_into(std::slice::from_ref(&f), dir.path())
            .unwrap()
            .paths;
        assert_eq!(same[0], f);
        assert!(f.exists());

        let moved = move_into(std::slice::from_ref(&f), &dest).unwrap().paths;
        assert_eq!(moved[0], dest.join("f.txt"));
        assert!(!f.exists() && moved[0].exists());
    }
}

#[cfg(test)]
mod edge_tests;
