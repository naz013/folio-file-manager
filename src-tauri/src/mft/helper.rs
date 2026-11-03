//! The elevated side of a fast scan: Folio started again, as administrator, with `--mft-helper`.
//!
//! It reads the drive's file table and writes what it found to the pipe Folio made, then exits.
//! It changes nothing on the drive and sends only to the Folio process that started it.

use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::AsRawHandle;

use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::Pipes::GetNamedPipeServerProcessId;
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::core::PWSTR;

use super::{Message, summarize};

/// The command-line switch that makes this process the helper.
pub const SWITCH: &str = "--mft-helper";
/// What the name of the pipe Folio listens on begins with; then 32 hex digits.
pub const PIPE_PREFIX: &str = r"\\.\pipe\folio-mft-";

/// `--mft-helper <drive:> <folder> <pipe> <parent pid>`: runs the helper when Folio was started
/// that way, and returns the exit code; `None` for a normal start.
pub fn run_if_requested() -> Option<i32> {
    // Every start comes through here, so an argument that is not Unicode must not panic.
    let args: Vec<std::ffi::OsString> = std::env::args_os().collect();
    if args.get(1).and_then(|a| a.to_str()) != Some(SWITCH) {
        return None;
    }
    let args: Vec<&str> = args.iter().filter_map(|a| a.to_str()).collect();
    Some(match args.as_slice() {
        [_, _, drive, folder, pipe, parent] if valid_drive(drive) && valid_pipe(pipe) => parent
            .parse()
            .map_or(2, |parent| serve(drive, folder, pipe, parent)),
        _ => 2,
    })
}

/// A drive letter and colon, nothing else: the argument goes into a device path.
fn valid_drive(drive: &str) -> bool {
    matches!(drive.as_bytes(), [letter, b':'] if letter.is_ascii_alphabetic())
}

/// The name of a pipe Folio makes: its prefix and 32 hex digits, nothing else.
pub fn valid_pipe(pipe: &str) -> bool {
    pipe.strip_prefix(PIPE_PREFIX)
        .is_some_and(|hex| hex.len() == 32 && hex.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// Whether `pid` runs the same program file as this process: the pipe's server must be Folio.
fn same_program(pid: u32) -> bool {
    let Ok(me) = std::env::current_exe() else {
        return false;
    };
    // SAFETY: a plain query handle on a process id; it is closed below.
    let Ok(process) = (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) })
    else {
        return false;
    };
    let mut buf = [0u16; 1024];
    let mut len = 1024u32;
    // SAFETY: `buf` holds `len` units and `process` is the handle opened above.
    let named = unsafe {
        QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &raw mut len,
        )
    };
    // SAFETY: the handle came from `OpenProcess` and is closed once.
    unsafe {
        let _ = CloseHandle(process);
    }
    let theirs = String::from_utf16_lossy(&buf[..usize::try_from(len).unwrap_or(0)]);
    named.is_ok() && theirs.eq_ignore_ascii_case(me.to_string_lossy().trim_start_matches(r"\\?\"))
}

/// The root of a drive is passed as `C:`, because a command line can't end a quoted argument
/// with a backslash; this puts the backslash back.
fn rooted(path: &str) -> String {
    if path.ends_with(':') {
        format!("{path}\\")
    } else {
        path.to_owned()
    }
}

fn serve(drive: &str, folder: &str, pipe: &str, parent: u32) -> i32 {
    let Ok(pipe) = OpenOptions::new().write(true).open(pipe) else {
        return 3;
    };
    // Only the Folio that started this may be told what is on the drive.
    let mut server = 0u32;
    // SAFETY: the handle belongs to `pipe`, which is open for the whole call, and `server` is a
    // valid place to write a process id.
    let found =
        unsafe { GetNamedPipeServerProcessId(HANDLE(pipe.as_raw_handle()), &raw mut server) };
    if found.is_err() || server != parent || !same_program(server) {
        return 4;
    }
    let mut out = BufWriter::new(pipe);
    let mut send = |m: Message| -> bool {
        let flush = matches!(
            m,
            Message::Progress { .. } | Message::Done { .. } | Message::Failed { .. }
        );
        serde_json::to_writer(&mut out, &m).is_ok()
            && out.write_all(b"\n").is_ok()
            && (!flush || out.flush().is_ok())
    };
    let result = open_volume(drive).and_then(|mut volume| {
        summarize(
            &mut volume,
            &rooted(drive),
            &rooted(folder),
            &mut || false,
            &mut send,
        )
    });
    if let Err(e) = result {
        send(Message::Failed { why: e.to_string() });
    }
    0
}

/// The raw volume, for reading only. Windows allows this to administrators.
fn open_volume(drive: &str) -> Result<File, super::Failure> {
    // Share everything: the drive is in use while it is read.
    OpenOptions::new()
        .read(true)
        .share_mode(0x7)
        .open(format!(r"\\.\{drive}"))
        .map_err(super::Failure::Io)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_drive_letter_and_colon_is_a_drive() {
        assert!(valid_drive("C:") && valid_drive("z:"));
        for bad in [
            "",
            "C",
            "C:\\",
            "CD:",
            "1:",
            r"GLOBALROOT\Device\HarddiskVolumeShadowCopy1",
            "C:x",
        ] {
            assert!(!valid_drive(bad), "{bad}");
        }
    }

    #[test]
    fn only_a_pipe_folio_would_make_is_accepted() {
        let good = format!("{PIPE_PREFIX}{}", "0123456789abcdefABCDEF0123456789");
        assert!(valid_pipe(&good));
        for bad in [
            String::new(),
            r"C:	emp
otes.txt"
                .to_owned(),
            r"\host\pipeolio-mft-0123456789abcdef0123456789abcdef".to_owned(),
            format!("{PIPE_PREFIX}short"),
            format!("{PIPE_PREFIX}{}", "g".repeat(32)),
            format!("{good}0"),
        ] {
            assert!(!valid_pipe(&bad), "{bad}");
        }
    }

    #[test]
    fn this_process_is_the_same_program_as_itself_and_a_system_process_is_not() {
        assert!(same_program(std::process::id()));
        // Process 4 is the system; it can't be opened, which counts as not the same.
        assert!(!same_program(4));
    }
}
