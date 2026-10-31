//! Mutating file operations. Deletes always go to the OS trash (ADR 0003).
//!
//! Conflicts are currently resolved as "keep both" (auto-renamed destination).
//! The interactive conflict dialog and the undo journal arrive in 0.2.

use std::fs;
use std::io::{self, ErrorKind};
use std::path::{Component, Path, PathBuf};

mod cross_volume;
#[cfg(windows)]
mod links;
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
/// Adds to `e` that `n` links in earlier items could not be pointed at their new place.
fn with_dangling(e: io::Error, n: usize) -> io::Error {
    if n == 0 {
        return e;
    }
    io::Error::new(
        e.kind(),
        format!("{e}. Earlier, {n} link(s) couldn't be pointed at their new place"),
    )
}

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
    copy_into_with(sources, dest_dir, &mut copied)?;
    Ok(copied)
}

/// Like [`copy_into`], recording into `copied` so that what was done before an error is
/// still known to the caller.
pub fn copy_into_with(sources: &[PathBuf], dest_dir: &Path, copied: &mut Copied) -> io::Result<()> {
    for src in sources {
        let copy = ensure_not_into_itself(src, dest_dir).and_then(|()| {
            let mut walk = Plain {
                made: Created::default(),
                skipped: &mut copied.skipped,
                not_repointed: &mut copied.not_repointed,
            };
            copy_tree(src, dest_dir, &file_name(src)?, &mut walk)
        });
        match copy {
            Ok(Some(target)) => {
                copied.pairs.push((src.clone(), target.clone()));
                copied.paths.push(target);
            }
            Ok(None) => {}
            Err(e) => {
                let e = with_dangling(e, copied.not_repointed.len());
                return Err(batch_error(e, "copied", &copied.paths));
            }
        }
    }
    Ok(())
}

/// The result of [`copy_into`].
#[derive(Debug, Default)]
pub struct Copied {
    /// The copy of each source.
    pub paths: Vec<PathBuf>,
    /// Each source and where its copy went, for the undo journal.
    pub pairs: Vec<(PathBuf, PathBuf)>,
    /// Links that weren't copied because they can't be recreated here.
    pub skipped: Vec<PathBuf>,
    /// Links in the copies that lead back into the original folder, or are missing:
    /// they should point into the copy but could not be repointed.
    pub not_repointed: Vec<PathBuf>,
}

/// The result of [`move_into`].
#[derive(Debug, Default)]
pub struct Moved {
    /// Where each source ended up.
    pub paths: Vec<PathBuf>,
    /// Each source that was moved and where it went, for the undo journal. Sources that
    /// were already in the destination are not here.
    pub pairs: Vec<(PathBuf, PathBuf)>,
    /// Source paths a cross-volume move had to leave: they changed during the move, or
    /// couldn't be removed. Each one still has a complete copy.
    pub left_behind: Vec<PathBuf>,
    /// Links in a moved folder that should point into its new place but could not be
    /// repointed: they dangle, or are missing.
    pub not_repointed: Vec<PathBuf>,
}

/// Moves each source into `dest_dir`. Items already in `dest_dir` are left untouched.
///
/// # Errors
/// Stops at the first source that fails. The error also mentions anything earlier
/// sources had to leave behind, so it isn't lost.
#[cfg(test)]
pub fn move_into(sources: &[PathBuf], dest_dir: &Path) -> io::Result<Moved> {
    let mut moved = Moved::default();
    move_into_with(sources, dest_dir, &mut moved)?;
    Ok(moved)
}

/// Like [`move_into`], recording into `moved` so that what was done before an error is
/// still known to the caller.
pub fn move_into_with(sources: &[PathBuf], dest_dir: &Path, moved: &mut Moved) -> io::Result<()> {
    for src in sources {
        if let Err(e) = move_one(src, dest_dir, moved) {
            let e = match moved.left_behind.len() {
                0 => e,
                n => io::Error::new(
                    e.kind(),
                    format!("{e}. Earlier, {n} item(s) stayed in the original folder"),
                ),
            };
            let e = with_dangling(e, moved.not_repointed.len());
            return Err(batch_error(e, "moved", &moved.paths));
        }
    }
    Ok(())
}

/// The identity of the item at `path` as text (volume and file id), which stays the same
/// when it is renamed or moved on its drive. `None` where it can't be read.
pub fn identity(path: &Path) -> Option<String> {
    let (volume, file) = noclobber::file_id(path).ok()?;
    // Some network shares report 0 for every item: that tells nothing apart.
    if file == 0 {
        return None;
    }
    Some(format!("{volume:x}-{file:x}"))
}

/// After the folder `old` was moved to `new` by an undo or redo, points the links inside
/// it that led into `old` at the same places under `new`, as a move does (see
/// [`repoint_renamed_tree`]). Does nothing for a file.
pub fn repoint_after_move(old: &Path, new: &Path) {
    if fs::symlink_metadata(new).is_ok_and(|m| m.is_dir()) {
        repoint_renamed_tree(&[old.to_path_buf()], new, &mut Vec::new());
    }
}

/// Moves `src` to exactly `dst` on the same drive, failing if `dst` exists. For undoing
/// and redoing a move, where the name must be the one the item had.
pub fn move_exact(src: &Path, dst: &Path) -> io::Result<()> {
    noclobber::rename(src, dst).map_err(|e| {
        if is_cross_device(&e) {
            io::Error::new(
                ErrorKind::Unsupported,
                "it was moved to another drive, so Folio can't move it back; move it yourself",
            )
        } else {
            e
        }
    })
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
    let was_folder = fs::symlink_metadata(src).is_ok_and(|m| m.is_dir());
    // Links may name the folder by its real path when `src` is reached through a link;
    // that can't be resolved once it has moved.
    let mut old_names = vec![src.to_path_buf()];
    if was_folder {
        old_names.extend(fs::canonicalize(src).ok());
    }
    let target = match noclobber::place(dest_dir, &name, |t| noclobber::rename(src, t)) {
        Ok(target) => {
            if was_folder {
                repoint_renamed_tree(&old_names, &target, &mut moved.not_repointed);
            }
            target
        }
        Err(e) if is_cross_device(&e) => {
            let (target, left, dangling) = cross_volume::move_by_copy_noted(src, dest_dir, &name)?;
            moved.left_behind.extend(left);
            moved.not_repointed.extend(dangling);
            match target {
                Some(target) => target,
                None => return Ok(()),
            }
        }
        Err(e) => return Err(e),
    };
    moved.pairs.push((src.to_path_buf(), target.clone()));
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
pub(crate) fn comparable(path: &Path) -> io::Result<PathBuf> {
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

    /// A link in the finished copy that should point into the copy but could not be
    /// repointed: it still leads to the original, or is missing.
    fn not_repointed(&mut self, _at: PathBuf) {}
}

/// A plain copy: it only needs to be undone on failure and to report skipped links.
struct Plain<'a> {
    made: Created,
    skipped: &'a mut Vec<PathBuf>,
    not_repointed: &'a mut Vec<PathBuf>,
}

impl Observe for Plain<'_> {
    fn made(&mut self) -> &mut Created {
        &mut self.made
    }

    fn skipped(&mut self) -> &mut Vec<PathBuf> {
        self.skipped
    }

    fn not_repointed(&mut self, at: PathBuf) {
        self.not_repointed.push(at);
    }
}

/// Notes the links recreated in a copy whose target is inside the tree being copied, so
/// they can be pointed at the copy once it has its final name.
struct Retarget<'a> {
    inner: &'a mut dyn Observe,
    root: &'a Path,
    /// Links to repoint: where each was in the source, and where it points, relative to
    /// the root of the tree.
    inside: Vec<(PathBuf, PathBuf)>,
}

impl Observe for Retarget<'_> {
    fn made(&mut self) -> &mut Created {
        self.inner.made()
    }

    fn skipped(&mut self) -> &mut Vec<PathBuf> {
        self.inner.skipped()
    }

    fn copy_file(&mut self, src: &Path, dst: &Path) -> io::Result<()> {
        self.inner.copy_file(src, dst)
    }

    fn link(&mut self, src: &Path, target: PathBuf) {
        // The root itself, when it is a link, is copied as it is.
        if src != self.root
            && let Some(rest) = inside_tree(self.root, &target)
        {
            self.inside.push((src.to_path_buf(), rest));
        }
        self.inner.link(src, target);
    }

    fn folder(&mut self, src: &Path) {
        self.inner.folder(src);
    }

    fn built(&mut self, temp: &Path) {
        self.inner.built(temp);
    }

    fn renamed(&mut self, temp: &Path, target: &Path) {
        self.inner.renamed(temp, target);
    }

    fn not_repointed(&mut self, at: PathBuf) {
        self.inner.not_repointed(at);
    }
}

impl Retarget<'_> {
    /// Points each link that led into the source tree at the same place in the copy at
    /// `copy`. A link that cannot be repointed keeps its old target, which works only
    /// until the original is gone, and is reported.
    fn apply(&mut self, copy: &Path) {
        // Absolute, so a symlink's new target doesn't depend on the working directory.
        let copy = std::path::absolute(copy).unwrap_or_else(|_| copy.to_path_buf());
        for (from, rest) in std::mem::take(&mut self.inside) {
            let Ok(relative) = from.strip_prefix(self.root) else {
                // Every recorded link is below the root; reported rather than lost.
                self.inner.not_repointed(from);
                continue;
            };
            let at = copy.join(relative);
            if let Err(leftover) = repoint_link(&copy.join(&rest), &at) {
                self.inner.not_repointed(at);
                if let Some(temp) = leftover {
                    self.inner.not_repointed(temp);
                }
            }
        }
    }
}

/// Where the absolute `target` is inside the tree at `root`, relative to it. `None` for
/// a relative target (it moves with the tree), one outside the tree, or one with `..`.
fn inside_tree(root: &Path, target: &Path) -> Option<PathBuf> {
    if !target.is_absolute() || target.components().any(|c| c == Component::ParentDir) {
        return None;
    }
    let (root_text, target_text) = (comparable(root).ok()?, comparable(target).ok()?);
    if !target_text.starts_with(&root_text) {
        return None;
    }
    // Same spelling as the link had, not the lower-cased text compared above.
    let depth = target_text.components().count() - root_text.components().count();
    let full = std::path::absolute(target).ok()?;
    let parts: Vec<_> = full.components().collect();
    Some(parts[parts.len().checked_sub(depth)?..].iter().collect())
}

/// Replaces the link `at` with one of the same kind to `new_target`. The new link is made
/// beside it first, so a failure leaves the old one. On failure, returns the temporary
/// link this call made and could not remove, if any; the old link may be missing then
/// only if it could not be put back.
fn repoint_link(new_target: &Path, at: &Path) -> Result<(), Option<PathBuf>> {
    let old_target = fs::read_link(at).map_err(|_| None)?;
    let temp = retarget_temp(at);
    // Fails if the name is taken, so nothing of anyone else's is touched.
    copy_link(at, new_target, &temp).map_err(|_| None)?;
    let swapped = cross_volume::remove_link(at).and_then(|()| noclobber::rename(&temp, at));
    if swapped.is_ok() {
        return Ok(());
    }
    // Put back what was there, whatever state the swap stopped in.
    if fs::symlink_metadata(at).is_err() {
        let _ = copy_link(&temp, &old_target, at);
    }
    Err(cross_volume::remove_link(&temp).err().map(|_| temp))
}

/// After the folder known as any of `olds` was renamed to `new`, points the links inside
/// it that led into the old folder at the same places under `new`: renaming a folder does
/// not touch absolute link targets, which would dangle. Links that can't be repointed are
/// added to `not_repointed`.
///
/// This reads every folder in the tree once (folders only, no file is opened), so a move
/// that was instant now takes as long as listing the tree. It runs on the blocking
/// thread of the move command, and links are never followed. Folders whose contents are
/// only in the cloud are not entered, so listing them doesn't download anything.
fn repoint_renamed_tree(olds: &[PathBuf], new: &Path, not_repointed: &mut Vec<PathBuf>) {
    // Absolute, so a symlink's new target doesn't depend on the working directory.
    let new = std::path::absolute(new).unwrap_or_else(|_| new.to_path_buf());
    let mut folders = vec![new.clone()];
    while let Some(folder) = folders.pop() {
        let Ok(entries) = fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let at = entry.path();
            if kind.is_dir() {
                if !is_cloud_only(&entry) {
                    folders.push(at);
                }
            } else if kind.is_symlink() {
                // A link of a kind that can't be read has nothing to repoint.
                let Some(rest) = fs::read_link(&at)
                    .ok()
                    .and_then(|target| olds.iter().find_map(|old| inside_tree(old, &target)))
                else {
                    continue;
                };
                if let Err(leftover) = repoint_link(&new.join(rest), &at) {
                    not_repointed.push(at);
                    not_repointed.extend(leftover);
                }
            }
        }
    }
}

/// A folder whose contents a sync provider keeps in the cloud until they are opened.
#[cfg(windows)]
fn is_cloud_only(entry: &fs::DirEntry) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_OFFLINE: u32 = 0x1000;
    const FILE_ATTRIBUTE_RECALL_ON_OPEN: u32 = 0x4_0000;
    const FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS: u32 = 0x40_0000;
    // The attributes come with the listing; nothing is opened.
    entry.metadata().is_ok_and(|m| {
        m.file_attributes()
            & (FILE_ATTRIBUTE_OFFLINE
                | FILE_ATTRIBUTE_RECALL_ON_OPEN
                | FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS)
            != 0
    })
}

#[cfg(not(windows))]
fn is_cloud_only(_entry: &fs::DirEntry) -> bool {
    false
}

/// Where a replacement for the link `at` is made before it is swapped in.
fn retarget_temp(at: &Path) -> PathBuf {
    at.with_file_name(format!(
        "{}.folio-retarget",
        at.file_name().and_then(|n| n.to_str()).unwrap_or_default()
    ))
}

/// Copies `src` into `dest_dir` as `name`, or the next free name. The copy is built
/// under a temporary `name.folio-partial` and renamed, without replacing anything,
/// once complete; a failure removes what it created. Returns `None` for a link that
/// can't be recreated. Links inside the tree that lead back into it are pointed at the
/// copy, not the original.
fn copy_tree(
    src: &Path,
    dest_dir: &Path,
    name: &str,
    obs: &mut dyn Observe,
) -> io::Result<Option<PathBuf>> {
    let mut retarget = Retarget {
        inner: obs,
        root: src,
        inside: Vec::new(),
    };
    let target = build_tree(src, dest_dir, name, &mut retarget)?;
    if let Some(copy) = &target {
        retarget.apply(copy);
    }
    Ok(target)
}

fn build_tree(
    src: &Path,
    dest_dir: &Path,
    name: &str,
    obs: &mut dyn Observe,
) -> io::Result<Option<PathBuf>> {
    let kind = fs::symlink_metadata(src)?.file_type();
    if kind.is_symlink() {
        // Created under its real name: a link has no contents to leave half-built
        // (a junction is an empty folder for the instant before its reparse data).
        let Some(target) = read_link_or_skip(src, obs.skipped())? else {
            return Ok(None);
        };
        let placed = noclobber::place(dest_dir, name, |t| copy_link(src, &target, t));
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

/// A folder being copied: its entries are read up front so no directory handle stays
/// open per level (a deep tree would run out of them).
struct Frame {
    src: PathBuf,
    dst: PathBuf,
    entries: std::vec::IntoIter<std::ffi::OsString>,
}

impl Frame {
    fn open(src: &Path, dst: &Path) -> io::Result<Self> {
        let entries = fs::read_dir(src)?
            .map(|e| e.map(|e| e.file_name()))
            .collect::<io::Result<Vec<_>>>()?;
        Ok(Self {
            src: src.to_path_buf(),
            dst: dst.to_path_buf(),
            entries: entries.into_iter(),
        })
    }
}

/// Copies what is inside the folder `src` into the new folder `dst`. The walk keeps its
/// place on an explicit stack, not the call stack, so a tree as deep as the path length
/// limit allows can't overflow the thread's stack.
fn copy_contents(src: &Path, dst: &Path, obs: &mut dyn Observe) -> io::Result<()> {
    let mut open = vec![Frame::open(src, dst)?];
    while let Some(top) = open.last_mut() {
        let Some(name) = top.entries.next() else {
            obs.folder(&top.src);
            open.pop();
            continue;
        };
        let (from, to) = (top.src.join(&name), top.dst.join(&name));
        if let Some(child) = copy_entry(&from, &to, obs)? {
            open.push(child);
        }
    }
    Ok(())
}

/// Copies one entry without following links, so a link to an ancestor can't loop and a
/// link to elsewhere can't pull in data from outside the selection. A folder is created
/// empty and returned for the caller to fill.
fn copy_entry(src: &Path, dst: &Path, obs: &mut dyn Observe) -> io::Result<Option<Frame>> {
    let kind = fs::symlink_metadata(src)?.file_type();
    if kind.is_symlink() {
        // Read once: the recorded target is what was recreated.
        let Some(target) = read_link_or_skip(src, obs.skipped())? else {
            return Ok(None);
        };
        let copied = copy_link(src, &target, dst);
        if skip_unsupported(copied, src, obs.skipped())?.is_some() {
            if let Err(e) = obs.made().file(dst) {
                // Not recorded, so the failure cleanup can't remove it: do it here.
                let _ = cross_volume::remove_link(dst);
                return Err(e);
            }
            obs.link(src, target);
        }
        Ok(None)
    } else if kind.is_dir() {
        fs::create_dir(dst)?;
        obs.made().dir(dst);
        Frame::open(src, dst).map(Some)
    } else {
        obs.copy_file(src, dst).map(|()| None)
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
            is_link: meta.file_type().is_symlink(),
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
                let _ = if file.is_link {
                    // A folder link goes with remove_dir; either way only the link.
                    cross_volume::remove_link(&file.path)
                } else {
                    cross_volume::remove_even_read_only(&file.path, |p| fs::remove_file(p))
                };
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
    /// A symlink or junction: removed as a link, never followed.
    is_link: bool,
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

/// Reads where the link `src` points. On Windows a link of a kind that can't be recreated
/// (a WSL symlink, say) is skipped and reported, so it doesn't abort the copy of everything
/// around it. Any other failure to read a link is a real error.
fn read_link_or_skip(src: &Path, skipped: &mut Vec<PathBuf>) -> io::Result<Option<PathBuf>> {
    #[cfg(windows)]
    if !links::is_recreatable(src)? {
        skipped.push(src.to_path_buf());
        return Ok(None);
    }
    #[cfg(not(windows))]
    let _ = skipped;
    fs::read_link(src).map(Some)
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

/// Creates at `dst` a link to `target` (what `read_link` returned for the link `src`),
/// failing if `dst` exists. Reports `Unsupported` where the volume can't hold links
/// (FAT and exFAT refuse with EPERM).
#[cfg(unix)]
fn copy_link(_src: &Path, target: &Path, dst: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(target, dst).map_err(|e| {
        let codes = [libc::EPERM, libc::ENOTSUP, libc::EOPNOTSUPP, libc::ENOSYS];
        if e.raw_os_error().is_some_and(|c| codes.contains(&c)) {
            io::Error::new(ErrorKind::Unsupported, e)
        } else {
            e
        }
    })
}

/// Junctions are recreated as junctions. Symlinks need Developer Mode or admin rights;
/// without them the link is `Unsupported` and skipped (see [`links`]).
#[cfg(windows)]
fn copy_link(src: &Path, target: &Path, dst: &Path) -> io::Result<()> {
    links::copy(src, target, dst)
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
