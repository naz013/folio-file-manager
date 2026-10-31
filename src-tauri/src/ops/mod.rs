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

/// Adds to a failure in the middle of a batch which items were already done.
fn batch_error(e: io::Error, verb: &str, done: &[PathBuf]) -> io::Error {
    const SHOWN: usize = 5;
    if done.is_empty() {
        return e;
    }
    let mut names: Vec<_> = done
        .iter()
        .take(SHOWN)
        .filter_map(|p| p.file_name())
        .map(|n| format!("\"{}\"", n.to_string_lossy()))
        .collect();
    if done.len() > SHOWN {
        names.push(format!("and {} more", done.len() - SHOWN));
    }
    io::Error::new(
        e.kind(),
        format!("{e}. Already {verb}: {}", names.join(", ")),
    )
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
pub fn copy_into(sources: &[PathBuf], dest_dir: &Path) -> io::Result<Copied> {
    let mut copied = Copied::default();
    for src in sources {
        let copy = ensure_not_into_itself(src, dest_dir).and_then(|()| {
            let mut walk = Plain {
                made: Created::default(),
                skipped: &mut copied.skipped,
            };
            copy_tree(src, dest_dir, &file_name(src)?, &mut walk)
        });
        match copy {
            Ok(Some(target)) => copied.paths.push(target),
            Ok(None) => {}
            Err(e) => return Err(batch_error(e, "copied", &copied.paths)),
        }
    }
    Ok(copied)
}

/// The result of [`copy_into`].
#[derive(Debug, Default)]
pub struct Copied {
    /// The copy of each source.
    pub paths: Vec<PathBuf>,
    /// Links that weren't copied because they can't be recreated here.
    pub skipped: Vec<PathBuf>,
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
            let e = match moved.left_behind.len() {
                0 => e,
                n => io::Error::new(
                    e.kind(),
                    format!("{e}. Earlier, {n} item(s) stayed in the original folder"),
                ),
            };
            return Err(batch_error(e, "moved", &moved.paths));
        }
    }
    Ok(moved)
}

fn move_one(src: &Path, dest_dir: &Path, moved: &mut Moved) -> io::Result<()> {
    // Already there, however the destination is spelled or linked to.
    let already_there = src.parent().is_some_and(|p| {
        p == dest_dir
            || noclobber::same_file(p, dest_dir).unwrap_or(false)
            || matches!((fs::canonicalize(p), fs::canonicalize(dest_dir)), (Ok(a), Ok(b)) if a == b)
    });
    if already_there {
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
            match target {
                Some(target) => target,
                None => return Ok(()),
            }
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

/// Refuses to put a folder inside itself. Compares file identity, not path text, for
/// the destination and every folder above it once links, `..`, letter case and `\\?\`
/// prefixes are resolved.
fn ensure_not_into_itself(src: &Path, dest_dir: &Path) -> io::Result<()> {
    // A link is moved or recreated as itself, so it can't contain its destination.
    if !fs::symlink_metadata(src).is_ok_and(|m| m.is_dir()) {
        return Ok(());
    }
    let into_itself = || {
        io::Error::new(
            ErrorKind::InvalidInput,
            "Cannot copy or move a folder into itself",
        )
    };
    // Baseline that needs nothing from the file system, for volumes where neither ids
    // nor resolved paths are available.
    let src_text = comparable(src)?;
    if comparable(dest_dir)?.starts_with(&src_text) {
        return Err(into_itself());
    }
    let src_id = noclobber::file_id(src)?;
    // Some file system drivers can't resolve a final path; their ids still work.
    let mut dir = fs::canonicalize(dest_dir).or_else(|_| std::path::absolute(dest_dir))?;
    let mut src_real = None;
    loop {
        let same = match noclobber::file_id(&dir) {
            Ok(id) if id.1 != 0 && src_id.1 != 0 => id == src_id,
            // Some network redirectors report 0 for every id: compare paths instead.
            _ => {
                let real = src_real.get_or_insert_with(|| fs::canonicalize(src).ok());
                real.as_deref() == Some(dir.as_path()) || comparable(&dir)? == src_text
            }
        };
        if same {
            return Err(into_itself());
        }
        if !dir.pop() {
            return Ok(());
        }
    }
}

/// `path` made absolute and, on Windows, without a `\\?\` prefix and in lower case, so
/// two spellings of one path compare equal as text.
fn comparable(path: &Path) -> io::Result<PathBuf> {
    let full = std::path::absolute(path)?;
    if !cfg!(windows) {
        return Ok(full);
    }
    let text = full.to_string_lossy();
    let text = match text.strip_prefix(r"\\?\UNC\") {
        Some(unc) => format!(r"\\{unc}"),
        None => text.strip_prefix(r"\\?\").unwrap_or(&text).to_owned(),
    };
    Ok(PathBuf::from(text.to_lowercase()))
}

/// Suffix of a copy in progress. A copy only gets its real name once it's complete, so
/// a failed or interrupted one never looks like a finished copy.
const PARTIAL: &str = ".folio-partial";

/// What a tree copy reports to its caller. The walk itself lives in [`copy_tree`]; a
/// caller that needs more than the copy (a move between drives records what it copied)
/// adds to the hooks, which do nothing by default.
trait Observe {
    /// What the copy has created so far, for removing it if the copy fails.
    fn made(&mut self) -> &mut Created;

    /// Links that couldn't be recreated here.
    fn skipped(&mut self) -> &mut Vec<PathBuf>;

    /// Copies one file without replacing `dst`; nothing is recorded if that fails.
    fn copy_file(&mut self, src: &Path, dst: &Path) -> io::Result<()> {
        noclobber::copy_file(src, dst)?;
        self.made().file(dst)
    }

    /// A link was recreated; `target` is what it points at.
    fn link(&mut self, _src: &Path, _target: PathBuf) {}

    /// Everything inside the folder `src` was copied.
    fn folder(&mut self, _src: &Path) {}

    /// The copy is complete at `temp`, just before it is renamed.
    fn built(&mut self, _temp: &Path) {}

    /// The copy at `temp` now has its final name `target`.
    fn renamed(&mut self, _temp: &Path, _target: &Path) {}
}

/// A plain copy: it only needs to be undone on failure and to report skipped links.
struct Plain<'a> {
    made: Created,
    skipped: &'a mut Vec<PathBuf>,
}

impl Observe for Plain<'_> {
    fn made(&mut self) -> &mut Created {
        &mut self.made
    }

    fn skipped(&mut self) -> &mut Vec<PathBuf> {
        self.skipped
    }
}

/// Copies `src` into `dest_dir` as `name`, or the next free name. The copy is built
/// under a temporary `name.folio-partial` and renamed, without replacing anything,
/// once complete; a failure removes what it created. Returns `None` for a link that
/// can't be recreated.
fn copy_tree(
    src: &Path,
    dest_dir: &Path,
    name: &str,
    obs: &mut dyn Observe,
) -> io::Result<Option<PathBuf>> {
    let kind = fs::symlink_metadata(src)?.file_type();
    if kind.is_symlink() {
        // A single link is created in one step: nothing partial to hide.
        let target = fs::read_link(src)?;
        let placed = noclobber::place(dest_dir, name, |t| copy_link(&target, t));
        let placed = skip_unsupported(placed, src, obs.skipped())?;
        if placed.is_some() {
            obs.link(src, target);
        }
        return Ok(placed);
    }
    let partial = partial_name(name);
    let temp = if kind.is_dir() {
        noclobber::place(dest_dir, &partial, |t| fs::create_dir(t)).and_then(|temp| {
            obs.made().dir(&temp);
            copy_contents(src, &temp, obs).map(|()| temp)
        })
    } else {
        noclobber::place(dest_dir, &partial, |t| obs.copy_file(src, t))
    };
    let temp = match temp {
        Ok(temp) => temp,
        Err(e) => return Err(std::mem::take(obs.made()).fail(e)),
    };
    obs.built(&temp);
    let target = finish_copy(&temp, dest_dir, name)?;
    obs.renamed(&temp, &target);
    Ok(Some(target))
}

/// The temporary name for a copy of `name`: `name.folio-partial`, or just
/// `.folio-partial` where that would pass the usual 255-character name limit.
fn partial_name(name: &str) -> String {
    let named = format!("{name}{PARTIAL}");
    // Room for the " (2)" a taken name gets.
    if named.len() <= 240 {
        named
    } else {
        PARTIAL.to_owned()
    }
}

/// Gives the finished copy at `temp` its real name in `dest_dir`, never replacing
/// anything. A rename blocked by a program holding a file inside (an indexer or
/// antivirus) is retried; if it still fails, the complete copy stays at `temp` and the
/// error says where, rather than throwing the finished work away.
///
/// On Unix file systems without a no-replace rename the final step falls back to
/// check-then-rename and can lose a race (see `noclobber::rename`).
fn finish_copy(temp: &Path, dest_dir: &Path, name: &str) -> io::Result<PathBuf> {
    let mut wait = std::time::Duration::from_millis(50);
    let mut retries = 5;
    loop {
        match noclobber::place(dest_dir, name, |t| noclobber::rename(temp, t)) {
            Err(e) if is_held_open(&e) && retries > 0 => {
                retries -= 1;
                std::thread::sleep(wait);
                wait *= 2;
            }
            Err(e) => {
                // A rename can take effect and still report failure (a network share
                // losing the reply), so only point at the partial if it's there.
                let at = if fs::symlink_metadata(temp).is_ok() {
                    format!("The complete copy is at \"{}\"", temp.display())
                } else {
                    format!("The copy may already be in \"{}\"", dest_dir.display())
                };
                return Err(io::Error::new(e.kind(), format!("{e}. {at}")));
            }
            done => return done,
        }
    }
}

/// Another program has a file open in a way that blocks the rename for now.
fn is_held_open(e: &io::Error) -> bool {
    // ERROR_SHARING_VIOLATION isn't mapped to an `ErrorKind`.
    e.kind() == ErrorKind::PermissionDenied || (cfg!(windows) && e.raw_os_error() == Some(32))
}

fn copy_contents(src: &Path, dst: &Path, obs: &mut dyn Observe) -> io::Result<()> {
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        copy_recursive(&entry.path(), &dst.join(entry.file_name()), obs)?;
    }
    obs.folder(src);
    Ok(())
}

/// Copies a tree without following links, so a link to an ancestor can't loop and a
/// link to elsewhere can't pull in data from outside the selection.
fn copy_recursive(src: &Path, dst: &Path, obs: &mut dyn Observe) -> io::Result<()> {
    let kind = fs::symlink_metadata(src)?.file_type();
    if kind.is_symlink() {
        // Read once: the recorded target is what was recreated.
        let target = fs::read_link(src)?;
        let copied = copy_link(&target, dst);
        if skip_unsupported(copied, src, obs.skipped())?.is_some() {
            obs.made().file(dst)?;
            obs.link(src, target);
        }
        Ok(())
    } else if kind.is_dir() {
        fs::create_dir(dst)?;
        obs.made().dir(dst);
        copy_contents(src, dst, obs)
    } else {
        obs.copy_file(src, dst)
    }
}

/// What one copy created at the destination, so a failed copy can remove exactly that
/// and nothing someone else put there meanwhile. These are the operation's own new
/// files, not the user's data, so they are removed, not sent to the trash (ADR 0003).
#[derive(Default)]
struct Created {
    files: Vec<CreatedFile>,
    /// Folders, parents before children.
    dirs: Vec<PathBuf>,
}

impl Created {
    fn file(&mut self, path: &Path) -> io::Result<()> {
        let meta = fs::symlink_metadata(path)?;
        self.files.push(CreatedFile {
            path: path.to_path_buf(),
            id: noclobber::file_id(path)?,
            len: meta.len(),
            modified: meta.modified().ok(),
        });
        Ok(())
    }

    fn dir(&mut self, path: &Path) {
        self.dirs.push(path.to_path_buf());
    }

    /// Removes what was created, newest first, and returns `e`, naming what's left if
    /// something couldn't be removed (it held someone else's file, say).
    fn fail(self, e: io::Error) -> io::Error {
        let top = self
            .dirs
            .first()
            .or(self.files.first().map(|f| &f.path))
            .cloned();
        // Errors are ignored on purpose: whatever can't be removed stays, and the
        // error below names the copy so it can be found.
        for file in self.files.into_iter().rev() {
            if file.is_still_ours() {
                let _ = cross_volume::remove_even_read_only(&file.path, |p| fs::remove_file(p));
            }
        }
        for dir in self.dirs.into_iter().rev() {
            let _ = cross_volume::remove_even_read_only(&dir, |p| fs::remove_dir(p));
        }
        match top {
            Some(top) if fs::symlink_metadata(&top).is_ok() => io::Error::new(
                e.kind(),
                format!("{e}. Part of the copy remains at \"{}\"", top.display()),
            ),
            _ => e,
        }
    }
}

/// A file or link one copy created, and what it looked like then.
struct CreatedFile {
    path: PathBuf,
    id: (u64, u128),
    len: u64,
    modified: Option<std::time::SystemTime>,
}

impl CreatedFile {
    /// Still the file this copy created. Where file ids are unknown (0, on some
    /// network shares) size and modified time must match too.
    fn is_still_ours(&self) -> bool {
        let Ok(meta) = fs::symlink_metadata(&self.path) else {
            return false;
        };
        let same_id = noclobber::file_id(&self.path).is_ok_and(|now| now == self.id);
        let same_look = meta.len() == self.len && meta.modified().ok() == self.modified;
        same_id && (self.id.1 != 0 || same_look)
    }
}

/// Turns a link copy that this system or volume can't do into a skipped entry, so it
/// is reported instead of failing the whole operation. Only for link copies: any other
/// `Unsupported` error is a real failure.
fn skip_unsupported<T>(
    copied: io::Result<T>,
    src: &Path,
    skipped: &mut Vec<PathBuf>,
) -> io::Result<Option<T>> {
    match copied {
        Ok(done) => Ok(Some(done)),
        Err(e) if e.kind() == ErrorKind::Unsupported => {
            skipped.push(src.to_path_buf());
            Ok(None)
        }
        Err(e) => Err(e),
    }
}

/// Creates at `dst` a link to `target`, failing if `dst` exists. Reports `Unsupported`
/// where the volume can't hold links (FAT and exFAT refuse with EPERM).
#[cfg(unix)]
fn copy_link(target: &Path, dst: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(target, dst).map_err(|e| {
        let codes = [libc::EPERM, libc::ENOTSUP, libc::EOPNOTSUPP, libc::ENOSYS];
        if e.raw_os_error().is_some_and(|c| codes.contains(&c)) {
            io::Error::new(ErrorKind::Unsupported, e)
        } else {
            e
        }
    })
}

/// Windows can't recreate junctions through std, and symlinks need Developer Mode or
/// admin rights, so links are reported as `Unsupported` and skipped.
#[cfg(windows)]
fn copy_link(_target: &Path, _dst: &Path) -> io::Result<()> {
    Err(io::Error::new(
        ErrorKind::Unsupported,
        "Links can't be copied on Windows yet",
    ))
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

        let first = copy_into(std::slice::from_ref(&src), &dest).unwrap().paths;
        let second = copy_into(std::slice::from_ref(&src), &dest).unwrap().paths;
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
