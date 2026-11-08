//! Folders Folio is asked to open when it starts, or when it is started again while running
//! (the system handing it a folder: see `default_manager`). The first start keeps them until
//! the window is ready to take them; a later one is sent to a window at once.

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{AppHandle, Emitter, Manager};

/// What the window is told, with the folders as text.
const EVENT: &str = "open-folders";

/// Folders waiting for the main window, and whether it has come for them yet.
#[derive(Default)]
pub struct Pending {
    folders: Mutex<Vec<PathBuf>>,
    taken: AtomicBool,
}

impl Pending {
    pub fn new(folders: Vec<PathBuf>) -> Self {
        Self {
            folders: Mutex::new(folders),
            taken: AtomicBool::new(false),
        }
    }

    /// Hands over what is waiting, once the window is ready; from then on folders are sent
    /// as they come.
    pub fn take(&self) -> Vec<PathBuf> {
        self.taken.store(true, Ordering::SeqCst);
        std::mem::take(
            &mut *self
                .folders
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
    }

    /// Keeps `more` for the window, unless it is already running: then they are not kept and
    /// `false` comes back.
    fn keep(&self, more: Vec<PathBuf>) -> bool {
        let mut waiting = self
            .folders
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if self.taken.load(Ordering::SeqCst) {
            return false;
        }
        waiting.extend(more);
        true
    }
}

/// The folders among command-line arguments (without the program's own name). Anything that
/// is not an existing folder, and every option, is skipped: a stray argument must not open
/// anything. A `file://` address is taken as the path it names.
pub fn folders_from_args<I>(args: I) -> Vec<PathBuf>
where
    I: IntoIterator<Item = OsString>,
{
    let mut found: Vec<PathBuf> = Vec::new();
    for arg in args {
        let Some(text) = arg.to_str() else {
            let path = PathBuf::from(&arg);
            if path.is_dir() && !found.contains(&path) {
                found.push(path);
            }
            continue;
        };
        if text.starts_with('-') {
            continue;
        }
        let path = fix_drive_root(&from_address(text));
        if path.is_dir() && !found.contains(&path) {
            found.push(path);
        }
    }
    found
}

/// `file:///home/me/a%20b` as `/home/me/a b`; anything else as it is.
fn from_address(text: &str) -> PathBuf {
    let Some(rest) = text.strip_prefix("file://") else {
        return PathBuf::from(text);
    };
    // `file:///C:/x` carries a slash before the drive letter; `file://host/x` is not local.
    let path = rest.strip_prefix("localhost").unwrap_or(rest);
    let decoded = percent_encoding::percent_decode_str(path)
        .decode_utf8_lossy()
        .into_owned();
    let bytes = decoded.as_bytes();
    if bytes.len() >= 3 && bytes[0] == b'/' && bytes[2] == b':' && bytes[1].is_ascii_alphabetic() {
        return PathBuf::from(&decoded[1..]);
    }
    PathBuf::from(decoded)
}

/// A drive root passed as `"D:\"` reaches the program as `D:"`, the closing quote taken as
/// part of the argument; put the root back (`D:` alone is the drive's current folder, not
/// its root).
fn fix_drive_root(path: &std::path::Path) -> PathBuf {
    let text = path.to_string_lossy();
    let bare = text.trim_end_matches('"');
    let bytes = bare.as_bytes();
    if bytes.len() == 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
        return PathBuf::from(format!("{bare}\\"));
    }
    if bare.len() != text.len() {
        return PathBuf::from(format!("{bare}\\"));
    }
    path.to_path_buf()
}

/// Asks the app to open `folders`: kept for the main window if it has not started yet,
/// else sent to the window that was last used (the main one, if no other is open), which
/// is also brought forward.
pub fn deliver(app: &AppHandle, folders: Vec<PathBuf>) {
    let strings: Vec<String> = folders
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    if !folders.is_empty() && app.state::<Pending>().keep(folders) {
        return;
    }
    let windows = app.webview_windows();
    let target = windows
        .values()
        .find(|w| w.is_focused().unwrap_or(false))
        .or_else(|| windows.get(crate::session::MAIN_LABEL))
        .or_else(|| windows.values().next());
    let Some(window) = target else { return };
    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_focus();
    if !strings.is_empty() {
        let _ = window.emit(EVENT, strings);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    #[test]
    fn only_existing_folders_are_taken() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.txt");
        std::fs::write(&file, "x").unwrap();
        let folder = dir.path().to_string_lossy().into_owned();
        let found = folders_from_args(args(&[
            "--flag",
            &file.to_string_lossy(),
            "nowhere-at-all",
            &folder,
            &folder,
        ]));
        assert_eq!(found, vec![dir.path().to_path_buf()]);
    }

    #[test]
    fn no_arguments_open_nothing() {
        assert_eq!(folders_from_args(args(&[])), Vec::<PathBuf>::new());
    }

    #[test]
    fn a_file_address_is_read_as_its_path() {
        assert_eq!(
            from_address("file:///home/me/a%20b"),
            PathBuf::from("/home/me/a b")
        );
        assert_eq!(
            from_address("file:///C:/Users/me"),
            PathBuf::from("C:/Users/me")
        );
        assert_eq!(from_address("/plain/path"), PathBuf::from("/plain/path"));
    }

    #[test]
    fn a_drive_root_that_lost_its_slash_gets_it_back() {
        assert_eq!(
            fix_drive_root(&PathBuf::from("D:\"")),
            PathBuf::from("D:\\")
        );
        assert_eq!(fix_drive_root(&PathBuf::from("D:")), PathBuf::from("D:\\"));
        assert_eq!(
            fix_drive_root(&PathBuf::from("D:\\data")),
            PathBuf::from("D:\\data")
        );
    }

    #[test]
    fn folders_wait_until_the_window_comes_for_them() {
        let pending = Pending::new(vec![PathBuf::from("/a")]);
        assert!(pending.keep(vec![PathBuf::from("/b")]));
        assert_eq!(
            pending.take(),
            vec![PathBuf::from("/a"), PathBuf::from("/b")]
        );
        // Once taken, later folders are sent on, not kept.
        assert!(!pending.keep(vec![PathBuf::from("/c")]));
        assert_eq!(pending.take(), Vec::<PathBuf>::new());
    }
}
