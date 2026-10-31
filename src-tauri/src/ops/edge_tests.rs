//! Edge-case integration tests for the file operations (issue #19).
//!
//! Covers conflicts, locked files, paths > 260 chars, disk full and read-only
//! targets. Everything runs inside `tempfile` directories. Windows-only tests are
//! gated with `#[cfg(windows)]`.
//!
//! # Disk-full tests (`#[ignore]`d)
//!
//! A full disk cannot be simulated on a single local volume without admin rights.
//! To run them, mount a small volume (for example a ~16 MiB VHD formatted as NTFS,
//! created via Disk Management or `diskpart`) and point `FOLIO_SMALL_VOLUME` at its
//! root, then run with `--ignored`:
//!
//! ```text
//! FOLIO_SMALL_VOLUME=E:\ cargo test --manifest-path src-tauri/Cargo.toml ops::edge_tests::disk_full -- --ignored
//! ```
//!
//! These tests are manual-only: they never run in CI or in a plain `cargo test`.
//! The tests fill the volume inside a private temp directory, which is removed at the
//! end. The volume must be a different volume than the system temp directory so that
//! the move test exercises the cross-volume copy-then-delete path.

use super::*;
use std::path::{Path, PathBuf};

fn tmp() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

fn write(path: impl AsRef<Path>, content: &str) {
    fs::write(path, content).unwrap();
}

fn read(path: impl AsRef<Path>) -> String {
    fs::read_to_string(path).unwrap()
}

fn dir(path: impl AsRef<Path>) -> PathBuf {
    fs::create_dir_all(path.as_ref()).unwrap();
    path.as_ref().to_path_buf()
}

fn one(p: &Path) -> Vec<PathBuf> {
    vec![p.to_path_buf()]
}

// ---------------------------------------------------------------- 1. Conflicts

#[test]
fn copy_into_keeps_both_and_never_overwrites_existing_file() {
    let t = tmp();
    let src = dir(t.path().join("src"));
    let dest = dir(t.path().join("dest"));
    write(src.join("f.txt"), "new");
    write(dest.join("f.txt"), "old");

    let out = copy_into(&one(&src.join("f.txt")), &dest).unwrap().paths;

    assert_eq!(out[0], dest.join("f (2).txt"));
    assert_eq!(read(dest.join("f.txt")), "old");
    assert_eq!(read(dest.join("f (2).txt")), "new");
    assert_eq!(read(src.join("f.txt")), "new");
}

#[test]
fn move_into_keeps_both_and_never_overwrites_existing_file() {
    let t = tmp();
    let src = dir(t.path().join("src"));
    let dest = dir(t.path().join("dest"));
    write(src.join("f.txt"), "new");
    write(dest.join("f.txt"), "old");

    let out = move_into(&one(&src.join("f.txt")), &dest).unwrap().paths;

    assert_eq!(out[0], dest.join("f (2).txt"));
    assert_eq!(read(dest.join("f.txt")), "old");
    assert_eq!(read(dest.join("f (2).txt")), "new");
    assert!(!src.join("f.txt").exists());
}

#[test]
fn copy_into_multiple_sources_with_same_name_get_distinct_targets() {
    let t = tmp();
    let a = dir(t.path().join("a"));
    let b = dir(t.path().join("b"));
    let c = dir(t.path().join("c"));
    let dest = dir(t.path().join("dest"));
    write(a.join("f.txt"), "A");
    write(b.join("f.txt"), "B");
    write(c.join("f.txt"), "C");

    let out = copy_into(&[a.join("f.txt"), b.join("f.txt"), c.join("f.txt")], &dest)
        .unwrap()
        .paths;

    assert_eq!(
        out,
        vec![
            dest.join("f.txt"),
            dest.join("f (2).txt"),
            dest.join("f (3).txt")
        ]
    );
    assert_eq!(read(&out[0]), "A");
    assert_eq!(read(&out[1]), "B");
    assert_eq!(read(&out[2]), "C");
}

#[test]
fn move_into_multiple_sources_with_same_name_get_distinct_targets() {
    let t = tmp();
    let a = dir(t.path().join("a"));
    let b = dir(t.path().join("b"));
    let dest = dir(t.path().join("dest"));
    write(a.join("f.txt"), "A");
    write(b.join("f.txt"), "B");

    let out = move_into(&[a.join("f.txt"), b.join("f.txt")], &dest)
        .unwrap()
        .paths;

    assert_eq!(out, vec![dest.join("f.txt"), dest.join("f (2).txt")]);
    assert_eq!(read(&out[0]), "A");
    assert_eq!(read(&out[1]), "B");
    assert!(!a.join("f.txt").exists() && !b.join("f.txt").exists());
}

#[test]
fn copy_into_folder_does_not_merge_into_file_with_same_name() {
    let t = tmp();
    let src = dir(t.path().join("src"));
    let dest = dir(t.path().join("dest"));
    dir(src.join("item"));
    write(src.join("item").join("inner.txt"), "inner");
    write(dest.join("item"), "i am a file");

    let out = copy_into(&one(&src.join("item")), &dest).unwrap().paths;

    assert_eq!(out[0], dest.join("item (2)"));
    assert_eq!(read(dest.join("item")), "i am a file");
    assert_eq!(read(dest.join("item (2)").join("inner.txt")), "inner");
}

#[test]
fn copy_into_file_does_not_replace_folder_with_same_name() {
    let t = tmp();
    let src = dir(t.path().join("src"));
    let dest = dir(t.path().join("dest"));
    write(src.join("item"), "file");
    dir(dest.join("item"));
    write(dest.join("item").join("keep.txt"), "keep");

    let out = copy_into(&one(&src.join("item")), &dest).unwrap().paths;

    assert_eq!(out[0], dest.join("item (2)"));
    assert!(dest.join("item").is_dir());
    assert_eq!(read(dest.join("item").join("keep.txt")), "keep");
    assert_eq!(read(dest.join("item (2)")), "file");
}

#[test]
fn move_into_folder_does_not_merge_into_file_with_same_name() {
    let t = tmp();
    let src = dir(t.path().join("src"));
    let dest = dir(t.path().join("dest"));
    dir(src.join("item"));
    write(src.join("item").join("inner.txt"), "inner");
    write(dest.join("item"), "i am a file");

    let out = move_into(&one(&src.join("item")), &dest).unwrap().paths;

    assert_eq!(out[0], dest.join("item (2)"));
    assert_eq!(read(dest.join("item")), "i am a file");
    assert_eq!(read(dest.join("item (2)").join("inner.txt")), "inner");
    assert!(!src.join("item").exists());
}

#[test]
fn move_into_folder_with_same_named_folder_keeps_both_without_merging() {
    let t = tmp();
    let src = dir(t.path().join("src"));
    let dest = dir(t.path().join("dest"));
    dir(src.join("docs"));
    write(src.join("docs").join("a.txt"), "from src");
    write(src.join("docs").join("clash.txt"), "src clash");
    dir(dest.join("docs"));
    write(dest.join("docs").join("clash.txt"), "dest clash");
    write(dest.join("docs").join("keep.txt"), "keep");

    let out = move_into(&one(&src.join("docs")), &dest).unwrap().paths;

    assert_eq!(out[0], dest.join("docs (2)"));
    // Existing folder is untouched: no merge, no overwrite.
    assert_eq!(read(dest.join("docs").join("clash.txt")), "dest clash");
    assert_eq!(read(dest.join("docs").join("keep.txt")), "keep");
    assert!(!dest.join("docs").join("a.txt").exists());
    // Moved folder arrived whole.
    assert_eq!(read(dest.join("docs (2)").join("a.txt")), "from src");
    assert_eq!(read(dest.join("docs (2)").join("clash.txt")), "src clash");
    assert!(!src.join("docs").exists());
}

#[test]
fn copy_into_folder_with_same_named_folder_keeps_both_without_merging() {
    let t = tmp();
    let src = dir(t.path().join("src"));
    let dest = dir(t.path().join("dest"));
    dir(src.join("docs"));
    write(src.join("docs").join("clash.txt"), "src clash");
    dir(dest.join("docs"));
    write(dest.join("docs").join("clash.txt"), "dest clash");

    let out = copy_into(&one(&src.join("docs")), &dest).unwrap().paths;

    assert_eq!(out[0], dest.join("docs (2)"));
    assert_eq!(read(dest.join("docs").join("clash.txt")), "dest clash");
    assert_eq!(read(dest.join("docs (2)").join("clash.txt")), "src clash");
    assert_eq!(read(src.join("docs").join("clash.txt")), "src clash");
}

#[test]
fn conflict_resolution_handles_non_ascii_names() {
    let t = tmp();
    let src = dir(t.path().join("src"));
    let dest = dir(t.path().join("dest"));
    let name = "отчёт 日本語 ñ.txt";
    write(src.join(name), "new");
    write(dest.join(name), "old");

    let copied = copy_into(&one(&src.join(name)), &dest).unwrap().paths;
    let moved = move_into(&one(&src.join(name)), &dest).unwrap().paths;

    assert_eq!(copied[0], dest.join("отчёт 日本語 ñ (2).txt"));
    assert_eq!(moved[0], dest.join("отчёт 日本語 ñ (3).txt"));
    assert_eq!(read(dest.join(name)), "old");
    assert_eq!(read(&copied[0]), "new");
    assert_eq!(read(&moved[0]), "new");
}

// ------------------------------------------------------------ 2. Locked files

#[cfg(windows)]
mod locked {
    use super::*;
    use std::fs::OpenOptions;
    use std::os::windows::fs::OpenOptionsExt;

    /// Opens `path` with no sharing; any other open (including copy/rename) fails
    /// with a sharing violation while the returned handle is alive.
    pub fn lock(path: &Path) -> fs::File {
        OpenOptions::new()
            .read(true)
            .write(true)
            .share_mode(0)
            .open(path)
            .unwrap()
    }

    #[test]
    fn copy_into_locked_file_errors_and_leaves_no_partial_target() {
        let t = tmp();
        let src = dir(t.path().join("src"));
        let dest = dir(t.path().join("dest"));
        write(src.join("locked.bin"), "payload");
        let guard = lock(&src.join("locked.bin"));

        let res = copy_into(&one(&src.join("locked.bin")), &dest);

        assert!(res.is_err(), "copying a locked file must fail");
        assert!(
            !dest.join("locked.bin").exists(),
            "no partial target may remain"
        );
        assert_eq!(fs::read_dir(&dest).unwrap().count(), 0);
        drop(guard);
        assert_eq!(read(src.join("locked.bin")), "payload");
    }

    #[test]
    fn move_into_locked_file_errors_and_keeps_source() {
        let t = tmp();
        let src = dir(t.path().join("src"));
        let dest = dir(t.path().join("dest"));
        write(src.join("locked.bin"), "payload");
        let guard = lock(&src.join("locked.bin"));

        let res = move_into(&one(&src.join("locked.bin")), &dest);

        assert!(res.is_err(), "moving a locked file must fail");
        assert!(!dest.join("locked.bin").exists());
        drop(guard);
        assert_eq!(read(src.join("locked.bin")), "payload");
    }

    #[test]
    fn rename_locked_file_errors_and_keeps_source() {
        let t = tmp();
        write(t.path().join("locked.bin"), "payload");
        let guard = lock(&t.path().join("locked.bin"));

        let res = rename(&t.path().join("locked.bin"), "renamed.bin");

        assert!(res.is_err(), "renaming a locked file must fail");
        assert!(!t.path().join("renamed.bin").exists());
        drop(guard);
        assert_eq!(read(t.path().join("locked.bin")), "payload");
    }

    #[test]
    fn move_into_folder_containing_locked_file_errors_and_keeps_source_tree() {
        let t = tmp();
        let src = dir(t.path().join("src").join("tree"));
        let dest = dir(t.path().join("dest"));
        write(src.join("a.txt"), "a");
        write(src.join("locked.bin"), "payload");
        let guard = lock(&src.join("locked.bin"));

        let res = move_into(&one(&src), &dest);

        assert!(res.is_err());
        assert!(!dest.join("tree").exists());
        drop(guard);
        assert_eq!(read(src.join("a.txt")), "a");
        assert_eq!(read(src.join("locked.bin")), "payload");
    }

    /// Characterizes today's behavior when a copy fails halfway through a tree:
    /// the error is reported, the source is untouched, but the destination folder
    /// created before the failure (with the files copied so far) is left behind.
    #[test]
    fn copy_into_failing_mid_tree_reports_error_and_leaves_partial_destination() {
        let t = tmp();
        let src = dir(t.path().join("src").join("tree"));
        let dest = dir(t.path().join("dest"));
        write(src.join("a.txt"), "a");
        write(src.join("b_locked.bin"), "payload");
        write(src.join("c.txt"), "c");
        let guard = lock(&src.join("b_locked.bin"));

        let res = copy_into(&one(&src), &dest);

        assert!(res.is_err());
        // The locked file itself never produces a partial target file.
        assert!(!dest.join("tree").join("b_locked.bin").exists());
        // Current behavior: the half-built folder remains (see the ignored test below).
        assert!(
            dest.join("tree").is_dir(),
            "characterization: partial destination folder is left behind"
        );
        drop(guard);
        assert_eq!(read(src.join("a.txt")), "a");
        assert_eq!(read(src.join("b_locked.bin")), "payload");
        assert_eq!(read(src.join("c.txt")), "c");
    }

    #[test]
    #[ignore = "bug: copy_into leaves a partially copied destination folder when a file fails mid-tree"]
    fn copy_into_failing_mid_tree_cleans_up_partial_destination() {
        let t = tmp();
        let src = dir(t.path().join("src").join("tree"));
        let dest = dir(t.path().join("dest"));
        write(src.join("a.txt"), "a");
        write(src.join("b_locked.bin"), "payload");
        write(src.join("c.txt"), "c");
        let guard = lock(&src.join("b_locked.bin"));

        let res = copy_into(&one(&src), &dest);

        assert!(res.is_err());
        let leftovers: Vec<_> = fs::read_dir(&dest)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        drop(guard);
        assert!(
            leftovers.is_empty(),
            "failed copy left partial output behind: {leftovers:?}"
        );
    }
}

// -------------------------------------------------------- 3. Paths > 260 chars

/// Creates a nested chain of 50-char folders under `base` until the chain alone is
/// longer than 270 chars, so the full path is always beyond `MAX_PATH`.
fn long_chain(base: &Path) -> PathBuf {
    let mut p = base.to_path_buf();
    let mut len = 0;
    while len <= 270 {
        p.push("d".repeat(50));
        len += 51;
    }
    fs::create_dir_all(&p).unwrap();
    p
}

fn path_len(p: &Path) -> usize {
    p.to_string_lossy().chars().count()
}

#[test]
fn copy_into_folder_tree_deeper_than_260_chars_preserves_contents() {
    let t = tmp();
    let root = dir(t.path().join("src").join("tree"));
    let deep = long_chain(&root);
    write(deep.join("leaf.txt"), "leaf");
    write(root.join("top.txt"), "top");
    let dest = dir(t.path().join("dest"));

    let out = copy_into(&one(&root), &dest).unwrap().paths;

    let rel = deep.strip_prefix(&root).unwrap();
    let copied_leaf = out[0].join(rel).join("leaf.txt");
    assert!(path_len(&copied_leaf) > 260);
    assert_eq!(read(&copied_leaf), "leaf");
    assert_eq!(read(out[0].join("top.txt")), "top");
    assert_eq!(read(deep.join("leaf.txt")), "leaf");
}

#[test]
fn copy_into_single_file_with_path_over_260_chars_succeeds() {
    let t = tmp();
    let deep = long_chain(&t.path().join("src"));
    let file = deep.join(format!("{}.txt", "f".repeat(100)));
    write(&file, "long");
    assert!(path_len(&file) > 260);
    let dest_short = dir(t.path().join("dest"));
    let dest_long = long_chain(&t.path().join("dest2"));

    let to_short = copy_into(&one(&file), &dest_short).unwrap().paths;
    let to_long = copy_into(&one(&file), &dest_long).unwrap().paths;

    assert_eq!(read(&to_short[0]), "long");
    assert!(path_len(&to_long[0]) > 260);
    assert_eq!(read(&to_long[0]), "long");
    assert_eq!(read(&file), "long");
}

#[test]
fn copy_into_long_destination_conflict_still_keeps_both() {
    let t = tmp();
    let dest_long = long_chain(&t.path().join("dest"));
    let src = dir(t.path().join("src"));
    write(src.join("f.txt"), "new");
    write(dest_long.join("f.txt"), "old");

    let out = copy_into(&one(&src.join("f.txt")), &dest_long)
        .unwrap()
        .paths;

    assert_eq!(out[0], dest_long.join("f (2).txt"));
    assert_eq!(read(dest_long.join("f.txt")), "old");
    assert_eq!(read(&out[0]), "new");
}

#[test]
fn move_into_folder_tree_deeper_than_260_chars_preserves_contents() {
    let t = tmp();
    let root = dir(t.path().join("src").join("tree"));
    let deep = long_chain(&root);
    write(deep.join("leaf.txt"), "leaf");
    let rel = deep.strip_prefix(&root).unwrap().to_path_buf();
    let dest = dir(t.path().join("dest"));

    let out = move_into(&one(&root), &dest).unwrap().paths;

    let moved_leaf = out[0].join(&rel).join("leaf.txt");
    assert!(path_len(&moved_leaf) > 260);
    assert_eq!(read(&moved_leaf), "leaf");
    assert!(!root.exists());
}

#[test]
fn move_into_single_file_with_path_over_260_chars_succeeds() {
    let t = tmp();
    let deep = long_chain(&t.path().join("src"));
    let file = deep.join(format!("{}.txt", "f".repeat(100)));
    write(&file, "long");
    let dest_long = long_chain(&t.path().join("dest"));

    let out = move_into(&one(&file), &dest_long).unwrap().paths;

    assert!(path_len(&out[0]) > 260);
    assert_eq!(read(&out[0]), "long");
    assert!(!file.exists());
}

#[test]
fn rename_file_and_folder_with_path_over_260_chars_succeeds() {
    let t = tmp();
    let deep = long_chain(&t.path().join("src"));
    let file = deep.join("before.txt");
    write(&file, "long");

    let renamed_file = rename(&file, "after.txt").unwrap();
    assert!(path_len(&renamed_file) > 260);
    assert_eq!(read(&renamed_file), "long");
    assert!(!file.exists());

    // Rename the deepest folder itself; the contents must come along.
    let renamed_dir = rename(&deep, "renamed").unwrap();
    assert!(path_len(&renamed_dir) > 260);
    assert_eq!(read(renamed_dir.join("after.txt")), "long");
    assert!(!deep.exists());
}

#[test]
fn create_folder_with_path_over_260_chars_succeeds() {
    let t = tmp();
    let deep = long_chain(t.path());

    let first = create_folder(&deep, "New folder").unwrap();
    let second = create_folder(&deep, "New folder").unwrap();

    assert!(path_len(&first) > 260);
    assert!(first.is_dir() && second.is_dir());
    assert_eq!(second, deep.join("New folder (2)"));
}

// ---------------------------------------------------------------- 4. Disk full

/// Largest volume the disk-full tests are willing to fill.
const MAX_VOLUME_BYTES: u64 = 256 * 1024 * 1024;

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetDiskFreeSpaceExW(
        dir: *const u16,
        free_to_caller: *mut u64,
        total: *mut u64,
        total_free: *mut u64,
    ) -> i32;
}

/// Total size in bytes of the volume holding `path`.
#[cfg(windows)]
fn volume_total_bytes(path: &Path) -> u64 {
    use std::os::windows::ffi::OsStrExt;
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let (mut free_caller, mut total, mut free) = (0u64, 0u64, 0u64);
    // SAFETY: `wide` is NUL-terminated and the out pointers are valid for the call.
    let ok = unsafe {
        GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &raw mut free_caller,
            &raw mut total,
            &raw mut free,
        )
    };
    assert!(ok != 0, "GetDiskFreeSpaceExW failed for {}", path.display());
    total
}

#[cfg(not(windows))]
fn volume_total_bytes(_path: &Path) -> u64 {
    u64::MAX // disk-full tests are Windows-only for now; refuse everywhere else
}

fn volume_prefix(path: &Path) -> Option<PathBuf> {
    use std::path::Component;
    let canon = fs::canonicalize(path).ok()?;
    match canon.components().next() {
        Some(Component::Prefix(p)) => Some(PathBuf::from(p.as_os_str())),
        _ => None,
    }
}

/// Validates `FOLIO_SMALL_VOLUME` and returns it. Panics (refuses to run) unless the
/// volume is at most 256 MiB and is not the volume holding the system temp dir.
fn small_volume() -> PathBuf {
    let Some(volume) = std::env::var_os("FOLIO_SMALL_VOLUME").map(PathBuf::from) else {
        panic!("set FOLIO_SMALL_VOLUME to the root of a small volume");
    };
    assert!(volume.is_dir(), "FOLIO_SMALL_VOLUME is not a directory");
    let total = volume_total_bytes(&volume);
    assert!(
        total <= MAX_VOLUME_BYTES,
        "refusing to fill {}: volume is {} MiB, limit is {} MiB",
        volume.display(),
        total / (1024 * 1024),
        MAX_VOLUME_BYTES / (1024 * 1024)
    );
    let temp = std::env::temp_dir();
    let same = volume_prefix(&volume).is_some() && volume_prefix(&volume) == volume_prefix(&temp);
    assert!(
        !same,
        "refusing to run: FOLIO_SMALL_VOLUME is on the same volume as {}",
        temp.display()
    );
    volume
}

/// Fills the volume with a file and removes it again on drop, even on panic.
struct Filler(PathBuf);

impl Filler {
    /// Writes until a write fails, but never more than `MAX_VOLUME_BYTES` in total.
    fn fill(volume_dir: &Path) -> Filler {
        use std::io::Write;
        let path = volume_dir.join("filler.bin");
        let filler = Filler(path.clone()); // registered first so it is removed on any exit
        let mut f = fs::File::create(&path).unwrap();
        let chunk = vec![0u8; 64 * 1024];
        let mut written = 0u64;
        while written < MAX_VOLUME_BYTES && f.write_all(&chunk).is_ok() {
            written += chunk.len() as u64;
        }
        let _ = f.flush();
        filler
    }
}

impl Drop for Filler {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

#[test]
#[ignore = "manual only: needs FOLIO_SMALL_VOLUME (small second volume); see module docs"]
fn disk_full_copy_into_errors_and_leaves_no_partial_file() {
    let volume = small_volume();
    let work = tempfile::tempdir_in(&volume).unwrap();
    let dest = dir(work.path().join("dest"));
    let src_dir = tmp();
    let src = src_dir.path().join("big.bin");
    fs::write(&src, vec![7u8; 8 * 1024 * 1024]).unwrap();
    let _filler = Filler::fill(work.path());

    let res = copy_into(&one(&src), &dest);

    let leftovers: Vec<_> = fs::read_dir(&dest)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert!(res.is_err(), "copy into a full volume must fail");
    assert!(
        leftovers.is_empty(),
        "failed copy left a partial file: {leftovers:?}"
    );
    assert!(src.exists());
}

#[test]
#[ignore = "manual only: needs FOLIO_SMALL_VOLUME (small second volume); see module docs"]
fn disk_full_cross_volume_move_into_errors_and_keeps_source() {
    let volume = small_volume();
    let work = tempfile::tempdir_in(&volume).unwrap();
    let dest = dir(work.path().join("dest"));
    let src_dir = tmp();
    let src = dir(src_dir.path().join("tree"));
    fs::write(src.join("big.bin"), vec![7u8; 8 * 1024 * 1024]).unwrap();
    fs::write(src.join("small.txt"), "small").unwrap();
    let _filler = Filler::fill(work.path());

    let res = move_into(&one(&src), &dest);

    assert!(res.is_err(), "move into a full volume must fail");
    assert_eq!(
        fs::metadata(src.join("big.bin")).unwrap().len(),
        8 * 1024 * 1024
    );
    assert_eq!(read(src.join("small.txt")), "small");
    // Not asserted: that `dest` is left empty. A failed folder copy currently leaves
    // the partially built destination behind (bug, see the ignored
    // `copy_into_failing_mid_tree_cleans_up_partial_destination`). Once that is fixed,
    // also assert `fs::read_dir(&dest)` is empty here.
}

// ----------------------------------------------------------- 5. Read-only targets

fn set_readonly(p: &Path, ro: bool) {
    let mut perms = fs::metadata(p).unwrap().permissions();
    perms.set_readonly(ro);
    fs::set_permissions(p, perms).unwrap();
}

fn is_readonly(p: &Path) -> bool {
    fs::metadata(p).unwrap().permissions().readonly()
}

/// Makes the path writable again so `TempDir` cleanup can delete it.
struct RoGuard(PathBuf);
impl Drop for RoGuard {
    fn drop(&mut self) {
        if self.0.exists() {
            #[allow(clippy::permissions_set_readonly_false)]
            set_readonly(&self.0, false);
        }
    }
}

#[test]
fn copy_into_read_only_file_succeeds_and_keeps_attribute() {
    let t = tmp();
    let src = dir(t.path().join("src"));
    let dest = dir(t.path().join("dest"));
    write(src.join("ro.txt"), "ro");
    set_readonly(&src.join("ro.txt"), true);
    let _g1 = RoGuard(src.join("ro.txt"));
    let _g2 = RoGuard(dest.join("ro.txt"));

    let out = copy_into(&one(&src.join("ro.txt")), &dest).unwrap().paths;

    assert_eq!(read(&out[0]), "ro");
    assert!(
        is_readonly(&out[0]),
        "copy should keep the read-only attribute"
    );
    assert!(is_readonly(&src.join("ro.txt")));
}

#[test]
fn move_into_read_only_file_succeeds_and_keeps_attribute() {
    let t = tmp();
    let src = dir(t.path().join("src"));
    let dest = dir(t.path().join("dest"));
    write(src.join("ro.txt"), "ro");
    set_readonly(&src.join("ro.txt"), true);
    let _g1 = RoGuard(src.join("ro.txt"));
    let _g2 = RoGuard(dest.join("ro.txt"));

    let out = move_into(&one(&src.join("ro.txt")), &dest).unwrap().paths;

    assert_eq!(read(&out[0]), "ro");
    assert!(is_readonly(&out[0]));
    assert!(!src.join("ro.txt").exists());
}

#[test]
fn rename_read_only_file_succeeds_and_keeps_attribute() {
    let t = tmp();
    write(t.path().join("ro.txt"), "ro");
    set_readonly(&t.path().join("ro.txt"), true);
    let _g1 = RoGuard(t.path().join("ro.txt"));
    let _g2 = RoGuard(t.path().join("renamed.txt"));

    let out = rename(&t.path().join("ro.txt"), "renamed.txt").unwrap();

    assert_eq!(read(&out), "ro");
    assert!(is_readonly(&out));
}

#[test]
fn copy_into_read_only_folder_tree_copies_contents() {
    let t = tmp();
    let src = dir(t.path().join("src").join("tree"));
    let dest = dir(t.path().join("dest"));
    write(src.join("ro.txt"), "ro");
    set_readonly(&src.join("ro.txt"), true);
    let _g1 = RoGuard(src.join("ro.txt"));
    let _g2 = RoGuard(dest.join("tree").join("ro.txt"));

    let out = copy_into(&one(&src), &dest).unwrap().paths;

    assert_eq!(read(out[0].join("ro.txt")), "ro");
    assert!(is_readonly(&out[0].join("ro.txt")));
}

#[test]
fn copy_into_dest_with_read_only_file_of_same_name_keeps_both_untouched() {
    let t = tmp();
    let src = dir(t.path().join("src"));
    let dest = dir(t.path().join("dest"));
    write(src.join("f.txt"), "new");
    write(dest.join("f.txt"), "old");
    set_readonly(&dest.join("f.txt"), true);
    let _g1 = RoGuard(dest.join("f.txt"));
    let _g2 = RoGuard(dest.join("f (2).txt"));

    let out = copy_into(&one(&src.join("f.txt")), &dest).unwrap().paths;

    assert_eq!(out[0], dest.join("f (2).txt"));
    assert_eq!(read(dest.join("f.txt")), "old");
    assert!(is_readonly(&dest.join("f.txt")));
    assert_eq!(read(&out[0]), "new");
}

#[test]
fn move_into_dest_with_read_only_file_of_same_name_keeps_both_untouched() {
    let t = tmp();
    let src = dir(t.path().join("src"));
    let dest = dir(t.path().join("dest"));
    write(src.join("f.txt"), "new");
    write(dest.join("f.txt"), "old");
    set_readonly(&dest.join("f.txt"), true);
    let _g1 = RoGuard(dest.join("f.txt"));

    let out = move_into(&one(&src.join("f.txt")), &dest).unwrap().paths;

    assert_eq!(out[0], dest.join("f (2).txt"));
    assert_eq!(read(dest.join("f.txt")), "old");
    assert!(is_readonly(&dest.join("f.txt")));
    assert_eq!(read(&out[0]), "new");
}

#[test]
fn rename_onto_read_only_file_is_refused_and_both_untouched() {
    let t = tmp();
    write(t.path().join("a.txt"), "a");
    write(t.path().join("b.txt"), "b");
    set_readonly(&t.path().join("b.txt"), true);
    let _g = RoGuard(t.path().join("b.txt"));

    assert!(rename(&t.path().join("a.txt"), "b.txt").is_err());

    assert_eq!(read(t.path().join("a.txt")), "a");
    assert_eq!(read(t.path().join("b.txt")), "b");
}

/// On Windows the read-only attribute on a directory does not stop files being created
/// in it, so it says nothing about "unwritable destination". This documents that.
#[cfg(windows)]
#[test]
fn windows_read_only_attribute_on_dest_folder_does_not_block_copy() {
    let t = tmp();
    let src = dir(t.path().join("src"));
    let dest = dir(t.path().join("dest"));
    write(src.join("f.txt"), "x");
    set_readonly(&dest, true);
    let _g = RoGuard(dest.clone());

    let res = copy_into(&one(&src.join("f.txt")), &dest);

    assert!(
        res.is_ok(),
        "directory read-only attribute is ignored on Windows"
    );
    assert_eq!(read(dest.join("f.txt")), "x");
}

/// A genuinely unwritable folder, built with an ACL deny entry for the current user
/// (no admin needed on a directory we own). The ACE is removed again on drop.
#[cfg(windows)]
mod unwritable {
    use super::*;
    use std::process::Command;

    struct DenyWrite {
        dir: PathBuf,
        sid: String,
    }

    /// The current user's SID, as `*S-1-...` (the form icacls accepts for a SID).
    fn current_sid() -> Option<String> {
        let out = Command::new("whoami")
            .args(["/user", "/fo", "csv", "/nh"])
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        let sid = text.trim().split(',').nth(1)?.trim().trim_matches('"');
        sid.starts_with("S-1-").then(|| format!("*{sid}"))
    }

    impl DenyWrite {
        /// Returns `None` when icacls is unavailable or the deny does not take effect.
        fn apply(dir: &Path) -> Option<Self> {
            let sid = current_sid()?;
            let ok = Command::new("icacls")
                .arg(dir)
                .arg("/deny")
                .arg(format!("{sid}:(WD,AD)"))
                .output()
                .ok()?
                .status
                .success();
            let guard = DenyWrite {
                dir: dir.to_path_buf(),
                sid,
            };
            if !ok || fs::write(dir.join(".probe"), "").is_ok() {
                return None; // guard drops and restores
            }
            Some(guard)
        }

        /// Applies the deny or panics. Only `FOLIO_ALLOW_SKIP_ACL=1` turns a failure
        /// into a visible skip (`None`).
        fn apply_or_skip(dir: &Path) -> Option<Self> {
            let applied = Self::apply(dir);
            if applied.is_none() {
                if std::env::var_os("FOLIO_ALLOW_SKIP_ACL").is_some() {
                    eprintln!("SKIPPED: could not make a folder unwritable via icacls");
                } else {
                    panic!(
                        "could not make a folder unwritable via icacls; \
                         set FOLIO_ALLOW_SKIP_ACL=1 to skip these tests on this machine"
                    );
                }
            }
            applied
        }
    }

    impl Drop for DenyWrite {
        fn drop(&mut self) {
            let _ = Command::new("icacls")
                .arg(&self.dir)
                .arg("/remove:d")
                .arg(&self.sid)
                .output();
        }
    }

    #[test]
    fn copy_into_unwritable_dest_folder_errors_and_keeps_source() {
        let t = tmp();
        let src = dir(t.path().join("src"));
        let dest = dir(t.path().join("dest"));
        write(src.join("f.txt"), "x");
        dir(src.join("tree"));
        write(src.join("tree").join("g.txt"), "g");
        let Some(deny) = DenyWrite::apply_or_skip(&dest) else {
            return;
        };

        assert!(copy_into(&one(&src.join("f.txt")), &dest).is_err());
        assert!(copy_into(&one(&src.join("tree")), &dest).is_err());

        drop(deny);
        assert_eq!(fs::read_dir(&dest).unwrap().count(), 0);
        assert_eq!(read(src.join("f.txt")), "x");
        assert_eq!(read(src.join("tree").join("g.txt")), "g");
    }

    #[test]
    fn move_into_unwritable_dest_folder_errors_and_keeps_source() {
        let t = tmp();
        let src = dir(t.path().join("src"));
        let dest = dir(t.path().join("dest"));
        write(src.join("f.txt"), "x");
        let Some(deny) = DenyWrite::apply_or_skip(&dest) else {
            return;
        };

        assert!(move_into(&one(&src.join("f.txt")), &dest).is_err());

        drop(deny);
        assert_eq!(fs::read_dir(&dest).unwrap().count(), 0);
        assert_eq!(read(src.join("f.txt")), "x");
    }
}

// ------------------------------------------------- Folder into itself by case

/// NTFS is case-insensitive: `...\Src` into `...\src\inner` is the same folder tree,
/// and the guard must see that (it once compared path text, see #62).
#[cfg(windows)]
#[test]
fn copy_into_folder_into_itself_with_different_letter_case_is_refused() {
    let t = tmp();
    let src = dir(t.path().join("Src"));
    dir(src.join("inner"));
    let sneaky = t.path().join("src").join("inner");
    assert!(
        sneaky.is_dir(),
        "precondition: file system is case-insensitive"
    );

    let guard = ensure_not_into_itself(&src, &sneaky);

    assert!(
        guard.is_err(),
        "folder-into-itself check missed a case-differing spelling"
    );
    assert!(copy_into(&one(&src), &sneaky).is_err());
}

#[cfg(windows)]
#[test]
fn move_into_folder_into_itself_with_different_letter_case_fails_and_keeps_source() {
    let t = tmp();
    let src = dir(t.path().join("Src"));
    dir(src.join("inner"));
    write(src.join("inner").join("f.txt"), "x");
    let sneaky = t.path().join("src").join("inner");

    // Refused by the identity check (the OS would refuse the rename too).
    let res = move_into(&one(&src), &sneaky);

    assert!(res.is_err());
    assert_eq!(read(src.join("inner").join("f.txt")), "x");
    assert_eq!(fs::read_dir(src.join("inner")).unwrap().count(), 1);
}

// Cross-volume move_into (copy then delete source) cannot be tested on one volume;
// the `disk_full_cross_volume_move_into_*` test above covers it when
// FOLIO_SMALL_VOLUME points at a second volume.

// ------------------------------------------------- 6. Concurrent writers (#60)

#[test]
fn concurrent_copies_of_one_name_produce_distinct_files() {
    let t = tmp();
    let dest = dir(t.path().join("dest"));
    let sources: Vec<PathBuf> = (0..8)
        .map(|i| {
            let d = dir(t.path().join(format!("src{i}")));
            write(d.join("f.txt"), &format!("copy {i}"));
            d.join("f.txt")
        })
        .collect();

    let barrier = std::sync::Barrier::new(sources.len());
    let mut out: Vec<PathBuf> = std::thread::scope(|s| {
        let handles: Vec<_> = sources
            .iter()
            .map(|src| {
                let (barrier, dest) = (&barrier, &dest);
                s.spawn(move || {
                    barrier.wait();
                    copy_into(&one(src), dest).unwrap().paths.remove(0)
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });

    out.sort();
    out.dedup();
    assert_eq!(out.len(), 8, "every copy got its own name");
    let mut contents: Vec<String> = out.iter().map(read).collect();
    contents.sort();
    let expected: Vec<String> = (0..8).map(|i| format!("copy {i}")).collect();
    assert_eq!(contents, expected, "no copy overwrote another");
}

#[test]
fn concurrent_moves_of_one_name_produce_distinct_files() {
    let t = tmp();
    let dest = dir(t.path().join("dest"));
    let sources: Vec<PathBuf> = (0..8)
        .map(|i| {
            let d = dir(t.path().join(format!("src{i}")));
            write(d.join("f.txt"), &format!("move {i}"));
            d.join("f.txt")
        })
        .collect();

    let barrier = std::sync::Barrier::new(sources.len());
    std::thread::scope(|s| {
        for src in &sources {
            let (barrier, dest) = (&barrier, &dest);
            s.spawn(move || {
                barrier.wait();
                move_into(&one(src), dest).unwrap();
            });
        }
    });

    let mut contents: Vec<String> = fs::read_dir(&dest)
        .unwrap()
        .map(|e| read(e.unwrap().path()))
        .collect();
    contents.sort();
    let expected: Vec<String> = (0..8).map(|i| format!("move {i}")).collect();
    assert_eq!(contents, expected);
    assert!(sources.iter().all(|s| !s.exists()));
}

// ------------------------------------------------ 7. Case-only renames (#61)

/// A folder where `readme.md` and `README.md` are different files, or `None` when the
/// platform can't make one (Windows without per-directory case sensitivity).
fn case_sensitive_dir(t: &tempfile::TempDir) -> Option<PathBuf> {
    let d = dir(t.path().join("cs"));
    #[cfg(windows)]
    {
        let enabled = std::process::Command::new("fsutil")
            .args(["file", "setCaseSensitiveInfo"])
            .arg(&d)
            .arg("enable")
            .output()
            .is_ok_and(|o| o.status.success());
        if !enabled {
            eprintln!("skipped: per-directory case sensitivity is unavailable");
            return None;
        }
    }
    write(d.join("probe"), "");
    let sensitive = !d.join("PROBE").exists();
    fs::remove_file(d.join("probe")).unwrap();
    sensitive.then_some(d)
}

#[test]
fn case_only_rename_onto_a_different_file_is_refused() {
    let t = tmp();
    let Some(d) = case_sensitive_dir(&t) else {
        return;
    };
    write(d.join("readme.md"), "lower");
    write(d.join("README.md"), "upper");

    let err = rename(&d.join("readme.md"), "README.md").unwrap_err();

    assert_eq!(err.kind(), ErrorKind::AlreadyExists);
    assert_eq!(read(d.join("readme.md")), "lower");
    assert_eq!(read(d.join("README.md")), "upper");
}

#[test]
fn case_only_rename_without_a_clash_works_in_a_case_sensitive_folder() {
    let t = tmp();
    let Some(d) = case_sensitive_dir(&t) else {
        return;
    };
    write(d.join("readme.md"), "lower");

    let renamed = rename(&d.join("readme.md"), "README.md").unwrap();

    assert_eq!(read(&renamed), "lower");
    assert!(!d.join("readme.md").exists());
}

#[cfg(windows)]
#[test]
fn case_only_rename_of_a_folder_keeps_its_contents() {
    let t = tmp();
    let docs = dir(t.path().join("docs"));
    write(docs.join("f.txt"), "f");

    let renamed = rename(&docs, "Docs").unwrap();

    let names: Vec<_> = fs::read_dir(t.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(names, ["Docs"]);
    assert_eq!(read(renamed.join("f.txt")), "f");
}

fn names_in(d: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(d)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn rename_onto_a_hard_link_of_the_same_file_is_refused_without_moving_it() {
    let t = tmp();
    let d = dir(t.path().join("d"));
    write(d.join("a"), "data");
    fs::hard_link(d.join("a"), d.join("b")).unwrap();

    let err = rename(&d.join("a"), "b").unwrap_err();

    assert_eq!(err.kind(), ErrorKind::AlreadyExists);
    assert_eq!(names_in(&d), ["a", "b"]);
}

#[test]
fn case_only_rename_between_hard_links_is_refused() {
    let t = tmp();
    let Some(d) = case_sensitive_dir(&t) else {
        return;
    };
    write(d.join("readme.md"), "data");
    fs::hard_link(d.join("readme.md"), d.join("README.md")).unwrap();

    assert!(rename(&d.join("readme.md"), "README.md").is_err());
    assert_eq!(names_in(&d), ["README.md", "readme.md"]);
}

#[test]
fn rename_in_place_puts_the_old_name_back_when_finishing_fails() {
    let t = tmp();
    let d = dir(t.path().join("d"));
    write(d.join("a"), "mine");
    // Stands in for a case-folding volume refusing the final step.
    write(d.join("b"), "theirs");

    let err = rename_in_place(&d, &d.join("a"), &d.join("b")).unwrap_err();

    assert_eq!(err.kind(), ErrorKind::AlreadyExists);
    assert_eq!(names_in(&d), ["a", "b"], "no temporary name left behind");
    assert_eq!(read(d.join("a")), "mine");
    assert_eq!(read(d.join("b")), "theirs");
}

#[cfg(windows)]
#[test]
fn names_windows_would_change_are_rejected() {
    let t = tmp();
    let a = t.path().join("a.txt");
    write(&a, "a");
    for bad in ["a.txt.", "a.txt ", "CON", "nul.txt", "com1", "LPT9.log"] {
        let err = rename(&a, bad).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::InvalidInput, "{bad}");
        assert!(create_folder(t.path(), bad).is_err(), "{bad}");
    }
    for good in ["com", "COM0", "console", ".env", "a.b"] {
        let renamed = rename(&t.path().join(names_in(t.path())[0].clone()), good).unwrap();
        assert!(renamed.exists(), "{good}");
    }
    assert_eq!(read(t.path().join("a.b")), "a");
}

// ------------------------------------- 8. Links and folder-into-itself (#62)

fn junction(link: &Path, target: &Path) {
    let ok = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .output()
        .unwrap()
        .status
        .success();
    assert!(ok, "mklink /J failed");
}

#[test]
fn folder_into_itself_is_refused_through_dot_dot_components() {
    let t = tmp();
    let src = dir(t.path().join("src"));
    dir(src.join("inner"));
    let sneaky = src.join("inner").join("..").join("inner");

    assert!(copy_into(&one(&src), &sneaky).is_err());
    assert!(move_into(&one(&src), &sneaky).is_err());
    assert_eq!(fs::read_dir(src.join("inner")).unwrap().count(), 0);
}

#[cfg(windows)]
#[test]
fn folder_into_itself_is_refused_through_a_verbatim_prefix() {
    let t = tmp();
    let src = dir(t.path().join("src"));
    dir(src.join("inner"));
    let sneaky = PathBuf::from(format!(r"\\?\{}", src.join("inner").display()));
    assert!(sneaky.is_dir());

    assert!(copy_into(&one(&src), &sneaky).is_err());
    assert_eq!(fs::read_dir(src.join("inner")).unwrap().count(), 0);
}

#[cfg(windows)]
#[test]
fn folder_into_itself_is_refused_through_a_junction() {
    let t = tmp();
    let src = dir(t.path().join("src"));
    dir(src.join("inner"));
    // Elsewhere\door leads into src\inner.
    let elsewhere = dir(t.path().join("elsewhere"));
    junction(&elsewhere.join("door"), &src.join("inner"));

    assert!(copy_into(&one(&src), &elsewhere.join("door")).is_err());
    assert_eq!(fs::read_dir(src.join("inner")).unwrap().count(), 0);
}

#[cfg(windows)]
#[test]
fn copy_skips_a_junction_loop_instead_of_recursing() {
    let t = tmp();
    let src = dir(t.path().join("src"));
    write(src.join("f.txt"), "f");
    junction(&src.join("loop"), &src);
    let dest = dir(t.path().join("dest"));

    let out = copy_into(&one(&src), &dest).unwrap();

    assert_eq!(out.skipped, [src.join("loop")]);
    assert_eq!(read(dest.join("src").join("f.txt")), "f");
    assert!(fs::symlink_metadata(dest.join("src").join("loop")).is_err());
}

#[cfg(windows)]
#[test]
fn copy_never_pulls_in_data_through_a_junction_to_outside() {
    let t = tmp();
    let outside = dir(t.path().join("outside"));
    write(outside.join("secret.txt"), "outside");
    let src = dir(t.path().join("src"));
    write(src.join("f.txt"), "f");
    junction(&src.join("door"), &outside);
    let dest = dir(t.path().join("dest"));

    let out = copy_into(&one(&src), &dest).unwrap();

    assert_eq!(out.skipped, [src.join("door")]);
    assert!(!dest.join("src").join("door").exists());
    assert_eq!(read(outside.join("secret.txt")), "outside");
}

#[cfg(windows)]
#[test]
fn a_junction_itself_can_be_moved_within_a_volume() {
    let t = tmp();
    let target = dir(t.path().join("target"));
    write(target.join("f.txt"), "f");
    junction(&t.path().join("door"), &target);
    let dest = dir(t.path().join("dest"));

    let moved = move_into(&one(&t.path().join("door")), &dest).unwrap();

    assert_eq!(moved.paths, [dest.join("door")]);
    assert_eq!(read(dest.join("door").join("f.txt")), "f");
    assert_eq!(read(target.join("f.txt")), "f");
}

#[cfg(unix)]
#[test]
fn copy_recreates_links_instead_of_following_them() {
    let t = tmp();
    let outside = dir(t.path().join("outside"));
    write(outside.join("secret.txt"), "outside");
    let src = dir(t.path().join("src"));
    std::os::unix::fs::symlink(&outside, src.join("door")).unwrap();
    std::os::unix::fs::symlink(&src, src.join("loop")).unwrap();
    let dest = dir(t.path().join("dest"));

    let out = copy_into(&one(&src), &dest).unwrap();

    assert!(out.skipped.is_empty(), "{:?}", out.skipped);
    assert_eq!(
        fs::read_link(dest.join("src").join("door")).unwrap(),
        outside
    );
    assert_eq!(fs::read_link(dest.join("src").join("loop")).unwrap(), src);
}

#[cfg(windows)]
#[test]
fn move_into_itself_is_refused_through_a_junction() {
    let t = tmp();
    let src = dir(t.path().join("src"));
    write(src.join("f.txt"), "f");
    dir(src.join("inner"));
    let elsewhere = dir(t.path().join("elsewhere"));
    junction(&elsewhere.join("door"), &src.join("inner"));

    assert!(move_into(&one(&src), &elsewhere.join("door")).is_err());
    assert_eq!(read(src.join("f.txt")), "f");
}

#[cfg(windows)]
#[test]
fn move_onto_its_own_parent_spelled_differently_is_a_no_op() {
    let t = tmp();
    let parent = dir(t.path().join("Parent"));
    write(parent.join("f.txt"), "f");
    let respelled = t.path().join("PARENT");

    let moved = move_into(&one(&parent.join("f.txt")), &respelled).unwrap();

    assert_eq!(moved.paths, [parent.join("f.txt")]);
    assert_eq!(names_in(&parent), ["f.txt"]);
}
