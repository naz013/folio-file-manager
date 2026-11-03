//! Folio's side of a fast scan: asks Windows for administrator rights for the helper, listens on
//! a private pipe, and turns what comes back into the tree a scan stores.

use std::fmt::Write as _;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::os::windows::io::{AsRawHandle, FromRawHandle, RawHandle};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{
    CloseHandle, ERROR_CANCELLED, ERROR_PIPE_CONNECTED, ERROR_PIPE_LISTENING, HANDLE,
    INVALID_HANDLE_VALUE, WAIT_OBJECT_0,
};
use windows::Win32::Security::Cryptography::{BCRYPT_USE_SYSTEM_PREFERRED_RNG, BCryptGenRandom};
use windows::Win32::Storage::FileSystem::{
    FILE_FLAG_FIRST_PIPE_INSTANCE, GetVolumeInformationW, PIPE_ACCESS_INBOUND,
};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, PIPE_NOWAIT, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE,
    PIPE_WAIT, SetNamedPipeHandleState,
};
use windows::Win32::System::Threading::WaitForSingleObject;
use windows::Win32::UI::Shell::{
    SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW,
};
use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;
use windows::core::{PCWSTR, w};

#[cfg(test)]
use super::helper::valid_pipe;
use super::helper::{PIPE_PREFIX, SWITCH};
use super::{Fast, Message};
use crate::scan::{Acc, Big, Tree};

/// How long the helper has to open the pipe after Windows has let it start.
const CONNECT_WITHIN: Duration = Duration::from_secs(20);

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// `C:` for a path on a lettered drive.
fn drive_of(root: &Path) -> Option<String> {
    let s = root.to_string_lossy();
    let mut chars = s.chars();
    let letter = chars.next().filter(char::is_ascii_alphabetic)?;
    (chars.next() == Some(':')).then(|| format!("{}:", letter.to_ascii_uppercase()))
}

fn is_ntfs(drive: &str) -> bool {
    let root = wide(&format!("{drive}\\"));
    let mut name = [0u16; 32];
    // SAFETY: `root` is NUL-terminated and `name` is a valid buffer of the length given; the
    // file system's name is the last of the six parameters (the first buffer is the label).
    let ok = unsafe {
        GetVolumeInformationW(
            PCWSTR(root.as_ptr()),
            None,
            None,
            None,
            None,
            Some(&mut name),
        )
    };
    let len = name.iter().position(|&c| c == 0).unwrap_or(name.len());
    ok.is_ok() && String::from_utf16_lossy(&name[..len]).eq_ignore_ascii_case("NTFS")
}

/// `rel` (a path relative to the folder scanned, as the helper sends it) below `root`.
fn under(root: &Path, rel: &str) -> PathBuf {
    if rel.is_empty() {
        root.to_path_buf()
    } else {
        root.join(rel)
    }
}

/// A pipe name nobody else can guess.
fn pipe_name() -> Option<String> {
    let mut bytes = [0u8; 16];
    // SAFETY: the buffer is valid for its length; the system RNG needs no algorithm handle.
    let status = unsafe { BCryptGenRandom(None, &mut bytes, BCRYPT_USE_SYSTEM_PREFERRED_RNG) };
    status.is_ok().then(|| {
        let hex = bytes.iter().fold(String::new(), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        });
        format!("{PIPE_PREFIX}{hex}")
    })
}

/// The helper process, closed when this goes.
struct Process(HANDLE);

impl Drop for Process {
    fn drop(&mut self) {
        // SAFETY: the handle was returned by `ShellExecuteExW` and is closed only here.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

impl Process {
    fn ended(&self) -> bool {
        // SAFETY: the handle is a live process handle owned by `self`.
        unsafe { WaitForSingleObject(self.0, 0) == WAIT_OBJECT_0 }
    }
}

/// Starts Folio again as administrator, as the helper. The error says whether the person
/// declined Windows' prompt, and why it failed otherwise.
fn launch(drive: &str, folder: &str, pipe: &str) -> Result<Process, (bool, String)> {
    let exe = std::env::current_exe().map_err(|e| (false, e.to_string()))?;
    // A drive root is passed as `C:`: a trailing backslash would escape the closing quote.
    let folder = folder.trim_end_matches(['\\', '/']);
    let params = format!(
        "{SWITCH} {drive} \"{folder}\" \"{pipe}\" {}",
        std::process::id()
    );
    let file = wide(&exe.to_string_lossy());
    let params = wide(&params);
    let mut info = SHELLEXECUTEINFOW {
        cbSize: u32::try_from(size_of::<SHELLEXECUTEINFOW>()).unwrap_or_default(),
        fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC,
        lpVerb: w!("runas"),
        lpFile: PCWSTR(file.as_ptr()),
        lpParameters: PCWSTR(params.as_ptr()),
        nShow: SW_HIDE.0,
        ..SHELLEXECUTEINFOW::default()
    };
    // SAFETY: `info` is filled in as the API requires and the strings it points to live until
    // the call returns.
    match unsafe { ShellExecuteExW(&raw mut info) } {
        Ok(()) if !info.hProcess.is_invalid() => Ok(Process(info.hProcess)),
        Ok(()) => Err((false, "Windows did not start the helper".to_owned())),
        Err(e) => Err((e.code() == ERROR_CANCELLED.to_hresult(), e.message())),
    }
}

/// Makes the pipe the helper will write to: one instance, no remote clients.
fn make_pipe(name: &str) -> Option<File> {
    let wname = wide(name);
    // SAFETY: the name is NUL-terminated; no security attributes means the default, which lets
    // administrators (the elevated helper) connect.
    let handle = unsafe {
        CreateNamedPipeW(
            PCWSTR(wname.as_ptr()),
            PIPE_ACCESS_INBOUND | FILE_FLAG_FIRST_PIPE_INSTANCE,
            PIPE_TYPE_BYTE | PIPE_NOWAIT | PIPE_REJECT_REMOTE_CLIENTS,
            1,
            0,
            1 << 20,
            0,
            None,
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return None;
    }
    // SAFETY: the handle is new and valid, and the `File` becomes its only owner.
    Some(unsafe { File::from_raw_handle(handle.0 as RawHandle) })
}

/// Waits for the helper to open the pipe; false if it never did, or the scan was cancelled.
fn wait_for_helper(pipe: &File, helper: &Process, cancel: &AtomicBool) -> bool {
    let handle = HANDLE(pipe.as_raw_handle());
    let began = Instant::now();
    loop {
        if cancel.load(Ordering::Relaxed) || began.elapsed() > CONNECT_WITHIN {
            return false;
        }
        // SAFETY: the handle belongs to `pipe`, which outlives the call.
        match unsafe { ConnectNamedPipe(handle, None) } {
            Ok(()) => break,
            Err(e) if e.code() == ERROR_PIPE_CONNECTED.to_hresult() => break,
            Err(e) if e.code() == ERROR_PIPE_LISTENING.to_hresult() => {
                if helper.ended() {
                    return false;
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(_) => return false,
        }
    }
    let wait = PIPE_WAIT;
    // SAFETY: as above; the mode is a valid local.
    unsafe { SetNamedPipeHandleState(handle, Some(&raw const wait), None, None).is_ok() }
}

/// Scans `root` through the helper. Calls `progress(files, bytes)` while it reads.
pub fn scan(root: &Path, cancel: &AtomicBool, progress: &dyn Fn(u64, u64)) -> Fast {
    let Some(drive) = drive_of(root) else {
        return Fast::Fallback("fast scan reads lettered drives only".to_owned());
    };
    if !is_ntfs(&drive) {
        return Fast::Fallback(format!("{drive} is not an NTFS drive"));
    }
    let Some(name) = pipe_name() else {
        return Fast::Fallback("Windows gave no secure random number".to_owned());
    };
    let Some(pipe) = make_pipe(&name) else {
        return Fast::Fallback("fast scan could not open its pipe".to_owned());
    };
    let helper = match launch(&drive, &root.to_string_lossy(), &name) {
        Ok(h) => h,
        Err((true, _)) => {
            return Fast::Fallback("the Windows permission prompt was declined".to_owned());
        }
        Err((false, why)) => return Fast::Fallback(format!("the helper did not start: {why}")),
    };
    if !wait_for_helper(&pipe, &helper, cancel) {
        return if cancel.load(Ordering::Relaxed) {
            Fast::Cancelled
        } else {
            Fast::Fallback("the helper did not connect".to_owned())
        };
    }
    read_tree(BufReader::new(pipe), root, cancel, progress)
}

/// Builds the tree from the helper's lines.
fn read_tree(
    lines: impl BufRead,
    root: &Path,
    cancel: &AtomicBool,
    progress: &dyn Fn(u64, u64),
) -> Fast {
    let mut tree = Tree::default();
    for line in lines.lines() {
        if cancel.load(Ordering::Relaxed) {
            return Fast::Cancelled;
        }
        let Ok(line) = line else { break };
        let Ok(message) = serde_json::from_str::<Message>(&line) else {
            return Fast::Fallback("the helper sent something unreadable".to_owned());
        };
        match message {
            Message::Progress { files, bytes } => progress(files, bytes),
            Message::Dir {
                path,
                own,
                files,
                cats,
            } => {
                // Joined onto the folder as it was asked for, so every path in the tree is
                // spelled the same way and the totals find their parents.
                tree.dirs.insert(
                    under(root, &path),
                    Acc {
                        own,
                        own_files: files,
                        cats,
                    },
                );
            }
            Message::Big {
                path,
                dir,
                size,
                mtime,
            } => tree.big.push(Big {
                path: under(root, &path),
                dir: under(root, &dir),
                size,
                mtime,
            }),
            Message::Done { files, bytes } => {
                if !tree.dirs.contains_key(root) {
                    return Fast::Fallback(
                        "the folder is not in the drive's file table".to_owned(),
                    );
                }
                tree.files = files;
                tree.bytes = bytes;
                progress(files, bytes);
                return Fast::Tree(tree);
            }
            Message::Failed { why } => return Fast::Fallback(why),
        }
    }
    Fast::Fallback("the helper stopped before it finished".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(messages: &[Message]) -> Vec<u8> {
        let mut out = Vec::new();
        for m in messages {
            out.extend(serde_json::to_vec(m).unwrap());
            out.push(b'\n');
        }
        out
    }

    fn dir(path: &str, own: u64, files: u64) -> Message {
        Message::Dir {
            path: path.to_owned(),
            own,
            files,
            cats: [0; 7],
        }
    }

    fn read(data: &[u8], root: &str, cancelled: bool) -> Fast {
        let cancel = AtomicBool::new(cancelled);
        read_tree(data, Path::new(root), &cancel, &|_, _| {})
    }

    #[test]
    fn a_finished_read_becomes_a_tree_with_the_folder_named_as_asked() {
        let data = lines(&[
            Message::Progress { files: 1, bytes: 2 },
            dir("", 5, 1),
            dir("me", 7, 2),
            Message::Big {
                path: "me/x.iso".to_owned(),
                dir: "me".to_owned(),
                size: 9,
                mtime: 4,
            },
            Message::Done {
                files: 3,
                bytes: 12,
            },
        ]);
        let Fast::Tree(tree) = read(&data, "c:/users", false) else {
            panic!("expected a tree");
        };
        assert_eq!((tree.files, tree.bytes), (3, 12));
        // Spelled as asked, whatever the disk's own capitals are.
        assert!(tree.dirs.contains_key(Path::new("c:/users")));
        assert_eq!(tree.dirs[&Path::new("c:/users").join("me")].own_files, 2);
        assert_eq!(tree.big[0].dir, Path::new("c:/users").join("me"));
        assert_eq!(tree.big[0].path, Path::new("c:/users").join("me/x.iso"));
    }

    #[test]
    fn a_failure_a_cut_off_read_and_garbage_all_fall_back() {
        let failed = lines(&[Message::Failed {
            why: "no".to_owned(),
        }]);
        assert!(matches!(read(&failed, "C:/", false), Fast::Fallback(w) if w == "no"));
        let cut = lines(&[dir("C:/", 1, 1)]);
        assert!(matches!(read(&cut, "C:/", false), Fast::Fallback(_)));
        assert!(matches!(
            read(b"not json\n", "C:/", false),
            Fast::Fallback(_)
        ));
    }

    #[test]
    fn a_root_missing_from_the_answer_falls_back_and_a_cancel_stops() {
        let data = lines(&[dir("C:/Other", 1, 1), Message::Done { files: 1, bytes: 1 }]);
        assert!(matches!(read(&data, "C:/Users", false), Fast::Fallback(_)));
        assert!(matches!(read(&data, "C:/Other", true), Fast::Cancelled));
    }

    #[test]
    fn the_totals_of_a_tree_whose_root_was_spelled_differently_still_add_up() {
        let data = lines(&[
            dir("", 1, 1),
            dir("Sub", 2, 1),
            dir("Sub/Deep", 4, 1),
            Message::Done { files: 3, bytes: 7 },
        ]);
        let Fast::Tree(tree) = read(&data, "c:/data", false) else {
            panic!("expected a tree");
        };
        // Every folder's parent is a folder of the tree, so nothing is cut off from the root.
        for path in tree.dirs.keys().filter(|p| *p != Path::new("c:/data")) {
            assert!(tree.dirs.contains_key(path.parent().unwrap()), "{path:?}");
        }
    }

    #[test]
    fn the_file_system_of_a_real_drive_is_read_not_its_label() {
        // The drive Windows is installed on is NTFS (and CI's runner too).
        let system = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".to_owned());
        assert!(is_ntfs(&system));
        assert!(!is_ntfs("Q:"));
    }

    #[test]
    fn the_drive_comes_from_the_path_and_only_lettered_paths_have_one() {
        assert_eq!(drive_of(Path::new("c:/Users")).as_deref(), Some("C:"));
        assert_eq!(drive_of(Path::new("//server/share")), None);
        assert_eq!(drive_of(Path::new("relative")), None);
    }

    #[test]
    fn pipe_names_are_long_and_never_repeat() {
        let (a, b) = (pipe_name().unwrap(), pipe_name().unwrap());
        assert_ne!(a, b);
        assert!(valid_pipe(&a) && valid_pipe(&b));
    }
}
