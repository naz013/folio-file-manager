//! Open windows, their tabs and their place on screen, saved to disk so they come back
//! on the next launch.
//!
//! Each window reports its tabs whenever they change, and the backend tracks its size
//! and position. Closing a window forgets it, unless it is the last open one: then its
//! tabs and place are what the next launch restores.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

pub const MAIN_LABEL: &str = "main";

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WindowTabs {
    pub tabs: Vec<String>,
    pub active: usize,
    /// Each tab's view mode ("grid" or "list"), in tab order; "" or absent means the default.
    #[serde(default)]
    pub views: Vec<String>,
    /// Each tab's collapsed sidebar sections, comma separated, in tab order; "" or absent
    /// means none are collapsed.
    #[serde(default)]
    pub collapsed: Vec<String>,
}

impl WindowTabs {
    /// Drops empty paths and clamps `active`; `None` when nothing is left.
    fn sanitized(mut self) -> Option<Self> {
        // Each tab keeps its view and its collapsed sections when the empty ones are dropped.
        let had_views = !self.views.is_empty();
        let had_collapsed = !self.collapsed.is_empty();
        let mut views = std::mem::take(&mut self.views).into_iter();
        let mut collapsed = std::mem::take(&mut self.collapsed).into_iter();
        let mut kept = Vec::new();
        for tab in std::mem::take(&mut self.tabs) {
            let view = views.next().unwrap_or_default();
            let fold = collapsed.next().unwrap_or_default();
            if !tab.is_empty() {
                kept.push((tab, view, fold));
            }
        }
        self.tabs = kept.iter().map(|(t, _, _)| t.clone()).collect();
        self.views = if had_views {
            kept.iter().map(|(_, v, _)| v.clone()).collect()
        } else {
            Vec::new()
        };
        self.collapsed = if had_collapsed {
            kept.into_iter().map(|(_, _, c)| c).collect()
        } else {
            Vec::new()
        };
        if self.tabs.is_empty() {
            return None;
        }
        self.active = self.active.min(self.tabs.len() - 1);
        Some(self)
    }
}

/// Where a window sits, in physical pixels: the outer position and the inner size.
/// When maximized, the rectangle is the one to return to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Geometry {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub maximized: bool,
}

/// A monitor's usable area, in physical pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Area {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Area {
    /// Pixels shared with the `width` x `height` rectangle at (`x`, `y`).
    fn overlap(self, x: i32, y: i32, width: u32, height: u32) -> i64 {
        let span = |from: i32, len: u32, other_from: i32, other_len: u32| {
            let end = i64::from(from) + i64::from(len);
            let other_end = i64::from(other_from) + i64::from(other_len);
            (end.min(other_end) - i64::from(from).max(i64::from(other_from))).max(0)
        };
        span(self.x, self.width, x, width) * span(self.y, self.height, y, height)
    }
}

impl Geometry {
    /// Moves and shrinks the rectangle to sit inside the monitor it overlaps most. One
    /// that overlaps none (its monitor is gone) is centered on the first monitor, which
    /// callers make the primary one. `None` for a degenerate rectangle or no monitors.
    pub fn fit(self, areas: &[Area]) -> Option<Self> {
        let first = areas.first()?;
        if self.width == 0 || self.height == 0 {
            return None;
        }
        let overlap = |a: &&Area| a.overlap(self.x, self.y, self.width, self.height);
        let best = areas.iter().max_by_key(overlap).filter(|a| overlap(a) > 0);
        let area = best.unwrap_or(first);
        let width = self.width.min(area.width);
        let height = self.height.min(area.height);
        let (x, y) = if best.is_some() {
            let shift = |at: i32, from: i32, room: u32| {
                let latest = i64::from(from) + i64::from(room);
                i32::try_from(i64::from(at).clamp(i64::from(from), latest)).unwrap_or(from)
            };
            (
                shift(self.x, area.x, area.width - width),
                shift(self.y, area.y, area.height - height),
            )
        } else {
            let middle = |from: i32, room: u32| {
                i32::try_from(i64::from(from) + i64::from(room) / 2).unwrap_or(from)
            };
            (
                middle(area.x, area.width - width),
                middle(area.y, area.height - height),
            )
        };
        Some(Self {
            x,
            y,
            width,
            height,
            maximized: self.maximized,
        })
    }
}

/// One window in `session.json`: its tabs, and its place when it is known. Files from
/// before windows remembered their place have none.
#[derive(Serialize, Deserialize)]
struct SavedWindow {
    #[serde(flatten)]
    tabs: WindowTabs,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    geometry: Option<Geometry>,
}

#[derive(Default, Serialize, Deserialize)]
struct SessionFile {
    windows: Vec<SavedWindow>,
}

#[derive(Default)]
struct State {
    /// Window label → tabs, in window creation order.
    windows: Vec<(String, WindowTabs)>,
    /// Window label → where it sits.
    geometry: HashMap<String, Geometry>,
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
    /// A delayed save of window places is waiting to run.
    flush_pending: AtomicBool,
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
            flush_pending: AtomicBool::new(false),
        };
        let mut extra = Vec::new();
        {
            let mut state = session.state.lock().unwrap();
            state.open.insert(MAIN_LABEL.to_owned());
            let restored = saved.windows.into_iter().filter_map(|w| {
                let geometry = w.geometry;
                w.tabs.sanitized().map(|tabs| (tabs, geometry))
            });
            for (i, (tabs, geometry)) in restored.enumerate() {
                let label = if i == 0 {
                    MAIN_LABEL.to_owned()
                } else {
                    let label = session.next_label();
                    state.open.insert(label.clone());
                    extra.push(label.clone());
                    label
                };
                if let Some(geometry) = geometry {
                    state.geometry.insert(label.clone(), geometry);
                }
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

    /// Where window `label` was last seen.
    pub fn geometry(&self, label: &str) -> Option<Geometry> {
        self.state.lock().unwrap().geometry.get(label).copied()
    }

    /// Records where window `label` is, in memory only: see [`Session::schedule_flush`].
    /// Returns whether that changed anything. Ignored for windows that were closed.
    pub fn set_geometry(&self, label: &str, geometry: Geometry) -> bool {
        let mut state = self.state.lock().unwrap();
        if state.closed.contains(label) {
            return false;
        }
        state.geometry.insert(label.to_owned(), geometry) != Some(geometry)
    }

    /// Claims the delayed save of window places. Dragging a window reports its place
    /// many times a second, so one save per short delay covers them all. Returns false
    /// when one is already waiting; otherwise the caller must call [`Session::flush`]
    /// after the delay.
    pub fn schedule_flush(&self) -> bool {
        !self.flush_pending.swap(true, Ordering::AcqRel)
    }

    /// Saves the session, including window places.
    pub fn flush(&self) {
        self.flush_pending.store(false, Ordering::Release);
        self.save();
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
            state.geometry.remove(label);
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
                windows: state
                    .windows
                    .iter()
                    .map(|(label, tabs)| SavedWindow {
                        tabs: tabs.clone(),
                        geometry: state.geometry.get(label).copied(),
                    })
                    .collect(),
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
            tabs: paths.iter().map(ToString::to_string).collect(),
            active,
            views: Vec::new(),
            collapsed: Vec::new(),
        }
    }

    #[test]
    fn views_stay_with_their_tabs_and_old_files_have_none() {
        let kept = WindowTabs {
            tabs: vec!["a".into(), String::new(), "c".into()],
            active: 2,
            views: vec!["list".into(), "grid".into(), "grid".into()],
            collapsed: vec!["tags".into(), String::new(), "drives,tags".into()],
        }
        .sanitized()
        .unwrap();
        assert_eq!(kept.tabs, ["a", "c"]);
        assert_eq!(kept.views, ["list", "grid"]);
        assert_eq!(kept.collapsed, ["tags", "drives,tags"]);
        // A session file from before views were saved still loads.
        let old: WindowTabs = serde_json::from_str(r#"{"tabs":["x"],"active":0}"#).unwrap();
        assert_eq!(old.views, Vec::<String>::new());
        assert_eq!(old.collapsed, Vec::<String>::new());
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
        assert_eq!(extra, [] as [String; 0]);
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
        assert_eq!(extra, [] as [String; 0]);
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
        assert_eq!(extra, [] as [String; 0]);
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

    fn place(x: i32, y: i32, width: u32, height: u32) -> Geometry {
        Geometry {
            x,
            y,
            width,
            height,
            maximized: false,
        }
    }

    const SCREEN: Area = Area {
        x: 0,
        y: 0,
        width: 1920,
        height: 1040,
    };
    /// A second monitor to the right.
    const RIGHT: Area = Area {
        x: 1920,
        y: 0,
        width: 1280,
        height: 1024,
    };

    #[test]
    fn a_window_inside_a_monitor_stays_where_it_is() {
        let g = place(100, 50, 1280, 800);
        assert_eq!(g.fit(&[SCREEN]), Some(g));
    }

    #[test]
    fn a_window_on_a_second_monitor_stays_there() {
        let g = place(2000, 100, 1000, 700);
        assert_eq!(g.fit(&[SCREEN, RIGHT]), Some(g));
    }

    #[test]
    fn a_window_whose_monitor_is_gone_is_centered_on_the_first() {
        let g = place(2000, 100, 1000, 700);
        let fitted = g.fit(&[SCREEN]).unwrap();
        assert_eq!(fitted, place(460, 170, 1000, 700));
    }

    #[test]
    fn a_window_partly_off_screen_is_moved_back_in() {
        let g = place(1500, -40, 1000, 700);
        assert_eq!(g.fit(&[SCREEN]), Some(place(920, 0, 1000, 700)));
    }

    #[test]
    fn a_window_larger_than_the_monitor_is_shrunk_to_it() {
        let g = place(-100, -100, 4000, 3000);
        assert_eq!(g.fit(&[SCREEN]), Some(place(0, 0, 1920, 1040)));
    }

    #[test]
    fn a_degenerate_rectangle_or_no_monitor_gives_nothing() {
        assert_eq!(place(0, 0, 0, 500).fit(&[SCREEN]), None);
        assert_eq!(place(0, 0, 500, 500).fit(&[]), None);
    }

    #[test]
    fn window_places_are_saved_and_restored_with_the_tabs() {
        let (_dir, file, session) = temp_session();
        session.update(MAIN_LABEL, tabs(&["C:\\a"], 0));
        session.open("w-1", tabs(&["C:\\b"], 0));
        let first = Geometry {
            maximized: true,
            ..place(10, 20, 1000, 700)
        };
        assert!(session.set_geometry(MAIN_LABEL, first));
        assert!(session.set_geometry("w-1", place(300, 40, 900, 600)));
        assert!(
            !session.set_geometry("w-1", place(300, 40, 900, 600)),
            "unchanged"
        );
        session.flush();

        let (restored, extra) = Session::load(Some(file));
        assert_eq!(restored.geometry(MAIN_LABEL), Some(first));
        assert_eq!(restored.geometry(&extra[0]), Some(place(300, 40, 900, 600)));
    }

    #[test]
    fn a_session_file_without_places_still_loads() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("session.json");
        std::fs::write(&file, r#"{"windows":[{"tabs":["C:\\a"],"active":0}]}"#).unwrap();

        let (session, _) = Session::load(Some(file));

        assert_eq!(session.tabs(MAIN_LABEL), Some(tabs(&["C:\\a"], 0)));
        assert_eq!(session.geometry(MAIN_LABEL), None);
    }

    #[test]
    fn a_closed_window_forgets_its_place_and_ignores_late_reports() {
        let (_dir, _file, session) = temp_session();
        session.update(MAIN_LABEL, tabs(&["C:\\a"], 0));
        session.open("w-1", tabs(&["C:\\b"], 0));
        session.set_geometry("w-1", place(1, 2, 800, 600));
        session.close("w-1");

        assert_eq!(session.geometry("w-1"), None);
        assert!(!session.set_geometry("w-1", place(1, 2, 800, 600)));
    }

    #[test]
    fn only_one_delayed_save_is_pending_at_a_time() {
        let (_dir, _file, session) = temp_session();
        assert!(session.schedule_flush());
        assert!(!session.schedule_flush());
        session.flush();
        assert!(session.schedule_flush());
    }

    #[test]
    fn corrupt_file_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("session.json");
        std::fs::write(&file, "{ not json").unwrap();
        let (session, extra) = Session::load(Some(file));
        assert_eq!(extra, [] as [String; 0]);
        assert_eq!(session.tabs(MAIN_LABEL), None);
    }
}
