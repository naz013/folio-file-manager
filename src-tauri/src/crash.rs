//! Opt-in crash reporting, kept on this computer. Nothing is sent anywhere: with consent,
//! a panic leaves a text report in the app data folder for the user to attach to a bug, and
//! every session is counted so the crash-free rate can be read off. Without consent none
//! of this is written (docs/ARCHITECTURE.md: no telemetry by default).

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;

/// Where the panic hook writes; set once at start.
static DIR: OnceLock<PathBuf> = OnceLock::new();
static ENABLED: AtomicBool = AtomicBool::new(false);

const CONSENT: &str = "consent";
const SESSIONS: &str = "sessions";
const RUNNING: &str = "running";
const REPORT_PREFIX: &str = "crash-";

/// What the settings dialog shows.
#[derive(Serialize, Debug, PartialEq, Eq)]
pub struct State {
    /// `None` until the user has answered the prompt.
    pub consent: Option<bool>,
    pub sessions: u32,
    /// Sessions that ended without a clean exit.
    pub crashed: u32,
    pub reports: usize,
    pub dir: String,
}

fn read_consent(dir: &Path) -> Option<bool> {
    match std::fs::read_to_string(dir.join(CONSENT)).ok()?.trim() {
        "on" => Some(true),
        "off" => Some(false),
        _ => None,
    }
}

/// (sessions, crashed) as last saved.
fn read_counts(dir: &Path) -> (u32, u32) {
    let text = std::fs::read_to_string(dir.join(SESSIONS)).unwrap_or_default();
    let mut numbers = text.split_whitespace().map(|n| n.parse().unwrap_or(0));
    (numbers.next().unwrap_or(0), numbers.next().unwrap_or(0))
}

fn write_counts(dir: &Path, sessions: u32, crashed: u32) {
    let _ = std::fs::write(dir.join(SESSIONS), format!("{sessions} {crashed}"));
}

/// Counts a new session. A marker left by the last one means it never exited cleanly.
/// This assumes one Folio process at a time: a second one would count the first as a crash.
fn begin_session(dir: &Path) {
    let _ = std::fs::create_dir_all(dir);
    let (sessions, mut crashed) = read_counts(dir);
    if dir.join(RUNNING).exists() {
        crashed += 1;
    }
    write_counts(dir, sessions + 1, crashed);
    let _ = std::fs::write(dir.join(RUNNING), "");
}

/// The app is closing normally.
pub fn end_session(dir: &Path) {
    let _ = std::fs::remove_file(dir.join(RUNNING));
}

fn report_files(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(REPORT_PREFIX))
        })
        .collect()
}

/// Writes one report; the name never repeats within a second's panics.
fn write_report(dir: &Path, text: &str) {
    let _ = std::fs::create_dir_all(dir);
    let stamp = crate::journal::unix_now();
    let mut name = format!("{REPORT_PREFIX}{stamp}.txt");
    for n in 2.. {
        if !dir.join(&name).exists() {
            break;
        }
        name = format!("{REPORT_PREFIX}{stamp}-{n}.txt");
    }
    let _ = std::fs::write(dir.join(name), text);
}

fn describe(info: &std::panic::PanicHookInfo<'_>) -> String {
    let message = info
        .payload()
        .downcast_ref::<&str>()
        .map(|s| (*s).to_owned())
        .or_else(|| info.payload().downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "(no message)".into());
    let place = info
        .location()
        .map_or_else(|| "(unknown)".into(), ToString::to_string);
    format!(
        "Folio {} on {}\nthread: {}\nat: {place}\n\n{message}\n\n{}\n",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::thread::current().name().unwrap_or("(unnamed)"),
        std::backtrace::Backtrace::force_capture(),
    )
}

/// Reads the saved answer, starts counting this session if it was yes, and installs the
/// panic hook (which writes only while consent is on).
pub fn init(dir: &Path) {
    let _ = DIR.set(dir.to_path_buf());
    if read_consent(dir) == Some(true) {
        ENABLED.store(true, Ordering::Relaxed);
        begin_session(dir);
    }
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if ENABLED.load(Ordering::Relaxed)
            && let Some(dir) = DIR.get()
        {
            write_report(dir, &describe(info));
        }
        previous(info);
    }));
}

/// Records the user's answer. Turning it on counts the running session; turning it off
/// stops all writing, and the reports already saved stay until cleared.
pub fn set_consent(dir: &Path, on: bool) {
    let _ = std::fs::create_dir_all(dir);
    let _ = std::fs::write(dir.join(CONSENT), if on { "on" } else { "off" });
    let was = ENABLED.swap(on, Ordering::Relaxed);
    if on && !was {
        begin_session(dir);
    } else if !on && was {
        end_session(dir);
    }
}

pub fn state(dir: &Path) -> State {
    let (sessions, crashed) = read_counts(dir);
    State {
        consent: read_consent(dir),
        sessions,
        crashed,
        reports: report_files(dir).len(),
        dir: dir.display().to_string(),
    }
}

/// Deletes the saved reports and the session counts.
pub fn clear(dir: &Path) {
    for file in report_files(dir) {
        let _ = std::fs::remove_file(file);
    }
    // The running session starts counting again from itself.
    let _ = std::fs::remove_file(dir.join(SESSIONS));
    if ENABLED.load(Ordering::Relaxed) {
        write_counts(dir, 1, 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_is_recorded_until_the_user_says_yes() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(state(dir.path()).consent, None);
        assert!(!dir.path().join(SESSIONS).exists());
    }

    #[test]
    fn an_unclean_exit_counts_as_a_crash_and_a_clean_one_does_not() {
        let dir = tempfile::tempdir().unwrap();
        begin_session(dir.path());
        end_session(dir.path());
        begin_session(dir.path());
        // Killed: the marker stays.
        begin_session(dir.path());
        end_session(dir.path());
        let s = state(dir.path());
        assert_eq!((s.sessions, s.crashed), (3, 1));
    }

    #[test]
    fn reports_are_counted_never_overwritten_and_cleared() {
        let dir = tempfile::tempdir().unwrap();
        write_report(dir.path(), "one");
        write_report(dir.path(), "two");
        assert_eq!(state(dir.path()).reports, 2);
        clear(dir.path());
        assert_eq!(state(dir.path()).reports, 0);
    }

    #[test]
    fn counts_that_cannot_be_read_start_over() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(SESSIONS), "garbage").unwrap();
        assert_eq!(read_counts(dir.path()), (0, 0));
    }
}
