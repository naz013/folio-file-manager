//! Linux: a desktop entry that handles `inode/directory`, made the user's default with
//! `xdg-mime`. What was the default before is kept in the app's folder and put back when
//! the setting is turned off.

use std::path::{Path, PathBuf};
use std::process::Command;

/// The entry's file name, in the user's applications folder.
const ENTRY: &str = "folio-folders.desktop";

/// The type every file manager handles.
const FOLDER: &str = "inode/directory";

/// Where the previous default is kept, in the app's folder.
const KEPT: &str = "previous-folder-handler";

fn applications_dir() -> Result<PathBuf, String> {
    let base = std::env::var_os("XDG_DATA_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .ok_or("Can't find your applications folder")?;
    Ok(base.join("applications"))
}

/// A path as an `Exec` argument: quoted, with the characters the format reserves escaped
/// (the desktop entry format, "The Exec key"), and the backslashes doubled again as the
/// format's string values require.
pub fn exec_quote(path: &str) -> String {
    let mut out = String::from("\"");
    for c in path.chars() {
        match c {
            '"' | '`' | '$' | '\\' => {
                out.push_str("\\\\");
                out.push(c);
            }
            '%' => out.push_str("%%"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The desktop entry. `%F` hands over the folders as paths.
pub fn entry_text(exe: &Path) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Folio\n\
         Comment=Open folders in Folio\n\
         Exec={} %F\n\
         Icon=folio\n\
         Terminal=false\n\
         NoDisplay=true\n\
         MimeType={FOLDER};\n\
         Categories=Utility;FileManager;\n",
        exec_quote(&exe.to_string_lossy())
    )
}

/// What `xdg-mime` says now handles folders.
fn current_handler() -> Result<String, String> {
    let out = Command::new("xdg-mime")
        .args(["query", "default", FOLDER])
        .output()
        .map_err(|_| "xdg-utils is not installed, so the setting can't be changed".to_owned())?;
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

fn set_handler(entry: &str) -> Result<(), String> {
    let status = Command::new("xdg-mime")
        .args(["default", entry, FOLDER])
        .status()
        .map_err(|_| "xdg-utils is not installed, so the setting can't be changed".to_owned())?;
    if status.success() {
        Ok(())
    } else {
        Err("xdg-mime could not change the setting".into())
    }
}

pub fn is_default() -> Result<bool, String> {
    Ok(current_handler()? == ENTRY)
}

pub fn enable(exe: &Path, data: &Path) -> Result<(), String> {
    let dir = applications_dir()?;
    let before = current_handler()?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::fs::write(dir.join(ENTRY), entry_text(exe)).map_err(|e| e.to_string())?;
    // Best effort: some desktops read the entry without the cache being rebuilt.
    let _ = Command::new("update-desktop-database").arg(&dir).status();
    // Keep what was there, but never our own entry (turned on twice).
    if !before.is_empty() && before != ENTRY {
        std::fs::create_dir_all(data).map_err(|e| e.to_string())?;
        std::fs::write(data.join(KEPT), &before).map_err(|e| e.to_string())?;
    }
    if let Err(e) = set_handler(ENTRY) {
        let _ = std::fs::remove_file(dir.join(ENTRY));
        return Err(e);
    }
    Ok(())
}

pub fn disable(data: &Path) -> Result<(), String> {
    let kept = std::fs::read_to_string(data.join(KEPT))
        .ok()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty() && s != ENTRY);
    if current_handler()? == ENTRY
        && let Some(previous) = &kept
    {
        set_handler(previous)?;
    }
    // Without the entry the system falls back to its own choice, so an entry that was ours
    // alone is not left behind either way.
    if let Ok(dir) = applications_dir() {
        let _ = std::fs::remove_file(dir.join(ENTRY));
        let _ = Command::new("update-desktop-database").arg(&dir).status();
    }
    let _ = std::fs::remove_file(data.join(KEPT));
    Ok(())
}

/// Rewrites the entry when the program moved.
pub fn refresh(exe: &Path) {
    let Ok(dir) = applications_dir() else { return };
    let file = dir.join(ENTRY);
    if !file.exists() {
        return;
    }
    let want = entry_text(exe);
    if std::fs::read_to_string(&file).ok().as_deref() != Some(want.as_str()) {
        let _ = std::fs::write(&file, want);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_path_is_just_quoted() {
        assert_eq!(exec_quote("/opt/Folio/folio"), "\"/opt/Folio/folio\"");
    }

    #[test]
    fn reserved_characters_are_escaped_twice() {
        assert_eq!(exec_quote("/a b/$x"), "\"/a b/\\\\$x\"");
        assert_eq!(exec_quote("/100%"), "\"/100%%\"");
    }

    #[test]
    fn the_entry_handles_folders_and_stays_out_of_menus() {
        let text = entry_text(Path::new("/usr/bin/folio"));
        assert!(text.contains("Exec=\"/usr/bin/folio\" %F\n"));
        assert!(text.contains("MimeType=inode/directory;\n"));
        assert!(text.contains("NoDisplay=true\n"));
    }
}
