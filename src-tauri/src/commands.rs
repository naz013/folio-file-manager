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
use crate::queue::{self, Kind, Queue};
use crate::session::{Session, WindowTabs};
use crate::watch::DirWatcher;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
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
                    // Copy or move is the target's call, from the keys held during the drag.
                    drag::Options {
                        mode: drag::DragMode::CopyOrMove,
                        ..drag::Options::default()
                    },
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

/// Which modifier keys are held down right now.
#[derive(Serialize)]
pub struct Modifiers {
    ctrl: bool,
    shift: bool,
    /// Space is not a modifier, but a drag owns the keyboard, so the page asks for it here.
    space: bool,
}

/// The modifier keys held now. A native drop reaches the page without them, so the page
/// asks while the drag is over it: Ctrl forces a copy and Shift a move, as in Explorer.
/// Only implemented on Windows; elsewhere a drop follows the drive rule alone.
#[tauri::command]
pub fn modifier_keys() -> Modifiers {
    #[cfg(windows)]
    {
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            GetAsyncKeyState, VK_CONTROL, VK_SHIFT, VK_SPACE,
        };
        // The high bit of the result is set while the key is down.
        let down = |key: u16| {
            // SAFETY: plain query of the keyboard state; no pointers are involved.
            unsafe { GetAsyncKeyState(i32::from(key)) < 0 }
        };
        Modifiers {
            ctrl: down(VK_CONTROL.0),
            shift: down(VK_SHIFT.0),
            space: down(VK_SPACE.0),
        }
    }
    #[cfg(not(windows))]
    {
        Modifiers {
            ctrl: false,
            shift: false,
            space: false,
        }
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

/// Moves `targets` to the Recycle Bin and returns the result with the journal steps for
/// whatever really left its place, even when the call reported an error.
fn trash_with_steps(journal: &Journal, targets: &[PathBuf]) -> (CmdResult<()>, Vec<Step>) {
    let started = journal::unix_now() - 2;
    // Only what is there to trash can end up in the bin.
    let existing: Vec<PathBuf> = targets
        .iter()
        .filter(|p| std::fs::symlink_metadata(p).is_ok())
        .cloned()
        .collect();
    let result = ops::trash(targets).map_err(|e| e.to_string());
    let gone: Vec<PathBuf> = existing
        .into_iter()
        .filter(|p| std::fs::symlink_metadata(p).is_err())
        .collect();
    let ids = journal::trash_ids(&gone, started, &journal.trash_ids_in_use());
    let steps = gone
        .into_iter()
        .zip(ids)
        .map(|(original, id)| Step::Trash { original, id })
        .collect();
    (result, steps)
}

/// Replacing promises the old item can be brought back. If one left its place without a
/// Recycle Bin entry to restore, it may have been deleted for good: stop before copying.
fn every_one_in_the_bin(steps: &[Step]) -> CmdResult<()> {
    if steps
        .iter()
        .any(|s| matches!(s, Step::Trash { id: None, .. }))
    {
        Err(
            "An item that was to be replaced may have been deleted instead of moved to the \
             Recycle Bin. Nothing was copied."
                .into(),
        )
    } else {
        Ok(())
    }
}

/// The items of `sources` whose names are taken in `dest`, with both sides to compare.
#[tauri::command]
pub async fn find_conflicts(
    sources: Vec<String>,
    dest: String,
) -> CmdResult<Vec<ops::conflicts::Conflict>> {
    blocking(move || Ok(ops::conflicts::find(&paths(sources), &PathBuf::from(dest)))).await
}

/// Starts moving `targets` to the Recycle Bin in the background; returns the job id.
#[tauri::command(async)]
pub fn trash_paths(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    queue: tauri::State<'_, Arc<Queue>>,
    targets: Vec<String>,
) -> u64 {
    let targets = paths(targets);
    let label = describe("Move to Recycle Bin:", &targets);
    let lane = targets
        .first()
        .map_or_else(String::new, |p| ops::volume_key(p.parent().unwrap_or(p)));
    let title = label.clone();
    // What each item was when it was chosen: the job may run much later.
    let chosen: Vec<Option<String>> = targets.iter().map(|p| ops::identity(p)).collect();
    let handle = app.clone();
    let work: queue::Work = Box::new(move |progress| {
        let journal = handle.state::<Journal>();
        progress.set_totals(0, targets.len() as u64);
        let pending = begin(&journal, &label)?;
        // Only what is still there, and still the same item, is trashed.
        let (targets, changed): (Vec<_>, Vec<_>) = targets
            .into_iter()
            .zip(chosen)
            .partition(|(path, was)| was.is_none() || ops::identity(path).as_ref() == was.as_ref());
        let targets: Vec<PathBuf> = targets.into_iter().map(|(path, _)| path).collect();
        let left_alone = changed.len();
        let (result, steps) = trash_with_steps(&journal, &targets);
        journal.finish(pending, &steps);
        for _ in &steps {
            ops::progress::add_item();
        }
        result.map(|()| serde_json::json!({ "count": steps.len(), "leftAlone": left_alone }))
    });
    queue.submit(&app, window.label(), Kind::Trash, title, &lane, work)
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

/// Starts copying `sources` into `dest` in the background; returns the job id.
#[tauri::command(async)]
pub fn copy_paths(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    queue: tauri::State<'_, Arc<Queue>>,
    sources: Vec<String>,
    dest: String,
    replace: Vec<String>,
    replace_ids: Vec<Option<String>>,
) -> u64 {
    let sources = paths(sources);
    let dest = PathBuf::from(dest);
    let replace: Vec<(PathBuf, Option<String>)> =
        paths(replace).into_iter().zip(replace_ids).collect();
    let label = describe("Copy", &sources);
    let lane = ops::volume_key(&dest);
    let title = label.clone();
    let handle = app.clone();
    let work: queue::Work = Box::new(move |progress| {
        let journal = handle.state::<Journal>();
        progress.set_totals(ops::total_bytes(&sources), sources.len() as u64);
        let doomed =
            ops::conflicts::to_replace(&sources, &replace, &dest).map_err(|e| e.to_string())?;
        let pending = begin(&journal, &label)?;
        // What is replaced goes to the Recycle Bin first, so its name is free.
        let (trashed, mut steps) = trash_with_steps(&journal, &doomed);
        let trashed = trashed.and_then(|()| every_one_in_the_bin(&steps));
        let mut done = ops::Copied::default();
        let result = trashed.and_then(|()| {
            ops::copy_into_with(&sources, &dest, &mut done).map_err(|e| e.to_string())
        });
        steps.extend(
            done.pairs
                .iter()
                .map(|(from, to)| Step::copied(from.clone(), to.clone())),
        );
        journal.finish(pending, &steps);
        result.map(|()| {
            serde_json::to_value(Copied {
                dest: dest.to_string_lossy().into_owned(),
                paths: strings(done.paths),
                skipped: strings(done.skipped),
                not_repointed: strings(done.not_repointed),
            })
            .unwrap_or_default()
        })
    });
    queue.submit(&app, window.label(), Kind::Copy, title, &lane, work)
}

/// `ops::Copied` for the UI.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Copied {
    dest: String,
    paths: Vec<String>,
    skipped: Vec<String>,
    not_repointed: Vec<String>,
}

/// `ops::Moved` for the UI.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Moved {
    dest: String,
    paths: Vec<String>,
    left_behind: Vec<String>,
    not_repointed: Vec<String>,
}

/// Starts moving `sources` into `dest` in the background; returns the job id.
#[tauri::command(async)]
pub fn move_paths(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    queue: tauri::State<'_, Arc<Queue>>,
    sources: Vec<String>,
    dest: String,
    replace: Vec<String>,
    replace_ids: Vec<Option<String>>,
) -> u64 {
    let sources = paths(sources);
    let dest = PathBuf::from(dest);
    let replace: Vec<(PathBuf, Option<String>)> =
        paths(replace).into_iter().zip(replace_ids).collect();
    let label = describe("Move", &sources);
    let lane = ops::volume_key(&dest);
    let title = label.clone();
    let dest_volume = lane.clone();
    let handle = app.clone();
    let work: queue::Work = Box::new(move |progress| {
        let journal = handle.state::<Journal>();
        // On one drive a move is a rename, which takes no time to show progress for.
        let same_drive = sources
            .iter()
            .all(|s| ops::volume_key(s.parent().unwrap_or(s)) == dest_volume);
        let bytes = if same_drive {
            0
        } else {
            ops::total_bytes(&sources)
        };
        progress.set_totals(bytes, sources.len() as u64);
        let doomed =
            ops::conflicts::to_replace(&sources, &replace, &dest).map_err(|e| e.to_string())?;
        let pending = begin(&journal, &label)?;
        // What is replaced goes to the Recycle Bin first, so its name is free.
        let (trashed, mut steps) = trash_with_steps(&journal, &doomed);
        let trashed = trashed.and_then(|()| every_one_in_the_bin(&steps));
        let mut done = ops::Moved::default();
        let result = trashed.and_then(|()| {
            ops::move_into_with(&sources, &dest, &mut done).map_err(|e| e.to_string())
        });
        steps.extend(
            done.pairs
                .iter()
                .map(|(from, to)| Step::moved(from.clone(), to.clone())),
        );
        journal.finish(pending, &steps);
        result.map(|()| {
            serde_json::to_value(Moved {
                dest: dest.to_string_lossy().into_owned(),
                paths: strings(done.paths),
                left_behind: strings(done.left_behind),
                not_repointed: strings(done.not_repointed),
            })
            .unwrap_or_default()
        })
    });
    queue.submit(&app, window.label(), Kind::Move, title, &lane, work)
}

#[tauri::command]
pub fn queue_pause(app: tauri::AppHandle, queue: tauri::State<'_, Arc<Queue>>, id: u64) {
    queue.pause(&app, id);
}

#[tauri::command]
pub fn queue_resume(app: tauri::AppHandle, queue: tauri::State<'_, Arc<Queue>>, id: u64) {
    queue.resume(&app, id);
}

#[tauri::command]
pub fn queue_cancel(app: tauri::AppHandle, queue: tauri::State<'_, Arc<Queue>>, id: u64) {
    queue.cancel(&app, id);
}

#[tauri::command]
pub fn queue_dismiss(queue: tauri::State<'_, Arc<Queue>>, id: u64) {
    queue.dismiss(id);
}

/// Every job in the queue, for a window that opens while operations are running.
#[tauri::command]
pub fn queue_jobs(queue: tauri::State<'_, Arc<Queue>>) -> Vec<queue::JobView> {
    queue.list()
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
    blocking(move || {
        wait_for_queue(&app)?;
        app.state::<Journal>().undo()
    })
    .await
}

/// Undo and redo act on what the queue's jobs are about to change, so they wait.
fn wait_for_queue(app: &tauri::AppHandle) -> CmdResult<()> {
    if app.state::<Arc<Queue>>().is_busy() {
        Err("Wait for the running operations to finish first".into())
    } else {
        Ok(())
    }
}

/// Redoes the latest undone operation; returns its name.
#[tauri::command]
pub async fn redo(app: tauri::AppHandle) -> CmdResult<String> {
    blocking(move || {
        wait_for_queue(&app)?;
        app.state::<Journal>().redo()
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
