//! Windows: an "Open in Folio" verb on the current user's `Directory` and `Drive` classes,
//! made the default one. Under `HKCU\Software\Classes`, so no admin rights are needed and
//! the machine-wide entries are never touched: removing ours restores the merged view.

use std::ffi::c_void;
use std::path::Path;

use ::windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, ERROR_SUCCESS};
use ::windows::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegDeleteTreeW, RegGetValueW,
    RegSetKeyValueW,
};
use ::windows::core::PCWSTR;

/// The classes folders and drives are opened through.
const CLASSES: [&str; 2] = ["Directory", "Drive"];

/// The verb's name; `shell`'s default value names the verb a double click runs.
const VERB: &str = "folio";

/// What the verb is called in a menu.
const LABEL: &str = "Open in Folio";

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn shell_key(class: &str) -> String {
    format!("Software\\Classes\\{class}\\shell")
}

/// The command that opens a folder: the program and the folder as one argument. A drive's
/// root ends in a backslash, which the `\"` rule of command lines would turn into a quote;
/// the launch code puts that right (see `launch::folders_from_args`).
pub fn command_line(exe: &Path) -> String {
    format!("\"{}\" \"%1\"", exe.display())
}

/// Writes a string value; `name` `None` is the key's default value. Creates the key.
fn set_string(key: &str, name: Option<&str>, value: &str) -> Result<(), String> {
    let key = wide(key);
    let name = name.map(wide);
    let data = wide(value);
    // SAFETY: `key`, `name` and `data` are NUL-terminated and outlive the call; `data` is
    // passed with its byte length, terminator included, as `REG_SZ` wants.
    let status = unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            PCWSTR(key.as_ptr()),
            name.as_ref().map_or(PCWSTR::null(), |n| PCWSTR(n.as_ptr())),
            REG_SZ.0,
            Some(data.as_ptr().cast::<c_void>()),
            u32::try_from(data.len() * 2).map_err(|e| e.to_string())?,
        )
    };
    if status == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(format!("The registry refused the change ({})", status.0))
    }
}

/// A string value, `None` where there is none.
fn get_string(key: &str, name: Option<&str>) -> Option<String> {
    let key = wide(key);
    let name = name.map(wide);
    let mut buf = [0u16; 1024];
    let mut bytes = u32::try_from(size_of_val(&buf)).ok()?;
    // SAFETY: `key` and `name` are NUL-terminated; `buf` is a live local and `bytes` says
    // how many bytes it holds.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(key.as_ptr()),
            name.as_ref().map_or(PCWSTR::null(), |n| PCWSTR(n.as_ptr())),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr().cast::<c_void>()),
            Some(&raw mut bytes),
        )
    };
    if status != ERROR_SUCCESS {
        return None;
    }
    // The size counts the terminator.
    let units = (bytes as usize / 2).saturating_sub(1);
    Some(String::from_utf16_lossy(&buf[..units.min(buf.len())]))
}

/// Whether the default verb of every class is ours.
pub fn is_default() -> bool {
    CLASSES
        .iter()
        .all(|class| get_string(&shell_key(class), None).as_deref() == Some(VERB))
}

/// Makes the verb the default for folders and drives. Refuses when another program already
/// holds the default verb for the user, so it is not lost.
pub fn enable(exe: &Path) -> Result<(), String> {
    for class in CLASSES {
        let shell = shell_key(class);
        if let Some(other) = get_string(&shell, None).filter(|v| !v.is_empty() && v != VERB) {
            return Err(format!(
                "Another program (\"{other}\") already opens {}. Remove it there first.",
                if class == "Drive" {
                    "drives"
                } else {
                    "folders"
                }
            ));
        }
    }
    for class in CLASSES {
        let shell = shell_key(class);
        let verb = format!("{shell}\\{VERB}");
        let result = set_string(&format!("{verb}\\command"), None, &command_line(exe))
            .and_then(|()| set_string(&verb, None, LABEL))
            .and_then(|()| set_string(&shell, None, VERB));
        if let Err(e) = result {
            // Half a change is worse than none.
            let _ = disable();
            return Err(e);
        }
    }
    Ok(())
}

/// Removes the verb and the default that names it. Only what is ours: a default that names
/// another verb is left alone.
pub fn disable() -> Result<(), String> {
    let mut failed = None;
    for class in CLASSES {
        let shell = shell_key(class);
        if get_string(&shell, None).as_deref() == Some(VERB) {
            let key = wide(&shell);
            // SAFETY: `key` is NUL-terminated; a null value name is the default value.
            let status = unsafe {
                RegDeleteKeyValueW(HKEY_CURRENT_USER, PCWSTR(key.as_ptr()), PCWSTR::null())
            };
            keep_error(status, &mut failed);
        }
        let verb = wide(&format!("{shell}\\{VERB}"));
        // SAFETY: `verb` is NUL-terminated and outlives the call.
        let status = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(verb.as_ptr())) };
        keep_error(status, &mut failed);
    }
    failed.map_or(Ok(()), Err)
}

/// Notes a failure other than "it was not there".
fn keep_error(status: ::windows::Win32::Foundation::WIN32_ERROR, failed: &mut Option<String>) {
    if status != ERROR_SUCCESS && status != ERROR_FILE_NOT_FOUND && status != ERROR_PATH_NOT_FOUND {
        *failed = Some(format!("The registry refused the change ({})", status.0));
    }
}

/// Points the verb at `exe` when the setting is on and it points elsewhere.
pub fn refresh(exe: &Path) {
    if !is_default() {
        return;
    }
    let want = command_line(exe);
    for class in CLASSES {
        let key = format!("{}\\{VERB}\\command", shell_key(class));
        if get_string(&key, None).as_deref() != Some(want.as_str()) {
            let _ = set_string(&key, None, &want);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_command_quotes_the_program_and_the_folder() {
        let line = command_line(Path::new("C:\\Program Files\\Folio\\folio.exe"));
        assert_eq!(line, "\"C:\\Program Files\\Folio\\folio.exe\" \"%1\"");
    }

    /// Turns the setting on and off in the real registry, so it is run by hand:
    /// `cargo test default_manager -- --ignored --nocapture`.
    #[test]
    #[ignore = "changes the current user's registry (and puts it back)"]
    fn the_setting_turns_on_and_off_for_real() {
        let was_on = is_default();
        let exe = Path::new(r"C:\Folio-test\folio.exe");
        if !was_on {
            enable(exe).unwrap();
            assert!(is_default());
            let key = format!(r"{}\{VERB}\command", shell_key("Directory"));
            assert_eq!(get_string(&key, None).unwrap(), command_line(exe));
            // Turning it on again changes nothing.
            enable(exe).unwrap();
            disable().unwrap();
            assert!(!is_default());
            assert_eq!(get_string(&shell_key("Directory"), None), None);
            // Turning it off again is fine too.
            disable().unwrap();
        }
    }

    #[test]
    fn a_missing_setting_reads_as_off() {
        // Nothing sets the verb in a test run, so the answer is a plain "no".
        assert_eq!(
            get_string("Software\\Classes\\FolioNoSuchClass", None),
            None
        );
    }
}
