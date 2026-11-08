//! "Open folders with Folio": the opt-in setting that makes Folio the program other programs
//! and the system hand a folder to.
//!
//! No system lets an app *become* the file manager outright; the shell owns that. What each
//! one allows is a per-user entry for "open a folder", and that is all this touches:
//!
//! - **Windows:** the current user's `Directory` and `Drive` classes get an "Open in Folio"
//!   verb and make it the default one. Explorer's own windows, `Win+E` and the taskbar still
//!   open Explorer.
//! - **Linux:** a desktop entry for `inode/directory`, set as the user's default with
//!   `xdg-mime`; the previous default is put back when the setting is turned off.
//! - **macOS:** Launch Services' default handler for `public.folder`. Finder itself, and
//!   "Show in Finder", stay as they are.
//!
//! It is off by default and fully undone by turning it off. Nothing here needs admin rights,
//! and nothing here touches a file or folder, only where the system sends a folder to open.

use std::path::{Path, PathBuf};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

/// Whether Folio is the program folders open with now.
///
/// # Errors
/// If the system could not be asked.
#[cfg_attr(
    any(windows, target_os = "macos"),
    expect(
        clippy::unnecessary_wraps,
        reason = "only the Linux version has to ask a program, and can fail"
    )
)]
pub fn is_default() -> Result<bool, String> {
    #[cfg(windows)]
    {
        Ok(windows::is_default())
    }
    #[cfg(target_os = "linux")]
    {
        linux::is_default()
    }
    #[cfg(target_os = "macos")]
    {
        Ok(macos::is_default())
    }
    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    {
        Err("This system has no way to choose the program folders open with".into())
    }
}

/// Makes Folio the program folders open with, or puts back what was there before. `data` is
/// the app's own folder, where what was replaced is kept so it can be put back.
///
/// # Errors
/// If the system refused, or another program already holds the setting and would be lost.
pub fn set_default(on: bool, data: &Path) -> Result<(), String> {
    let exe = current_program()?;
    #[cfg(windows)]
    {
        let _ = data;
        if on {
            windows::enable(&exe)
        } else {
            windows::disable()
        }
    }
    #[cfg(target_os = "linux")]
    {
        if on {
            linux::enable(&exe, data)
        } else {
            linux::disable(data)
        }
    }
    #[cfg(target_os = "macos")]
    {
        let _ = &exe;
        if on {
            macos::enable(data)
        } else {
            macos::disable(data)
        }
    }
    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    {
        let _ = (on, data, exe);
        Err("This system has no way to choose the program folders open with".into())
    }
}

/// If the setting is on, makes sure it points at the Folio that is running: an app that was
/// moved or updated to another place would otherwise leave folders opening nothing. Quiet
/// when the setting is off or anything fails; this is only upkeep.
pub fn refresh() {
    let Ok(exe) = current_program() else { return };
    #[cfg(windows)]
    windows::refresh(&exe);
    #[cfg(target_os = "linux")]
    linux::refresh(&exe);
    #[cfg(not(any(windows, target_os = "linux")))]
    let _ = exe;
}

/// The program a folder is handed to. For an `AppImage` that is the image itself, not the
/// temporary place it unpacks to, which is gone when it closes.
fn current_program() -> Result<PathBuf, String> {
    if let Some(image) = std::env::var_os("APPIMAGE").filter(|v| !v.is_empty()) {
        return Ok(PathBuf::from(image));
    }
    std::env::current_exe().map_err(|e| format!("Can't tell where Folio is: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_running_program_is_found() {
        let exe = current_program().unwrap();
        assert!(exe.is_absolute());
    }
}
