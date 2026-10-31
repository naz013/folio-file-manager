//! Watches the folder shown in each window and notifies that window about changes.

use notify_debouncer_mini::notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_mini::{DebounceEventResult, Debouncer, new_debouncer};
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

pub const DIR_CHANGED: &str = "dir-changed";

#[derive(Clone, Serialize)]
pub struct DirChanged {
    pub path: String,
}

/// One active watcher per window label. Replacing a window's watcher stops the previous one.
#[derive(Default)]
pub struct DirWatcher(Mutex<HashMap<String, Debouncer<RecommendedWatcher>>>);

impl DirWatcher {
    pub fn watch(
        &self,
        app: AppHandle,
        window: &str,
        dir: &Path,
    ) -> notify_debouncer_mini::notify::Result<()> {
        let path = dir.to_string_lossy().into_owned();
        let target = window.to_owned();
        let mut debouncer = new_debouncer(
            Duration::from_millis(250),
            move |res: DebounceEventResult| {
                if res.is_ok() {
                    let _ = app.emit_to(
                        target.as_str(),
                        DIR_CHANGED,
                        DirChanged { path: path.clone() },
                    );
                }
            },
        )?;
        debouncer
            .watcher()
            .watch(dir, RecursiveMode::NonRecursive)?;
        self.0.lock().unwrap().insert(window.to_owned(), debouncer);
        Ok(())
    }

    /// Stops watching for a closed window.
    pub fn unwatch(&self, window: &str) {
        self.0.lock().unwrap().remove(window);
    }
}
