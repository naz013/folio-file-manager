//! Thin Tauri command layer. Validation and error mapping only; logic lives in modules.
//!
//! File operations run on the blocking thread pool so the UI never freezes.
#![expect(
    clippy::needless_pass_by_value,
    reason = "Tauri deserializes command arguments and injects state by value"
)]

use crate::fs::{self, Entry};
use crate::journal::{self, Journal, Step};
use crate::ops;
use crate::session::{Session, WindowTabs};
use crate::watch::DirWatcher;
use serde::Serialize;
use std::path::PathBuf;
use tauri::Manager;

type CmdResult<T> = Result<T, String>;

fn paths(list: Vec<String>) -> Vec<PathBuf> {
    list.into_iter().map(PathBuf::from).collect()
}

fn strings(list: Vec<PathBuf>) -> Vec<String> {
    list.into_iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect()
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> CmdResult<T> + Send + 'static,
) -> CmdResult<T> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn list_dir(path: String) -> CmdResult<Vec<Entry>> {
    fs::list_dir(&PathBuf::from(&path)).map_err(|e| format!("Cannot open {path}: {e}"))
}

#[tauri::command]
pub fn home_dir(app: tauri::AppHandle) -> CmdResult<String> {
    app.path()
        .home_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn watch_dir(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    watcher: tauri::State<'_, DirWatcher>,
    path: String,
) -> CmdResult<()> {
    watcher
        .watch(app, window.label(), &PathBuf::from(path))
        .map_err(|e| e.to_string())
}

/// Tabs to restore in the calling window, if the session has any for it.
#[tauri::command]
pub fn window_tabs(
    window: tauri::WebviewWindow,
    session: tauri::State<'_, Session>,
) -> Option<WindowTabs> {
    session.tabs(window.label())
}

/// Saves the calling window's tabs. The file write runs off the main thread.
#[tauri::command]
pub async fn save_tabs(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    tabs: Vec<String>,
    active: usize,
) -> CmdResult<()> {
    let label = window.label().to_owned();
    blocking(move || {
        app.state::<Session>()
            .update(&label, WindowTabs { tabs, active });
        Ok(())
    })
    .await
}

/// Opens a new window with `tabs`, its top-left corner at logical screen position (x, y).
/// Async: creating a window from a synchronous command deadlocks on Windows.
#[tauri::command]
pub async fn open_window(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    session: tauri::State<'_, Session>,
    tabs: Vec<String>,
    x: f64,
    y: f64,
) -> CmdResult<()> {
    // The new window is as big as the one the tab came from.
    // A maximized window is as big as the screen: use the size it returns to.
    let size = match session.geometry(window.label()).filter(|g| g.maximized) {
        Some(g) if window.is_maximized().unwrap_or(false) => {
            tauri::PhysicalSize::new(g.width, g.height)
        }
        _ => window.inner_size().map_err(|e| e.to_string())?,
    };
    let scale = window.scale_factor().map_err(|e| e.to_string())?;
    let size = size.to_logical::<f64>(scale);
    let place = crate::window::Place::TornOff {
        x,
        y,
        width: size.width,
        height: size.height,
    };
    let label = session.next_label();
    session.open(&label, WindowTabs { tabs, active: 0 });
    crate::window::open(&app, &label, &place).map_err(|e| {
        session.close(&label);
        e.to_string()
    })
}

#[tauri::command]
pub fn open_path(path: String) -> CmdResult<()> {
    tauri_plugin_opener::open_path(&path, None::<&str>).map_err(|e| e.to_string())
}

/// Starts a native OS drag of `paths`, so files can be dropped into Explorer and other apps.
/// Drops back onto Folio arrive through the webview's regular drag-drop events.
#[tauri::command]
pub async fn start_drag(window: tauri::WebviewWindow, paths: Vec<String>) -> CmdResult<()> {
    #[cfg(not(target_os = "linux"))]
    {
        let files: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
        let first = files.first().cloned().ok_or("Nothing to drag")?;
        let preview = blocking(move || {
            crate::platform::thumbnail_png(&first, 64)
                .or_else(|_| crate::platform::encode_png(1, 1, &[0, 0, 0, 0]))
        })
        .await?;
        let target = window.clone();
        window
            .run_on_main_thread(move || {
                if let Err(e) = drag::start_drag(
                    &target,
                    drag::DragItem::Files(files),
                    drag::Image::Raw(preview),
                    |_, _| {},
                    drag::Options::default(),
                ) {
                    eprintln!("native drag failed: {e}");
                }
            })
            .map_err(|e| e.to_string())
    }
    #[cfg(target_os = "linux")]
    {
        let _ = (window, paths);
        Err("Dragging to other apps is not supported on Linux yet".into())
    }
}

/// Shows the native OS context menu for `paths` at the cursor.
/// `extended` adds the Shift+right-click verbs (e.g. "Copy as path").
#[tauri::command]
pub fn show_context_menu(
    window: tauri::WebviewWindow,
    paths: Vec<String>,
    extended: bool,
) -> CmdResult<()> {
    #[cfg(windows)]
    {
        if paths.is_empty() {
            return Ok(());
        }
        let hwnd = window.hwnd().map_err(|e| e.to_string())?.0 as isize;
        let files: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
        window
            .run_on_main_thread(move || {
                if let Err(e) = crate::platform::show_context_menu(hwnd, &files, extended) {
                    eprintln!("context menu failed: {e}");
                }
            })
            .map_err(|e| e.to_string())
    }
    #[cfg(not(windows))]
    {
        let _ = (window, paths, extended);
        Err("The native context menu is only available on Windows for now".into())
    }
}

/// "Move "a.txt"" or "Move 3 items": what an undo entry is called.
fn describe(verb: &str, items: &[PathBuf]) -> String {
    match items {
        [one] => format!(
            "{verb} \"{}\"",
            one.file_name().map_or_else(
                || one.display().to_string(),
                |n| n.to_string_lossy().into_owned()
            )
        ),
        many => format!("{verb} {} items", many.len()),
    }
}

/// Writes the journal entry that must exist before an operation runs.
fn begin(journal: &Journal, label: &str) -> Result<journal::Pending, String> {
    journal.begin(label).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn trash_paths(app: tauri::AppHandle, targets: Vec<String>) -> CmdResult<()> {
    blocking(move || {
        let journal = app.state::<Journal>();
        let targets = paths(targets);
        let pending = begin(&journal, &describe("Move to Recycle Bin:", &targets))?;
        let started = journal::unix_now() - 2;
        // Only what is there to trash can end up in the bin.
        let existing: Vec<PathBuf> = targets
            .iter()
            .filter(|p| std::fs::symlink_metadata(p).is_ok())
            .cloned()
            .collect();
        let result = ops::trash(&targets).map_err(|e| e.to_string());
        // Whatever left its place is in the bin, even if the call reported an error.
        let gone: Vec<PathBuf> = existing
            .into_iter()
            .filter(|p| std::fs::symlink_metadata(p).is_err())
            .collect();
        let ids = journal::trash_ids(&gone, started, &journal.trash_ids_in_use());
        let steps: Vec<Step> = gone
            .into_iter()
            .zip(ids)
            .map(|(original, id)| Step::Trash { original, id })
            .collect();
        journal.finish(pending, &steps);
        result
    })
    .await
}

#[tauri::command]
pub async fn rename_path(
    app: tauri::AppHandle,
    path: String,
    new_name: String,
) -> CmdResult<String> {
    blocking(move || {
        let journal = app.state::<Journal>();
        let from = PathBuf::from(path);
        let pending = begin(&journal, &describe("Rename", std::slice::from_ref(&from)))?;
        let result = ops::rename(&from, &new_name);
        let steps: Vec<Step> = result
            .as_ref()
            .map(|to| Step::rename(from.clone(), to.clone()))
            .into_iter()
            .collect();
        journal.finish(pending, &steps);
        result
            .map(|p| p.to_string_lossy().into_owned())
            .map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn create_folder(
    app: tauri::AppHandle,
    parent: String,
    name: String,
) -> CmdResult<String> {
    blocking(move || {
        let journal = app.state::<Journal>();
        let pending = begin(&journal, &format!("New folder \"{name}\""))?;
        let result = ops::create_folder(&PathBuf::from(parent), &name);
        let steps: Vec<Step> = result
            .as_ref()
            .map(|path| Step::create_folder(path.clone()))
            .into_iter()
            .collect();
        journal.finish(pending, &steps);
        result
            .map(|p| p.to_string_lossy().into_owned())
            .map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn copy_paths(
    app: tauri::AppHandle,
    sources: Vec<String>,
    dest: String,
) -> CmdResult<Copied> {
    blocking(move || {
        let journal = app.state::<Journal>();
        let sources = paths(sources);
        let pending = begin(&journal, &describe("Copy", &sources))?;
        let mut done = ops::Copied::default();
        let result = ops::copy_into_with(&sources, &PathBuf::from(dest), &mut done);
        let steps: Vec<Step> = done
            .pairs
            .iter()
            .map(|(from, to)| Step::copied(from.clone(), to.clone()))
            .collect();
        journal.finish(pending, &steps);
        result
            .map(|()| Copied {
                paths: strings(done.paths),
                skipped: strings(done.skipped),
                not_repointed: strings(done.not_repointed),
            })
            .map_err(|e| e.to_string())
    })
    .await
}

/// `ops::Copied` for the UI.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Copied {
    paths: Vec<String>,
    skipped: Vec<String>,
    not_repointed: Vec<String>,
}

/// `ops::Moved` for the UI.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Moved {
    paths: Vec<String>,
    left_behind: Vec<String>,
    not_repointed: Vec<String>,
}

#[tauri::command]
pub async fn move_paths(
    app: tauri::AppHandle,
    sources: Vec<String>,
    dest: String,
) -> CmdResult<Moved> {
    blocking(move || {
        let journal = app.state::<Journal>();
        let sources = paths(sources);
        let pending = begin(&journal, &describe("Move", &sources))?;
        let mut done = ops::Moved::default();
        let result = ops::move_into_with(&sources, &PathBuf::from(dest), &mut done);
        let steps: Vec<Step> = done
            .pairs
            .iter()
            .map(|(from, to)| Step::moved(from.clone(), to.clone()))
            .collect();
        journal.finish(pending, &steps);
        result
            .map(|()| Moved {
                paths: strings(done.paths),
                left_behind: strings(done.left_behind),
                not_repointed: strings(done.not_repointed),
            })
            .map_err(|e| e.to_string())
    })
    .await
}

/// What Ctrl+Z and Ctrl+Y would do now, for the UI to show.
#[derive(Serialize)]
pub struct UndoState {
    undo: Option<String>,
    redo: Option<String>,
}

#[tauri::command]
pub async fn undo_state(app: tauri::AppHandle) -> CmdResult<UndoState> {
    blocking(move || {
        let (undo, redo) = app.state::<Journal>().next();
        Ok(UndoState { undo, redo })
    })
    .await
}

/// Undoes the latest operation; returns its name.
#[tauri::command]
pub async fn undo(app: tauri::AppHandle) -> CmdResult<String> {
    blocking(move || app.state::<Journal>().undo()).await
}

/// Redoes the latest undone operation; returns its name.
#[tauri::command]
pub async fn redo(app: tauri::AppHandle) -> CmdResult<String> {
    blocking(move || app.state::<Journal>().redo()).await
}

#[tauri::command]
pub fn perf_config() -> crate::perf::PerfConfig {
    crate::perf::config()
}

#[tauri::command]
pub fn perf_report(event: String) {
    crate::perf::report(&event);
}
