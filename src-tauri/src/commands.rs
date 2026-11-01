//! Thin Tauri command layer. Validation and error mapping only; logic lives in modules.
//!
//! File operations run on the blocking thread pool so the UI never freezes.
#![expect(
    clippy::needless_pass_by_value,
    reason = "Tauri deserializes command arguments and injects state by value"
)]

use crate::cleanup;
use crate::fs::{self, Entry};
use crate::journal::{self, Journal, Step};
use crate::ops;
use crate::queue::{self, Kind, Queue};
use crate::scan::{BigFile, RootInfo, Scanner, Sized};
use crate::session::{Session, WindowTabs};
use crate::shelf::{Shelf, ShelfItem};
use crate::tags::{Tag, Tags};
use crate::watch::DirWatcher;
use serde::{Deserialize, Serialize};
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
    views: Vec<String>,
) -> CmdResult<()> {
    let label = window.label().to_owned();
    blocking(move || {
        app.state::<Session>().update(
            &label,
            WindowTabs {
                tabs,
                active,
                views,
            },
        );
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
    views: Vec<String>,
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
    session.open(
        &label,
        WindowTabs {
            tabs,
            active: 0,
            views,
        },
    );
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

/// Names the one file the webview may read through `preview://`, replacing the last. `None`
/// closes Quick Look's access altogether.
#[tauri::command]
pub fn preview_allow(current: tauri::State<'_, crate::preview::Current>, path: Option<String>) {
    current.set(path.map(PathBuf::from));
}

/// The start of a text file for Quick Look; fails for files that are not text.
#[tauri::command(async)]
pub fn read_text_preview(path: String) -> CmdResult<crate::preview::TextPreview> {
    crate::preview::read_text_preview(&PathBuf::from(&path))
        .map_err(|e| format!("Cannot preview {path}: {e}"))
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
        if let Ok(to) = &result {
            app.state::<Tags>().relocated(&from, to);
        }
        result
            .map(|p| p.to_string_lossy().into_owned())
            .map_err(|e| e.to_string())
    })
    .await
}

/// One item of a batch rename and the name it should get.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenamePlan {
    path: String,
    new_name: String,
}

fn planned(plan: Vec<RenamePlan>) -> Vec<ops::bulk_rename::Planned> {
    plan.into_iter()
        .map(|p| ops::bulk_rename::Planned {
            path: PathBuf::from(p.path),
            new_name: p.new_name,
        })
        .collect()
}

/// Why each rename of a batch can't be done (`null` where it can), checked as a whole.
#[tauri::command]
pub async fn check_renames(plan: Vec<RenamePlan>) -> CmdResult<Vec<Option<String>>> {
    blocking(move || Ok(ops::bulk_rename::check(&planned(plan)))).await
}

/// Renames a batch as one undo step. Nothing changes if any name is refused, and a batch
/// that fails part way is put back. Returns the new path of each item, in order.
#[tauri::command]
pub async fn rename_many(app: tauri::AppHandle, plan: Vec<RenamePlan>) -> CmdResult<Vec<String>> {
    blocking(move || {
        let plan = planned(plan);
        let journal = app.state::<Journal>();
        let items: Vec<PathBuf> = plan.iter().map(|p| p.path.clone()).collect();
        let pending = begin(&journal, &describe("Rename", &items))?;
        let (done, result) = ops::bulk_rename::apply(&plan);
        // An item parked under a temporary name is no longer there, so its identity is
        // read from where it ended up: the later step that takes it from the temp name.
        let mut ids: std::collections::HashMap<&PathBuf, Option<String>> =
            std::collections::HashMap::new();
        let mut steps: Vec<Step> = done
            .iter()
            .rev()
            .map(|(from, to)| {
                let id = ops::identity(to).or_else(|| ids.get(to).cloned().flatten());
                ids.insert(from, id.clone());
                Step::Rename {
                    from: from.clone(),
                    to: to.clone(),
                    id,
                }
            })
            .collect();
        steps.reverse();
        journal.finish(pending, &steps);
        app.state::<Tags>().relocated_all(&done);
        result?;
        Ok(plan
            .iter()
            .map(|p| {
                p.path
                    .parent()
                    .map_or_else(|| PathBuf::from(&p.new_name), |d| d.join(&p.new_name))
                    .to_string_lossy()
                    .into_owned()
            })
            .collect())
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
pub async fn create_file(app: tauri::AppHandle, parent: String, name: String) -> CmdResult<String> {
    blocking(move || {
        let journal = app.state::<Journal>();
        let pending = begin(&journal, &format!("New file \"{name}\""))?;
        let result = ops::create_file(&PathBuf::from(parent), &name);
        let steps: Vec<Step> = result
            .as_ref()
            .map(|path| Step::create_file(path.clone()))
            .into_iter()
            .collect();
        journal.finish(pending, &steps);
        result
            .map(|p| p.to_string_lossy().into_owned())
            .map_err(|e| e.to_string())
    })
    .await
}

/// Opens a terminal in `dir`.
#[tauri::command(async)]
pub fn open_terminal(dir: String) -> CmdResult<()> {
    crate::platform::open_terminal(&PathBuf::from(dir))
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
        handle.state::<Tags>().relocated_all(&done.pairs);
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
        let result = app.state::<Journal>().undo();
        follow_relocations(&app);
        result
    })
    .await
}

/// What an undo or redo moved, even partly, keeps its tags.
fn follow_relocations(app: &tauri::AppHandle) {
    let moves = app.state::<Journal>().take_relocations();
    app.state::<Tags>().relocated_all(&moves);
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
        let result = app.state::<Journal>().redo();
        follow_relocations(&app);
        result
    })
    .await
}

// ---- Tags ----

#[tauri::command]
pub fn tag_list(tags: tauri::State<'_, Tags>) -> Vec<Tag> {
    tags.list()
}

#[tauri::command]
pub fn tag_create(
    tags: tauri::State<'_, Tags>,
    name: String,
    color: Option<String>,
) -> CmdResult<Tag> {
    tags.create(&name, color.as_deref())
}

#[tauri::command]
pub fn tag_delete(tags: tauri::State<'_, Tags>, id: i64) -> CmdResult<()> {
    tags.delete(id)
}

/// Gives the tag to `paths`, or takes it from them if they all have it.
#[tauri::command(async)]
pub fn tag_toggle(
    tags: tauri::State<'_, Tags>,
    paths: Vec<String>,
    tag: i64,
) -> CmdResult<crate::tags::Toggled> {
    tags.toggle(&self::paths(paths), tag)
}

/// Every item with the tag, from any folder: (path, entry, the item's tag ids).
#[tauri::command(async)]
pub fn tag_files(tags: tauri::State<'_, Tags>, tag: i64) -> Vec<(String, Entry, Vec<i64>)> {
    tags.files_with(tag)
        .into_iter()
        .map(|(path, entry, ids)| (path.to_string_lossy().into_owned(), entry, ids))
        .collect()
}

/// The tagged items directly in `dir`: (name, tag ids).
#[tauri::command(async)]
pub fn tags_in_dir(tags: tauri::State<'_, Tags>, dir: String) -> Vec<(String, Vec<i64>)> {
    tags.in_dir(&PathBuf::from(dir))
}

// ---- System clipboard (Windows) ----

/// Mirrors the latest cut or copy to the system clipboard so Explorer can paste it.
#[tauri::command(async)]
pub fn clipboard_mirror(paths: Vec<String>, cut: bool) -> CmdResult<()> {
    crate::platform::clipboard_write_files(&self::paths(paths), cut)
}

/// Files cut or copied in another program since the last look: (paths, cut).
#[tauri::command(async)]
pub fn clipboard_take() -> Option<(Vec<String>, bool)> {
    crate::platform::clipboard_take_files().map(|(paths, cut)| (strings(paths), cut))
}

// ---- Recycle Bin ----

#[tauri::command]
pub async fn bin_list() -> CmdResult<Vec<crate::bin::BinItem>> {
    blocking(crate::bin::list).await
}

/// What the items weigh; slower than the listing, so asked for after it.
#[tauri::command]
pub async fn bin_sizes() -> CmdResult<Vec<crate::bin::BinSize>> {
    blocking(crate::bin::sizes).await
}

#[tauri::command]
pub async fn bin_restore(items: Vec<crate::bin::BinRef>) -> CmdResult<()> {
    blocking(move || crate::bin::restore(&items)).await
}

/// Deletes items from the Recycle Bin for good. Only the bin view calls this, after asking.
#[tauri::command]
pub async fn bin_purge(items: Vec<crate::bin::BinRef>) -> CmdResult<()> {
    blocking(move || crate::bin::purge(&items)).await
}

// ---- Disk scanner ----

#[derive(Clone, Serialize)]
struct ScanProgress {
    root: String,
    files: u64,
    bytes: u64,
}

#[derive(Clone, Serialize)]
struct ScanDone {
    root: String,
    files: u64,
    bytes: u64,
    cancelled: bool,
    error: Option<String>,
}

/// Starts scanning `root` in the background; progress and the end are events for every
/// window. One scan runs at a time.
#[tauri::command]
pub fn scan_start(app: tauri::AppHandle, root: String) -> CmdResult<()> {
    use crate::scan::Outcome;
    use tauri::Emitter;
    let flag = app
        .state::<Scanner>()
        .begin()
        .ok_or("A scan is already running")?;
    let root_path = PathBuf::from(&root);
    std::thread::spawn(move || {
        let scanner = app.state::<Scanner>();
        let progress = |files, bytes| {
            let _ = app.emit(
                "scan-progress",
                ScanProgress {
                    root: root.clone(),
                    files,
                    bytes,
                },
            );
        };
        let result = scanner.scan(&root_path, &flag, &progress);
        let (files, bytes, cancelled, error) = match result {
            Ok(Outcome::Done { files, bytes }) => (files, bytes, false, None),
            Ok(Outcome::Cancelled) => (0, 0, true, None),
            Err(e) => (0, 0, false, Some(e)),
        };
        let _ = app.emit(
            "scan-done",
            ScanDone {
                root,
                files,
                bytes,
                cancelled,
                error,
            },
        );
    });
    Ok(())
}

#[tauri::command]
pub fn scan_cancel(scanner: tauri::State<'_, Scanner>) {
    scanner.cancel();
}

#[tauri::command(async)]
pub fn scan_children(scanner: tauri::State<'_, Scanner>, dir: String) -> CmdResult<Vec<Sized>> {
    scanner
        .children(&PathBuf::from(dir))
        .map_err(|e| e.to_string())
}

/// Whether a scan is running, for a window that opened while one was.
#[tauri::command]
pub fn scan_running(scanner: tauri::State<'_, Scanner>) -> bool {
    scanner.is_running()
}

/// The size of a scanned folder and everything in it: (bytes, files).
#[tauri::command(async)]
pub fn scan_size(scanner: tauri::State<'_, Scanner>, dir: String) -> Option<(u64, u64)> {
    scanner.size_of(&PathBuf::from(dir))
}

#[tauri::command(async)]
pub fn scan_large(
    scanner: tauri::State<'_, Scanner>,
    min: u64,
    limit: u32,
) -> CmdResult<Vec<BigFile>> {
    scanner.large(min, limit).map_err(|e| e.to_string())
}

/// Large files untouched for `older_than_days`, as a cleanup suggestion list: biggest
/// first, never from a protected location. Only suggestions; nothing is changed.
#[tauri::command(async)]
pub fn cleanup_large_old(
    app: tauri::AppHandle,
    scanner: tauri::State<'_, Scanner>,
    min_bytes: u64,
    older_than_days: u32,
    limit: u32,
) -> CmdResult<Vec<BigFile>> {
    let home = app.path().home_dir().ok();
    cleanup::large_old(
        &scanner,
        cleanup_skips(home),
        min_bytes,
        older_than_days,
        limit as usize,
        journal::unix_now(),
    )
    .map_err(|e| e.to_string())
}

/// What cleanup never touches: protected places, and files the Recycle Bin can't be
/// trusted to take (on a drive without one, or bigger than it holds), which a delete
/// would destroy for good.
fn cleanup_skips(home: Option<PathBuf>) -> impl Fn(&std::path::Path, u64) -> bool {
    move |path, size| {
        cleanup::is_protected(path, home.as_deref())
            || crate::platform::recycle_limit(path).is_none_or(|limit| size > limit)
    }
}

/// What `cleanup_trash` did with the files it was given.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupTrash {
    /// The background job that moves them, if any were still eligible.
    job: Option<u64>,
    /// Files left alone because they changed, moved, or are no longer eligible.
    left_alone: usize,
}

/// Moves files from a cleanup list to the Recycle Bin, but only those still exactly as
/// listed (same size, not changed since) and still allowed: the list may be old. Anything
/// else is left alone.
#[tauri::command(async)]
pub fn cleanup_trash(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    queue: tauri::State<'_, Arc<Queue>>,
    items: Vec<BigFile>,
) -> CleanupTrash {
    let home = app.path().home_dir().ok();
    let ok = cleanup::still_as_listed(&items, cleanup_skips(home));
    // And together they must fit the bin: an overflowing one deletes its oldest items for good.
    let ok = cleanup::within_bin(ok, crate::platform::recycle_limit);
    let left_alone = items.len() - ok.len();
    let targets: Vec<String> = ok.iter().map(|f| f.path.clone()).collect();
    let job = (!targets.is_empty()).then(|| trash_paths(app.clone(), window, queue, targets));
    CleanupTrash { job, left_alone }
}

/// A drive with its capacity and free space.
#[derive(Serialize)]
pub struct Drive {
    path: String,
    total: u64,
    free: u64,
}

#[tauri::command(async)]
pub fn list_drives() -> Vec<Drive> {
    crate::platform::list_drives()
        .into_iter()
        .map(|d| Drive {
            path: d.path,
            total: d.total,
            free: d.free,
        })
        .collect()
}

/// Bytes per category in a scanned folder, in the order of [`crate::scan::CATEGORIES`].
#[tauri::command(async)]
pub fn scan_categories(scanner: tauri::State<'_, Scanner>, dir: String) -> CmdResult<Vec<u64>> {
    scanner
        .categories(&PathBuf::from(dir))
        .map_err(|e| e.to_string())
}

#[tauri::command(async)]
pub fn scan_roots(scanner: tauri::State<'_, Scanner>) -> CmdResult<Vec<RootInfo>> {
    scanner.roots().map_err(|e| e.to_string())
}

// ---- Crash reporting (local, opt-in) ----

/// Where crash reports and session counts are kept.
pub struct CrashDir(pub PathBuf);

#[tauri::command]
pub fn crash_state(dir: tauri::State<'_, CrashDir>) -> crate::crash::State {
    crate::crash::state(&dir.0)
}

#[tauri::command]
pub fn crash_set(dir: tauri::State<'_, CrashDir>, on: bool) -> crate::crash::State {
    crate::crash::set_consent(&dir.0, on);
    crate::crash::state(&dir.0)
}

#[tauri::command]
pub fn crash_clear(dir: tauri::State<'_, CrashDir>) -> crate::crash::State {
    crate::crash::clear(&dir.0);
    crate::crash::state(&dir.0)
}

// ---- Clipboard shelf ----

#[tauri::command(async)]
pub fn shelf_list(shelf: tauri::State<'_, Shelf>) -> Vec<ShelfItem> {
    shelf.list()
}

/// Puts `paths` on the shelf for copying or cutting; the files are not touched.
#[tauri::command]
pub fn shelf_add(
    shelf: tauri::State<'_, Shelf>,
    paths: Vec<String>,
    mode: String,
) -> CmdResult<()> {
    shelf.add(&self::paths(paths), &mode)
}

#[tauri::command]
pub fn shelf_set_mode(shelf: tauri::State<'_, Shelf>, path: String, mode: String) -> CmdResult<()> {
    shelf.set_mode(&PathBuf::from(path), &mode)
}

/// Takes items off the shelf; the files are not touched.
#[tauri::command]
pub fn shelf_remove(shelf: tauri::State<'_, Shelf>, paths: Vec<String>) -> CmdResult<()> {
    shelf.remove(&self::paths(paths))
}

#[tauri::command]
pub fn shelf_clear(shelf: tauri::State<'_, Shelf>) -> CmdResult<()> {
    shelf.clear()
}

#[tauri::command]
pub fn perf_config() -> crate::perf::PerfConfig {
    crate::perf::config()
}

#[tauri::command]
pub fn perf_report(event: String) {
    crate::perf::report(&event);
}
