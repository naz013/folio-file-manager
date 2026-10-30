//! Watches the folder shown in the UI and notifies the frontend about changes.

use notify_debouncer_mini::notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_mini::{DebounceEventResult, Debouncer, new_debouncer};
use serde::Serialize;
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

pub const DIR_CHANGED: &str = "dir-changed";

#[derive(Clone, Serialize)]
pub struct DirChanged {
    pub path: String,
}

/// Holds the single active watcher. Replacing it stops the previous one.
#[derive(Default)]
pub struct DirWatcher(Mutex<Option<Debouncer<RecommendedWatcher>>>);

impl DirWatcher {
    pub fn watch(&self, app: AppHandle, dir: &Path) -> notify_debouncer_mini::notify::Result<()> {
        let path = dir.to_string_lossy().into_owned();
        let mut debouncer = new_debouncer(
            Duration::from_millis(250),
            move |res: DebounceEventResult| {
                if res.is_ok() {
                    let _ = app.emit(DIR_CHANGED, DirChanged { path: path.clone() });
                }
            },
        )?;
        debouncer
            .watcher()
            .watch(dir, RecursiveMode::NonRecursive)?;
        *self.0.lock().unwrap() = Some(debouncer);
        Ok(())
    }
}
