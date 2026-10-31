//! Thin Tauri command layer. Validation and error mapping only; logic lives in modules.
//!
//! File operations run on the blocking thread pool so the UI never freezes.

use crate::fs::{self, Entry};
use crate::ops;
use crate::session::{Session, WindowTabs};
use crate::watch::DirWatcher;
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
    session: tauri::State<'_, Session>,
    tabs: Vec<String>,
    x: f64,
    y: f64,
) -> CmdResult<()> {
    let label = session.next_label();
    session.open(&label, WindowTabs { tabs, active: 0 });
    crate::window::open(&app, &label, Some((x, y))).map_err(|e| {
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

#[tauri::command]
pub async fn trash_paths(targets: Vec<String>) -> CmdResult<()> {
    blocking(move || ops::trash(&paths(targets)).map_err(|e| e.to_string())).await
}

#[tauri::command]
pub async fn rename_path(path: String, new_name: String) -> CmdResult<String> {
    blocking(move || {
        ops::rename(&PathBuf::from(path), &new_name)
            .map(|p| p.to_string_lossy().into_owned())
            .map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn create_folder(parent: String, name: String) -> CmdResult<String> {
    blocking(move || {
        ops::create_folder(&PathBuf::from(parent), &name)
            .map(|p| p.to_string_lossy().into_owned())
            .map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn copy_paths(sources: Vec<String>, dest: String) -> CmdResult<Vec<String>> {
    blocking(move || {
        ops::copy_into(&paths(sources), &PathBuf::from(dest))
            .map(strings)
            .map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn move_paths(sources: Vec<String>, dest: String) -> CmdResult<Vec<String>> {
    blocking(move || {
        ops::move_into(&paths(sources), &PathBuf::from(dest))
            .map(strings)
            .map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub fn perf_config() -> crate::perf::PerfConfig {
    crate::perf::config()
}

#[tauri::command]
pub fn perf_report(event: String) {
    crate::perf::report(&event);
}
