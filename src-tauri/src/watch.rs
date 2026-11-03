//! Watches the folder shown in each window and notifies that window about changes.

use notify_debouncer_mini::notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_mini::{DebounceEventResult, Debouncer, new_debouncer};
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tauri::{AppHandle, Emitter};

pub const DIR_CHANGED: &str = "dir-changed";

#[derive(Clone, Serialize)]
pub struct DirChanged {
    pub path: String,
}

/// One active watcher per window label. Replacing a window's watcher stops the previous one.
#[derive(Default)]
pub struct DirWatcher {
    /// The watcher of each window, with the number of the request that made it.
    active: Mutex<HashMap<String, (u64, Debouncer<RecommendedWatcher>)>>,
    requests: AtomicU64,
}

impl DirWatcher {
    /// Watches `dir` for `window`. Starting a watch can take long on a slow share, so two
    /// requests may finish out of order: the one asked for last wins, a late older one is
    /// dropped.
    pub fn watch(
        &self,
        app: AppHandle,
        window: &str,
        dir: &Path,
    ) -> notify_debouncer_mini::notify::Result<()> {
        let ticket = self.requests.fetch_add(1, Ordering::Relaxed);
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
        // The replaced watcher is stopped after the lock is let go: stopping one on a share
        // that went away can wait.
        let replaced = {
            let mut active = self.active.lock().unwrap();
            match active.get(window) {
                Some((newer, _)) if *newer > ticket => Some((ticket, debouncer)),
                _ => active.insert(window.to_owned(), (ticket, debouncer)),
            }
        };
        drop(replaced);
        Ok(())
    }

    /// Stops watching for a closed window.
    pub fn unwatch(&self, window: &str) {
        let removed = self.active.lock().unwrap().remove(window);
        drop(removed);
    }
}
