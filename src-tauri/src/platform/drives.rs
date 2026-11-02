//! The fixed and removable drives, for the space breakdown's drive picker.

use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
use windows::Win32::Storage::FileSystem::{
    GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives, GetVolumeNameForVolumeMountPointW,
};
use windows::Win32::System::Diagnostics::Debug::{SEM_FAILCRITICALERRORS, SetErrorMode};
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
    // SAFETY: only sets this process's error mode, and it is put back below.
    let before = unsafe { SetErrorMode(SEM_FAILCRITICALERRORS) };
    let left = bin_room_of(&wide);
    // SAFETY: restores the value returned above.
    unsafe { SetErrorMode(before) };
    left
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
}
