//! The fixed and removable drives, for the space breakdown's drive picker.

use windows::Win32::Storage::FileSystem::{GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives};
use windows::Win32::System::Diagnostics::Debug::{SEM_FAILCRITICALERRORS, SetErrorMode};
use windows::core::PCWSTR;

/// `DRIVE_REMOVABLE` and `DRIVE_FIXED` from `GetDriveTypeW`.
const REMOVABLE: u32 = 2;
const FIXED: u32 = 3;

/// A drive's root with its capacity and free space, in bytes.
pub struct Drive {
    pub path: String,
    pub total: u64,
    pub free: u64,
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
