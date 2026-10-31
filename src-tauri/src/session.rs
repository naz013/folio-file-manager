//! Open windows and their tabs, saved to disk so they come back on the next launch.
//!
//! Each window reports its tabs whenever they change. Closing a window forgets it,
//! unless it is the last open one: then its tabs are what the next launch restores.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

pub const MAIN_LABEL: &str = "main";

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WindowTabs {
    pub tabs: Vec<String>,
    pub active: usize,
}

impl WindowTabs {
    /// Drops empty paths and clamps `active`; `None` when nothing is left.
    fn sanitized(mut self) -> Option<Self> {
        self.tabs.retain(|t| !t.is_empty());
        if self.tabs.is_empty() {
            return None;
        }
        self.active = self.active.min(self.tabs.len() - 1);
        Some(self)
    }
}

#[derive(Default, Serialize, Deserialize)]
struct SessionFile {
    windows: Vec<WindowTabs>,
}

#[derive(Default)]
struct State {
    /// Window label → tabs, in window creation order.
    windows: Vec<(String, WindowTabs)>,
    /// Windows currently open.
    open: HashSet<String>,
    /// Windows closed for good. Labels are never reused, so a save that arrives after
    /// the close (a debounced update still in flight) cannot bring the window back.
    closed: HashSet<String>,
}

pub struct Session {
    file: Option<PathBuf>,
    state: Mutex<State>,
    /// Held while writing the file, with the bytes last written.
    writer: Mutex<Vec<u8>>,
    next_id: AtomicUsize,
}

impl Session {
    /// Reads the saved session and assigns each saved window a label: the first one is
    /// `main`, the rest get fresh labels. Returns the labels that still need a window.
    pub fn load(file: Option<PathBuf>) -> (Self, Vec<String>) {
        let saved: SessionFile = file
            .as_ref()
            .and_then(|f| std::fs::read(f).ok())
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        let session = Session {
            file,
            state: Mutex::default(),
            writer: Mutex::default(),
            next_id: AtomicUsize::new(1),
        };
        let mut extra = Vec::new();
        {
            let mut state = session.state.lock().unwrap();
            state.open.insert(MAIN_LABEL.to_owned());
            let restored = saved.windows.into_iter().filter_map(WindowTabs::sanitized);
            for (i, tabs) in restored.enumerate() {
                let label = if i == 0 {
                    MAIN_LABEL.to_owned()
                } else {
                    let label = session.next_label();
                    state.open.insert(label.clone());
                    extra.push(label.clone());
                    label
                };
                state.windows.push((label, tabs));
            }
        }
        (session, extra)
    }

    pub fn next_label(&self) -> String {
        format!("w-{}", self.next_id.fetch_add(1, Ordering::Relaxed))
    }

    pub fn tabs(&self, label: &str) -> Option<WindowTabs> {
        let state = self.state.lock().unwrap();
        let found = state.windows.iter().find(|(l, _)| l == label);
        found.map(|(_, t)| t.clone())
    }

    /// Registers a new window that will show `tabs`.
    pub fn open(&self, label: &str, tabs: WindowTabs) {
        self.state.lock().unwrap().open.insert(label.to_owned());
        self.update(label, tabs);
    }

    /// Records the tabs of window `label` and saves the session.
    /// Ignored for windows that were already closed.
    pub fn update(&self, label: &str, tabs: WindowTabs) {
        let Some(tabs) = tabs.sanitized() else {
            return;
        };
        {
            let mut state = self.state.lock().unwrap();
            if state.closed.contains(label) {
                return;
            }
            match state.windows.iter_mut().find(|(l, _)| l == label) {
                Some((_, t)) => *t = tabs,
                None => state.windows.push((label.to_owned(), tabs)),
            }
        }
        self.save();
    }

    /// Forgets window `label` and saves the session, unless it is the last open window:
    /// its tabs stay for the next launch. Deciding under the lock means two windows
    /// closing at once can't both see the other and leave the session empty.
    pub fn close(&self, label: &str) {
        {
            let mut state = self.state.lock().unwrap();
            if state.open.len() <= 1 && state.open.contains(label) {
                return;
            }
            state.open.remove(label);
            state.closed.insert(label.to_owned());
            state.windows.retain(|(l, _)| l != label);
        }
        self.save();
    }

    /// Writes the current state. Serializing under the writer lock means the last
    /// writer always writes the newest state; unchanged bytes are not rewritten.
    fn save(&self) {
        let Some(file) = &self.file else { return };
        let mut last = self.writer.lock().unwrap();
        let data = {
            let state = self.state.lock().unwrap();
            SessionFile {
                windows: state.windows.iter().map(|(_, t)| t.clone()).collect(),
            }
        };
        let Ok(bytes) = serde_json::to_vec(&data) else {
            return;
        };
        if bytes == *last {
            return;
        }
        let result = (|| {
            if let Some(dir) = file.parent() {
                std::fs::create_dir_all(dir)?;
            }
            // Write then rename, so a crash mid-write never leaves a truncated session.
            let tmp = file.with_extension("json.tmp");
            std::fs::write(&tmp, &bytes)?;
            std::fs::rename(&tmp, file)
        })();
        match result {
            Ok(()) => *last = bytes,
            Err(e) => eprintln!("saving session failed: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tabs(paths: &[&str], active: usize) -> WindowTabs {
        WindowTabs {
            tabs: paths.iter().map(|p| p.to_string()).collect(),
            active,
        }
    }

    fn temp_session() -> (tempfile::TempDir, PathBuf, Session) {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("session.json");
        let (session, _) = Session::load(Some(file.clone()));
        (dir, file, session)
    }

    #[test]
    fn load_without_file_restores_nothing() {
        let (session, extra) = Session::load(None);
        assert!(extra.is_empty());
        assert_eq!(session.tabs(MAIN_LABEL), None);
    }

    #[test]
    fn saved_windows_are_restored_with_main_first() {
        let (_dir, file, session) = temp_session();
        session.update(MAIN_LABEL, tabs(&["C:\\a", "C:\\b"], 1));
        session.open("w-7", tabs(&["D:\\c"], 0));

        let (restored, extra) = Session::load(Some(file));
        assert_eq!(
            restored.tabs(MAIN_LABEL),
            Some(tabs(&["C:\\a", "C:\\b"], 1))
        );
        assert_eq!(extra.len(), 1);
        assert_eq!(restored.tabs(&extra[0]), Some(tabs(&["D:\\c"], 0)));
        // Fresh labels never collide with restored ones.
        assert_ne!(restored.next_label(), extra[0]);
    }

    #[test]
    fn closed_windows_are_not_restored() {
        let (_dir, file, session) = temp_session();
        session.update(MAIN_LABEL, tabs(&["C:\\a"], 0));
        session.open("w-1", tabs(&["C:\\b"], 0));
        session.close(MAIN_LABEL);

        let (restored, extra) = Session::load(Some(file));
        assert_eq!(restored.tabs(MAIN_LABEL), Some(tabs(&["C:\\b"], 0)));
        assert!(extra.is_empty());
    }

    #[test]
    fn last_open_window_is_kept_on_close() {
        let (_dir, file, session) = temp_session();
        session.update(MAIN_LABEL, tabs(&["C:\\a"], 0));
        session.close(MAIN_LABEL);

        let (restored, _) = Session::load(Some(file));
        assert_eq!(restored.tabs(MAIN_LABEL), Some(tabs(&["C:\\a"], 0)));
    }

    #[test]
    fn closing_two_windows_keeps_the_one_closed_last() {
        let (_dir, file, session) = temp_session();
        session.update(MAIN_LABEL, tabs(&["C:\\a"], 0));
        session.open("w-1", tabs(&["C:\\b"], 0));
        session.close("w-1");
        session.close(MAIN_LABEL);

        let (restored, extra) = Session::load(Some(file));
        assert_eq!(restored.tabs(MAIN_LABEL), Some(tabs(&["C:\\a"], 0)));
        assert!(extra.is_empty());
    }

    #[test]
    fn concurrent_closes_never_empty_the_session() {
        let (_dir, _file, session) = temp_session();
        session.update(MAIN_LABEL, tabs(&["C:\\a"], 0));
        session.open("w-1", tabs(&["C:\\b"], 0));
        std::thread::scope(|s| {
            s.spawn(|| session.close(MAIN_LABEL));
            s.spawn(|| session.close("w-1"));
        });
        let kept = [MAIN_LABEL, "w-1"]
            .iter()
            .filter(|l| session.tabs(l).is_some())
            .count();
        assert_eq!(kept, 1);
    }

    #[test]
    fn late_update_does_not_resurrect_a_closed_window() {
        let (_dir, _file, session) = temp_session();
        session.update(MAIN_LABEL, tabs(&["C:\\a"], 0));
        session.open("w-1", tabs(&["C:\\b"], 0));
        session.close("w-1");
        session.update("w-1", tabs(&["C:\\b"], 0));
        assert_eq!(session.tabs("w-1"), None);
    }

    #[test]
    fn invalid_entries_are_sanitized() {
        let (session, _) = Session::load(None);
        session.update(MAIN_LABEL, tabs(&["", "C:\\a"], 5));
        assert_eq!(session.tabs(MAIN_LABEL), Some(tabs(&["C:\\a"], 0)));
        session.update("w-1", tabs(&[""], 0));
        assert_eq!(session.tabs("w-1"), None);
    }

    #[test]
    fn corrupt_file_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("session.json");
        std::fs::write(&file, "{ not json").unwrap();
        let (session, extra) = Session::load(Some(file));
        assert!(extra.is_empty());
        assert_eq!(session.tabs(MAIN_LABEL), None);
    }
}
