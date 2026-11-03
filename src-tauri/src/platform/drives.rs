//! The fixed and removable drives, for the space breakdown's drive picker.

use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, HANDLE};
use windows::Win32::Storage::FileSystem::{
    FILE_FLAG_BACKUP_SEMANTICS, GETFINALPATHNAMEBYHANDLE_FLAGS, GetDiskFreeSpaceExW, GetDriveTypeW,
    GetFinalPathNameByHandleW, GetLogicalDrives, GetVolumeNameForVolumeMountPointW,
    GetVolumePathNamesForVolumeNameW, QueryDosDeviceW, VOLUME_NAME_DOS, VOLUME_NAME_GUID,
};
use windows::Win32::System::Diagnostics::Debug::{
    SEM_FAILCRITICALERRORS, SetErrorMode, SetThreadErrorMode, THREAD_ERROR_MODE,
};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD, RegGetValueW,
};
use windows::Win32::UI::Shell::{SHQUERYRBINFO, SHQueryRecycleBinW};
use windows::core::{PCWSTR, w};

/// `DRIVE_REMOVABLE` and `DRIVE_FIXED` from `GetDriveTypeW`.
const REMOVABLE: u32 = 2;
const FIXED: u32 = 3;

/// A drive's root with its capacity and free space, in bytes.
pub struct Drive {
    pub path: String,
    pub total: u64,
    pub free: u64,
}

/// How much more the Recycle Bin of the fixed drive with this letter can take, in bytes, or
/// `None` where a delete would be for good: on a removable or network drive, when the
/// bin is turned off for the drive or by policy, or when its setting can't be read (then
/// nothing is assumed). Windows throws a bin's oldest items away when it overflows, so
/// what is already in it counts.
pub fn bin_room(letter: char) -> Option<u64> {
    let root = format!("{}:\\", letter.to_ascii_uppercase());
    let wide: Vec<u16> = root.encode_utf16().chain(std::iter::once(0)).collect();
    let mut before = THREAD_ERROR_MODE(0);
    // SAFETY: sets this thread's error mode; `before` is a live local, and it is put back below.
    let _ = unsafe { SetThreadErrorMode(SEM_FAILCRITICALERRORS, Some(&raw mut before)) };
    let left = bin_room_of(&wide);
    // SAFETY: restores the value read above.
    let _ = unsafe { SetThreadErrorMode(before, None) };
    left
}

/// The volume an item is really on, and the Recycle Bin room that volume has.
///
/// The item's folder is opened and its final path read, so a junction or a directory
/// symlink into another volume, a volume mounted into a folder and a `\\?\` spelling are
/// judged by the volume the item sits on, not by the drive letter it was reached through. A
/// drive made with `subst` has no bin of its own. What can't be resolved (a network path, a
/// folder that is gone) is judged by its drive letter, or has no bin.
pub struct BinVolume {
    /// Names the volume whatever the spelling of the path, so jobs share one count of its
    /// room.
    pub key: String,
    /// `None` where a delete would be for good.
    pub room: Option<u64>,
}

/// Which volume `path` is on, and how much more its bin can take.
pub fn bin_volume(path: &std::path::Path) -> BinVolume {
    // Only this thread's error mode is touched: jobs on other lanes ask at the same time.
    let mut before = THREAD_ERROR_MODE(0);
    // SAFETY: sets this thread's error mode; `before` is a live local, and it is put back below.
    let _ = unsafe { SetThreadErrorMode(SEM_FAILCRITICALERRORS, Some(&raw mut before)) };
    let found = bin_volume_of(path);
    // SAFETY: restores the value read above.
    let _ = unsafe { SetThreadErrorMode(before, None) };
    found
}

fn bin_volume_of(path: &std::path::Path) -> BinVolume {
    let letter = lexical_letter(path);
    if let Some(l) = letter
        && is_subst(l)
    {
        return BinVolume {
            key: format!("subst:{}", l.to_ascii_lowercase()),
            room: None,
        };
    }
    match volume_of(path) {
        Placed::Volume(guid, root) => BinVolume {
            key: guid,
            room: root.and_then(|r| bin_room_at(&r)),
        },
        // On another computer, or somewhere else with no bin, however it was reached.
        Placed::Elsewhere(key) => BinVolume { key, room: None },
        // Nothing of it exists to ask about: the drive letter is all there is to go by.
        Placed::Nowhere => match letter {
            Some(l) => BinVolume {
                key: format!("drive:{}", l.to_ascii_lowercase()),
                room: bin_room(l),
            },
            None => BinVolume {
                key: format!("other:{}", first_part(path)),
                room: None,
            },
        },
    }
}

/// How much more the bin of the volume mounted at `root` can take.
fn bin_room_at(root: &str) -> Option<u64> {
    let wide: Vec<u16> = root.encode_utf16().chain(std::iter::once(0)).collect();
    bin_room_of(&wide)
}

/// The drive letter at the start of a path (`C:\x` or `\\?\C:\x`).
fn lexical_letter(path: &std::path::Path) -> Option<char> {
    use std::path::{Component, Prefix};
    let Some(Component::Prefix(prefix)) = path.components().next() else {
        return None;
    };
    match prefix.kind() {
        Prefix::Disk(l) | Prefix::VerbatimDisk(l) => Some(char::from(l)),
        _ => None,
    }
}

fn first_part(path: &std::path::Path) -> String {
    path.components()
        .next()
        .map(|c| c.as_os_str().to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

/// Whether the drive letter is a `subst` drive: its DOS device points at a path
/// (`\??\C:\dir`) rather than at a volume.
fn is_subst(letter: char) -> bool {
    let name: Vec<u16> = format!("{}:", letter.to_ascii_uppercase())
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let mut target = [0u16; 1024];
    // SAFETY: `name` is NUL-terminated and `target` is a live buffer.
    let n = unsafe { QueryDosDeviceW(PCWSTR(name.as_ptr()), Some(&mut target)) } as usize;
    let first = target[..n.min(target.len())]
        .split(|&c| c == 0)
        .next()
        .unwrap_or(&[]);
    String::from_utf16_lossy(first).starts_with("\\??\\")
}

/// Where the folder holding an item really is.
enum Placed {
    /// A local volume: its `\\?\Volume{guid}\` name, and a place it is mounted, from where
    /// its bin is asked about.
    Volume(String, Option<String>),
    /// Not on a local volume (another computer's share, say): a name for it. It has no bin.
    Elsewhere(String),
    /// No folder of the path could be opened.
    Nowhere,
}

/// The final path (links followed) of an open item, in the form `flags` asks for.
fn final_path(file: &std::fs::File, flags: GETFINALPATHNAMEBYHANDLE_FLAGS) -> Option<String> {
    use std::os::windows::io::AsRawHandle;
    let handle = HANDLE(file.as_raw_handle());
    let mut buf = vec![0u16; 1024];
    loop {
        // SAFETY: the handle belongs to `file`, which outlives the call; `buf` is a live buffer.
        let n = unsafe { GetFinalPathNameByHandleW(handle, &mut buf, flags) } as usize;
        match n {
            0 => return None,
            n if n < buf.len() => return Some(String::from_utf16_lossy(&buf[..n])),
            // Too short: `n` is the size needed, with the NUL.
            n => buf = vec![0u16; n + 1],
        }
    }
}

/// Where the folder holding `path` is: the nearest one above it that exists is opened and
/// its final path read, which is what the shell is handed (junctions and symlinks followed).
fn volume_of(path: &std::path::Path) -> Placed {
    use std::os::windows::fs::OpenOptionsExt;
    // A folder that exists but can't be opened is passed over for the one above it; the trash
    // crate opens the parent the same way and fails the same way, so nothing is moved then.
    let Some(file) = path
        .ancestors()
        .skip(1)
        .chain(std::iter::once(path))
        .filter(|p| !p.as_os_str().is_empty())
        .find_map(|dir| {
            // No access is asked for, so a folder that can't be read can still be placed.
            std::fs::OpenOptions::new()
                .access_mode(0)
                .share_mode(7)
                .custom_flags(FILE_FLAG_BACKUP_SEMANTICS.0)
                .open(dir)
                .ok()
        })
    else {
        return Placed::Nowhere;
    };
    if let Some(text) = final_path(&file, VOLUME_NAME_GUID) {
        // `\\?\Volume{xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx}\` is 49 characters.
        if let Some(prefix) = text
            .get(..49)
            .filter(|p| p.starts_with("\\\\?\\Volume{") && p.ends_with("}\\"))
        {
            return Placed::Volume(prefix.to_lowercase(), mount_of(prefix));
        }
    }
    // No volume name (a share, or a volume the system can't name): the path says which.
    let Some(text) = final_path(&file, VOLUME_NAME_DOS) else {
        return Placed::Nowhere;
    };
    let bytes = text.as_bytes();
    if text.starts_with("\\\\?\\")
        && bytes.get(5) == Some(&b':')
        && bytes.get(4).is_some_and(u8::is_ascii_alphabetic)
    {
        let root = format!("{}:\\", char::from(bytes[4]).to_ascii_uppercase());
        // Both ways of naming a volume must give one key.
        return match volume_guid(&root) {
            Some(guid) => Placed::Volume(guid, Some(root)),
            None => Placed::Elsewhere(format!("drive:{}", root.to_lowercase())),
        };
    }
    let lower = text.to_lowercase();
    let share: Vec<&str> = lower
        .trim_start_matches("\\\\?\\")
        .split('\\')
        .take(3)
        .collect();
    Placed::Elsewhere(format!("share:{}", share.join("\\")))
}

/// The `\\?\Volume{guid}\` name (lower case) of the volume mounted at `root`.
fn volume_guid(root: &str) -> Option<String> {
    let wide: Vec<u16> = root.encode_utf16().chain(std::iter::once(0)).collect();
    let mut name = [0u16; 64];
    // SAFETY: `wide` is NUL-terminated and `name` is a live buffer.
    unsafe { GetVolumeNameForVolumeMountPointW(PCWSTR(wide.as_ptr()), &mut name) }.ok()?;
    let end = name.iter().position(|&c| c == 0)?;
    Some(String::from_utf16_lossy(&name[..end]).to_lowercase())
}

/// A place the volume named `guid` (`\\?\Volume{guid}\`) is mounted: a drive letter if it
/// has one, else a folder.
fn mount_of(guid: &str) -> Option<String> {
    let wide: Vec<u16> = guid.encode_utf16().chain(std::iter::once(0)).collect();
    let mut names = [0u16; 1024];
    let mut len = 0u32;
    // SAFETY: `wide` is NUL-terminated; `names` and `len` are live locals.
    let listed = unsafe {
        GetVolumePathNamesForVolumeNameW(PCWSTR(wide.as_ptr()), Some(&mut names), &raw mut len)
    };
    if listed.is_err() {
        return None;
    }
    let mounts: Vec<String> = names[..(len as usize).min(names.len())]
        .split(|&c| c == 0)
        .filter(|m| !m.is_empty())
        .map(String::from_utf16_lossy)
        .collect();
    mounts
        .iter()
        .find(|m| m.len() == 3 && m.as_bytes()[1] == b':')
        .or_else(|| mounts.first())
        .cloned()
}

/// `DRIVE_REMOTE` from `GetDriveTypeW`.
const REMOTE: u32 = 4;

/// Whether `path` is on another computer, where reading a whole folder tree is slow and
/// not worth doing unasked: a `\\server\share` path, or a drive letter mapped to a share.
pub fn is_network(path: &std::path::Path) -> bool {
    let text = path.to_string_lossy().replace('/', "\\");
    if let Some(rest) = text.strip_prefix("\\\\") {
        // `\\?\C:\` and `\\.\C:` name volumes of this computer; `\\?\UNC\server` and
        // `\\server` name another one.
        let local = rest
            .strip_prefix("?\\")
            .or_else(|| rest.strip_prefix(".\\"))
            .is_some_and(|r| r.as_bytes().get(1) == Some(&b':'));
        return !local;
    }
    let bytes = text.as_bytes();
    if bytes.len() < 2 || bytes[1] != b':' || !bytes[0].is_ascii_alphabetic() {
        return false;
    }
    let root = format!("{}:\\", char::from(bytes[0]).to_ascii_uppercase());
    let wide: Vec<u16> = root.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: `wide` is NUL-terminated and outlives the call.
    unsafe { GetDriveTypeW(PCWSTR(wide.as_ptr())) == REMOTE }
}

/// Bytes the current user may still write on the volume holding `path`.
pub fn free_for(path: &std::path::Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut free = 0u64;
    // SAFETY: `wide` is NUL-terminated and outlives the call; the out-pointer refers to a live local.
    unsafe { GetDiskFreeSpaceExW(PCWSTR(wide.as_ptr()), Some(&raw mut free), None, None) }.ok()?;
    Some(free)
}

fn bin_room_of(root: &[u16]) -> Option<u64> {
    // SAFETY: `root` is NUL-terminated and outlives the call.
    let kind = unsafe { GetDriveTypeW(PCWSTR(root.as_ptr())) };
    if kind != FIXED {
        return None;
    }
    let mut total = 0u64;
    // SAFETY: `root` is NUL-terminated; the out-pointer refers to a live local.
    unsafe { GetDiskFreeSpaceExW(PCWSTR(root.as_ptr()), None, Some(&raw mut total), None) }.ok()?;
    let setup = bin_setup(root)?;
    let mut info = SHQUERYRBINFO {
        cbSize: u32::try_from(size_of::<SHQUERYRBINFO>()).ok()?,
        ..Default::default()
    };
    // SAFETY: `root` is NUL-terminated and `info` is a live local with `cbSize` set.
    unsafe { SHQueryRecycleBinW(PCWSTR(root.as_ptr()), &raw mut info) }.ok()?;
    room(total, &setup, u64::try_from(info.i64Size).unwrap_or(0))
}

/// What the drive's Recycle Bin is set to, and the policies on top of it. A value that
/// isn't there is the default; one that can't be read is not guessed.
#[derive(Default)]
struct BinSetup {
    /// The drive's own size limit, in MB.
    max_mb: Option<u64>,
    /// "Don't move files to the Recycle Bin" for this drive.
    nuke_on_delete: bool,
    /// A policy that turns the bin off everywhere.
    no_recycle_policy: bool,
    /// A policy that sets the size limit as a percentage of the drive.
    size_policy_percent: Option<u64>,
}

/// How much more the bin can take, given the drive's size, its settings and what the bin
/// already holds. `None`: it takes nothing, a delete is for good.
fn room(total: u64, setup: &BinSetup, used: u64) -> Option<u64> {
    if total == 0 || setup.nuke_on_delete || setup.no_recycle_policy {
        return None;
    }
    // A policy beats the drive's own limit. With neither, the bin keeps to a share of the
    // drive; this guess is on the careful side of what Windows picks.
    let capacity = match (setup.size_policy_percent, setup.max_mb) {
        (Some(percent), _) => total / 100 * percent.min(100),
        (None, Some(mb)) => mb.saturating_mul(1 << 20),
        (None, None) => total / 20,
    };
    Some(capacity.saturating_sub(used))
}

/// A DWORD from the current user's or the machine's registry: `Ok(None)` where it isn't set,
/// `Err` where it couldn't be read for another reason.
fn dword(hive: HKEY, key: &str, value: PCWSTR) -> Result<Option<u32>, ()> {
    let key: Vec<u16> = key.encode_utf16().chain(std::iter::once(0)).collect();
    let (mut data, mut size) = (0u32, 4u32);
    // SAFETY: `key` and `value` are NUL-terminated; `data` and `size` are live locals and
    // `size` says `data` holds four bytes.
    let status = unsafe {
        RegGetValueW(
            hive,
            PCWSTR(key.as_ptr()),
            value,
            RRF_RT_REG_DWORD,
            None,
            Some((&raw mut data).cast()),
            Some(&raw mut size),
        )
    };
    if status == ERROR_SUCCESS {
        Ok(Some(data))
    } else if status == ERROR_FILE_NOT_FOUND {
        Ok(None)
    } else {
        Err(())
    }
}

fn bin_setup(root: &[u16]) -> Option<BinSetup> {
    // `\\?\Volume{guid}\`
    let mut name = [0u16; 64];
    // SAFETY: `root` is NUL-terminated and `name` is a live buffer.
    unsafe { GetVolumeNameForVolumeMountPointW(PCWSTR(root.as_ptr()), &mut name) }.ok()?;
    let text = String::from_utf16_lossy(&name[..name.iter().position(|&c| c == 0).unwrap_or(0)]);
    let from = text.find('{')?;
    let guid = &text[from..=from + text[from..].find('}')?];
    let explorer = "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer";
    let volume = format!("{explorer}\\BitBucket\\Volume\\{guid}");
    let policies = "Software\\Microsoft\\Windows\\CurrentVersion\\Policies\\Explorer";
    let user = HKEY_CURRENT_USER;
    let machine = HKEY_LOCAL_MACHINE;
    let policy = |name: PCWSTR| -> Result<Option<u32>, ()> {
        // Set for the user or for the machine: either one counts.
        Ok(dword(user, policies, name)?.or(dword(machine, policies, name)?))
    };
    Some(BinSetup {
        max_mb: dword(user, &volume, w!("MaxCapacity")).ok()?.map(u64::from),
        nuke_on_delete: dword(user, &volume, w!("NukeOnDelete"))
            .ok()?
            .is_some_and(|v| v != 0),
        no_recycle_policy: policy(w!("NoRecycleFiles")).ok()?.is_some_and(|v| v != 0),
        size_policy_percent: policy(w!("RecycleBinSize")).ok()?.map(u64::from),
    })
}

pub fn list() -> Vec<Drive> {
    // An empty card reader would otherwise raise a "no disk" dialog.
    // SAFETY: only sets this process's error mode, and it is put back below.
    let before = unsafe { SetErrorMode(SEM_FAILCRITICALERRORS) };
    let drives = list_drives();
    // SAFETY: restores the value returned above.
    unsafe { SetErrorMode(before) };
    drives
}

fn list_drives() -> Vec<Drive> {
    // SAFETY: a plain query with no arguments.
    let mask = unsafe { GetLogicalDrives() };
    (0..26u8)
        .filter(|bit| mask & (1 << bit) != 0)
        .filter_map(|bit| {
            let path = format!("{}:\\", char::from(b'A' + bit));
            let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
            // SAFETY: `wide` is NUL-terminated and outlives the call.
            let kind = unsafe { GetDriveTypeW(PCWSTR(wide.as_ptr())) };
            if kind != FIXED && kind != REMOVABLE {
                return None;
            }
            let (mut total, mut free) = (0u64, 0u64);
            // SAFETY: `wide` is NUL-terminated; both out-pointers refer to live locals.
            let ok = unsafe {
                GetDiskFreeSpaceExW(
                    PCWSTR(wide.as_ptr()),
                    None,
                    Some(&raw mut total),
                    Some(&raw mut free),
                )
            };
            // An empty card reader has no size: not worth listing.
            (ok.is_ok() && total > 0).then_some(Drive { path, total, free })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const GB: u64 = 1 << 30;

    fn key_of(path: &std::path::Path) -> String {
        bin_volume(path).key
    }

    #[test]
    fn every_spelling_of_a_path_names_one_volume() {
        let dir = tempfile::tempdir().unwrap();
        let plain = dir.path().join("f");
        let verbatim = std::path::PathBuf::from(format!("\\\\?\\{}", plain.display()));
        let key = key_of(&plain);
        assert!(key.starts_with("\\\\?\\volume{"), "{key}");
        assert_eq!(key_of(&verbatim), key);
        // A file that is not there yet is judged by the folder it would be in.
        assert_eq!(key_of(&dir.path().join("a").join("b")), key_of(dir.path()));
    }

    #[test]
    fn a_path_through_a_junction_is_judged_by_where_it_leads() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real");
        std::fs::create_dir(&real).unwrap();
        let link = dir.path().join("link");
        let made = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(&real)
            .output()
            .unwrap();
        assert!(made.status.success(), "mklink failed");
        // Reaching the folder through the junction gives the volume it is really on.
        assert_eq!(key_of(&link.join("f")), key_of(&real.join("f")));
    }

    #[test]
    fn a_long_path_names_the_same_volume_as_a_short_one() {
        let dir = tempfile::tempdir().unwrap();
        let mut deep = std::path::PathBuf::from(format!("\\\\?\\{}", dir.path().display()));
        for _ in 0..6 {
            deep.push("d".repeat(200));
        }
        std::fs::create_dir_all(&deep).unwrap();
        assert!(deep.as_os_str().len() > 1024);
        assert_eq!(key_of(&deep.join("f")), key_of(&dir.path().join("f")));
    }

    #[test]
    fn a_link_to_a_share_has_no_bin() {
        // The shell is handed the share's path, whatever drive the link is on.
        let dir = tempfile::tempdir().unwrap();
        let text = dir.path().display().to_string();
        let Some((letter, rest)) = text.split_once(":\\") else {
            return;
        };
        let share = format!("\\\\localhost\\{letter}$\\{rest}");
        if !std::path::Path::new(&share).is_dir() {
            println!("skipped: {share} is not reachable");
            return;
        }
        let link = dir.path().join("link");
        let made = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/D"])
            .arg(&link)
            .arg(&share)
            .output()
            .unwrap();
        if !made.status.success() {
            println!("skipped: no right to make symbolic links");
            return;
        }
        let found = bin_volume(&link.join("f"));
        assert!(found.room.is_none(), "{}", found.key);
    }

    #[test]
    fn a_subst_drive_has_no_bin() {
        // SAFETY: a plain query with no arguments.
        let used = unsafe { GetLogicalDrives() };
        let Some(bit) = (2..26u8).rev().find(|bit| used & (1 << bit) == 0) else {
            return;
        };
        let drive = format!("{}:", char::from(b'A' + bit));
        let dir = tempfile::tempdir().unwrap();
        let ran = std::process::Command::new("subst")
            .arg(&drive)
            .arg(dir.path())
            .status();
        if !ran.is_ok_and(|s| s.success()) {
            return;
        }
        let found = bin_volume(std::path::Path::new(&format!("{drive}\\file")));
        let _ = std::process::Command::new("subst")
            .args([&drive, "/d"])
            .status();
        assert!(found.room.is_none());
        assert!(found.key.starts_with("subst:"), "{}", found.key);
    }

    #[test]
    fn a_bin_set_to_delete_at_once_takes_nothing() {
        let setup = BinSetup {
            nuke_on_delete: true,
            ..BinSetup::default()
        };
        assert_eq!(room(100 * GB, &setup, 0), None);
    }

    #[test]
    fn a_policy_that_turns_the_bin_off_beats_the_drives_setting() {
        let setup = BinSetup {
            no_recycle_policy: true,
            max_mb: Some(10_000),
            ..BinSetup::default()
        };
        assert_eq!(room(100 * GB, &setup, 0), None);
    }

    #[test]
    fn the_drives_limit_is_in_megabytes() {
        let setup = BinSetup {
            max_mb: Some(2048),
            ..BinSetup::default()
        };
        assert_eq!(room(100 * GB, &setup, 0), Some(2 * GB));
    }

    #[test]
    fn a_size_policy_beats_the_drives_limit() {
        let setup = BinSetup {
            max_mb: Some(50_000),
            size_policy_percent: Some(10),
            ..BinSetup::default()
        };
        assert_eq!(room(100 * GB, &setup, 0), Some(10 * GB));
    }

    #[test]
    fn with_no_setting_the_bin_gets_a_twentieth_of_the_drive() {
        assert_eq!(room(100 * GB, &BinSetup::default(), 0), Some(5 * GB));
    }

    #[test]
    fn what_the_bin_holds_is_taken_off_and_never_underflows() {
        let setup = BinSetup::default();
        assert_eq!(room(100 * GB, &setup, 2 * GB), Some(3 * GB));
        assert_eq!(room(100 * GB, &setup, 9 * GB), Some(0));
    }

    #[test]
    fn a_drive_of_no_size_has_no_bin() {
        assert_eq!(room(0, &BinSetup::default(), 0), None);
    }

    #[test]
    fn share_paths_are_network_and_local_spellings_are_not() {
        use std::path::Path;
        for share in [
            "\\\\server\\share\\a",
            "//server/share/a",
            "\\\\?\\UNC\\server\\share\\a",
            "\\\\wsl.localhost\\Ubuntu\\home",
        ] {
            assert!(is_network(Path::new(share)), "{share}");
        }
        for local in [
            "\\\\?\\C:\\Users",
            "\\\\.\\C:",
            "relative\\dir",
            "folio:bin",
            "",
        ] {
            assert!(!is_network(Path::new(local)), "{local}");
        }
        // A fixed drive is not a share. The system drive is always there on Windows.
        assert!(!is_network(Path::new("C:\\Windows")));
    }
}
