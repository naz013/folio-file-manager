//! Thin Tauri command layer. Validation and error mapping only; logic lives in modules.
//!
//! File operations run on the blocking thread pool so the UI never freezes.
#![expect(
    clippy::needless_pass_by_value,
    reason = "Tauri deserializes command arguments and injects state by value"
)]

use crate::cleanup;
use crate::cleanup_temp;
use crate::fs::{self, Entry};
use crate::journal::{self, Journal, Step};
use crate::ops;
use crate::queue::{self, Kind, Queue};
use crate::scan::{self, BigFile, Measured, RootInfo, Scanner, Sized};
use crate::search::{self, Hit, Query};
use crate::session::{Session, WindowTabs};
use crate::shelf::{Shelf, ShelfItem};
use crate::slow;
use crate::smart::{self, Criteria, SmartFolder};
use crate::tags::{Tag, Tags};
use crate::watch::DirWatcher;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
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

/// Lists a folder off the main thread and gives up after a while: a network share that does
/// not answer must not freeze the window, and the person can go somewhere else meanwhile.
#[tauri::command]
pub async fn list_dir(path: String) -> CmdResult<Vec<Entry>> {
    let dir = PathBuf::from(&path);
    match slow::LISTINGS
        .within(slow::LISTING, move || fs::list_dir(&dir))
        .await
    {
        Some(listed) => listed.map_err(|e| format!("Cannot open {path}: {e}")),
        None => Err(format!(
            "{path} is not answering. If it is a network folder, check the connection."
        )),
    }
}

#[tauri::command]
pub fn home_dir(app: tauri::AppHandle) -> CmdResult<String> {
    app.path()
        .home_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .map_err(|e| e.to_string())
}

/// Starts watching a folder. Starting a watch on a network folder waits for the server, so it
/// runs on the blocking pool: a command that only calls itself `async` would hold one of the
/// runtime's few worker threads, and with them the timers that end the other waits.
#[tauri::command]
pub async fn watch_dir(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    path: String,
) -> CmdResult<()> {
    let label = window.label().to_owned();
    blocking(move || {
        app.state::<DirWatcher>()
            .watch(app.clone(), &label, &PathBuf::from(path))
            .map_err(|e| e.to_string())
    })
    .await
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
    collapsed: Vec<String>,
) -> CmdResult<()> {
    let label = window.label().to_owned();
    blocking(move || {
        app.state::<Session>().update(
            &label,
            WindowTabs {
                tabs,
                active,
                views,
                collapsed,
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
    tabs: WindowTabs,
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
    session.open(&label, WindowTabs { active: 0, ..tabs });
    crate::window::open(&app, &label, &place).map_err(|e| {
        session.close(&label);
        e.to_string()
    })
}

// On the blocking pool: opening a file on a slow share, or one the shell must fetch, waits.
#[tauri::command]
pub async fn open_path(path: String) -> CmdResult<()> {
    blocking(move || tauri_plugin_opener::open_path(&path, None::<&str>).map_err(|e| e.to_string()))
        .await
}

/// Starts a native OS drag of `paths`, so files can be dropped into Explorer and other apps.
/// Drops back onto Folio arrive through the webview's regular drag-drop events.
#[tauri::command]
pub async fn start_drag(window: tauri::WebviewWindow, paths: Vec<String>) -> CmdResult<()> {
    #[cfg(not(target_os = "linux"))]
    {
        let files: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
        let first = files.first().cloned().ok_or("Nothing to drag")?;
        // A cloud-only file drags with a blank image; so does a folder, whose picture the shell
        // may build from the files inside it.
        let preview = slow::THUMBNAILS
            .within(slow::THUMBNAIL, move || {
                let draw = !first.is_dir();
                draw.then(|| crate::platform::thumbnail_png(&first, 64))
                    .and_then(Result::ok)
                    .map_or_else(|| crate::platform::encode_png(1, 1, &[0, 0, 0, 0]), Ok)
            })
            .await
            .ok_or("The file is not answering")??;
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
    submit_trash(&app, &window, &queue, paths(targets), None)
}

/// Picks, when the job runs, which of the targets still go to the Recycle Bin: the ones to
/// move, and how many were held back for lack of room in the bin.
type TrashFilter = Box<dyn FnOnce(Vec<PathBuf>) -> Picked + Send>;

/// A test each path must pass just before it moves.
type LastLook = Box<dyn Fn(&std::path::Path) -> bool + Send>;

/// What a [`TrashFilter`] chose: the paths to move, how many were held back for room, and
/// optionally a last test to pass just before each one moves.
struct Picked {
    paths: Vec<PathBuf>,
    no_room: usize,
    last_look: Option<LastLook>,
}

fn submit_trash(
    app: &tauri::AppHandle,
    window: &tauri::WebviewWindow,
    queue: &Arc<Queue>,
    targets: Vec<PathBuf>,
    filter: Option<TrashFilter>,
) -> u64 {
    let label = describe("Move to Recycle Bin:", &targets);
    // Cleanup moves wait in one line, whatever the drive: each checks its files when it
    // starts, so it sees what the one before it did (a copy that stays is not removed by
    // another job meanwhile).
    let lane = if filter.is_some() {
        "cleanup".to_owned()
    } else {
        targets
            .first()
            .map_or_else(String::new, |p| ops::volume_key(p.parent().unwrap_or(p)))
    };
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
        let mut left_alone = changed.len();
        // The room left in the bin is looked at now, not when the job was queued: other
        // jobs ahead of this one may have filled it.
        let (targets, no_room, last_look) = match filter {
            Some(pick) => {
                let before = targets.len();
                let picked = pick(targets);
                left_alone += before - picked.paths.len() - picked.no_room;
                (picked.paths, picked.no_room, picked.last_look)
            }
            None => (targets, 0, None),
        };
        // Checking the files can take long; where the move depends on something staying
        // as it was, each one is looked at again right before it goes.
        let (result, steps) = match last_look {
            None => trash_with_steps(&journal, &targets),
            Some(ok_to_move) => {
                let (mut result, mut steps) = (Ok(()), Vec::new());
                for path in &targets {
                    if !ok_to_move(path) {
                        left_alone += 1;
                        continue;
                    }
                    let (one, mut done) = trash_with_steps(&journal, std::slice::from_ref(path));
                    steps.append(&mut done);
                    if one.is_err() {
                        result = one;
                        break;
                    }
                }
                (result, steps)
            }
        };
        journal.finish(pending, &steps);
        for _ in &steps {
            ops::progress::add_item();
        }
        // On Windows an item that left its place with no Recycle Bin entry to restore may
        // have been deleted for good (a bin set to delete at once, or one that overflowed);
        // it is not counted as moved, and an error says so too.
        let unconfirmed = if cfg!(windows) {
            steps
                .iter()
                .filter(|s| matches!(s, Step::Trash { id: None, .. }))
                .count()
        } else {
            0
        };
        match result {
            Ok(()) => Ok(serde_json::json!({
                "count": steps.len() - unconfirmed,
                "leftAlone": left_alone,
                "noRoom": no_room,
                "unconfirmed": unconfirmed,
            })),
            Err(e) if unconfirmed > 0 => Err(format!(
                "{e}. {unconfirmed} item(s) left without a Recycle Bin entry and may have been deleted permanently."
            )),
            Err(e) => Err(e),
        }
    });
    queue.submit(app, window.label(), Kind::Trash, title, &lane, work)
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

#[tauri::command]
pub fn smart_list(tags: tauri::State<'_, Tags>) -> Vec<SmartFolder> {
    tags.smart_list()
}

/// Adds a smart folder, or changes the one with `id`.
#[tauri::command]
pub fn smart_save(
    tags: tauri::State<'_, Tags>,
    id: Option<i64>,
    name: String,
    criteria: Criteria,
) -> CmdResult<SmartFolder> {
    tags.smart_save(id, &name, &criteria)
}

#[tauri::command]
pub fn smart_delete(tags: tauri::State<'_, Tags>, id: i64) -> CmdResult<()> {
    tags.smart_delete(id)
}

/// The folder walk of each window's smart folder under way, so a newer one can stop it.
#[derive(Default)]
pub struct SmartWalks(std::sync::Mutex<std::collections::HashMap<String, Arc<AtomicBool>>>);

impl SmartWalks {
    /// The stop flag for a new walk for `window`; the walk before it, if any, is stopped.
    fn begin(&self, window: &str) -> Arc<AtomicBool> {
        let mut walks = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let fresh = Arc::new(AtomicBool::new(false));
        if let Some(old) = walks.insert(window.to_owned(), Arc::clone(&fresh)) {
            old.store(true, Ordering::Relaxed);
        }
        fresh
    }
}

/// What a smart folder matches right now: (path, entry, the item's tag ids). The answer is
/// worked out afresh, so it is always current. A folder tree is walked on its own thread and
/// stopped if it takes too long; a tag lookup is quick.
#[tauri::command]
pub async fn smart_open(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    walks: tauri::State<'_, SmartWalks>,
    id: i64,
) -> CmdResult<Vec<(String, Entry, Vec<i64>)>> {
    let now = u64::try_from(journal::unix_now()).unwrap_or(0) * 1000;
    let folder = {
        let app = app.clone();
        blocking(move || app.state::<Tags>().smart_get(id)).await?
    };
    let found = if folder.criteria.tags.is_empty() {
        // A new look from this window stops the one before it, which no one waits for now.
        let stop = walks.begin(window.label());
        let flag = Arc::clone(&stop);
        let criteria = folder.criteria;
        let walked = slow::WALKS
            .within(slow::SMART, move || smart::walk(&criteria, now, &flag))
            .await;
        let Some(result) = walked else {
            stop.store(true, Ordering::Relaxed);
            return Err("Looking took too long. Try a smaller folder.".into());
        };
        result?
    } else {
        blocking(move || app.state::<Tags>().smart_tagged(&folder.criteria, now)).await?
    };
    Ok(found
        .into_iter()
        .map(|(path, entry, ids)| (path.to_string_lossy().into_owned(), entry, ids))
        .collect())
}

/// What a finished tags export tells the window.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TagsExported {
    path: String,
    tags: usize,
    files: usize,
    /// Tagged items left out because they are no longer where they were tagged.
    left_out: usize,
}

/// A tags file bigger than this is not one: a person's tags are a few megabytes at most.
const MAX_TAGS_FILE: u64 = 64 << 20;

/// Writes every tag and every tagged item to a new "Folio tags.json" in `dir` (the Documents
/// folder if none is given), under the next free name if there is one already.
#[tauri::command(async)]
pub fn tags_export(
    app: tauri::AppHandle,
    tags: tauri::State<'_, Tags>,
    dir: Option<String>,
) -> CmdResult<TagsExported> {
    let dir = match dir {
        Some(dir) => PathBuf::from(dir),
        None => app.path().document_dir().map_err(|e| e.to_string())?,
    };
    let (backup, left_out) = tags.backup_counted(app.path().home_dir().ok().as_deref())?;
    let bytes = serde_json::to_vec_pretty(&backup).map_err(|e| e.to_string())?;
    let path = ops::made::write_new(&dir, "Folio tags.json", &bytes).map_err(|e| e.to_string())?;
    Ok(TagsExported {
        path: path.to_string_lossy().into_owned(),
        tags: backup.tags.len(),
        files: backup.files.len(),
        left_out,
    })
}

/// Adds what a tags file holds to the tags here. Nothing is taken away.
#[tauri::command(async)]
pub fn tags_import(
    app: tauri::AppHandle,
    tags: tauri::State<'_, Tags>,
    path: String,
) -> CmdResult<crate::tags::Restored> {
    let path = PathBuf::from(path);
    let len = std::fs::metadata(&path).map_err(|e| e.to_string())?.len();
    if len > MAX_TAGS_FILE {
        return Err("This is not a Folio tags file".into());
    }
    let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
    let backup = crate::tags::Backup::parse(&bytes)?;
    tags.restore(&backup, app.path().home_dir().ok().as_deref())
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

/// The folder measurements under way, by the id the window gave each, so a newer selection
/// can stop them.
#[derive(Default)]
pub struct Measuring(std::sync::Mutex<std::collections::HashMap<u64, Arc<AtomicBool>>>);

impl Measuring {
    fn jobs(&self) -> std::sync::MutexGuard<'_, std::collections::HashMap<u64, Arc<AtomicBool>>> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// Counts a folder fresh from the disk, in the background: (bytes, files, folders), or
/// nothing if `measure_cancel` stopped it first.
#[tauri::command(async)]
pub fn measure_dir(jobs: tauri::State<'_, Measuring>, id: u64, dir: String) -> Option<Measured> {
    let cancel = Arc::new(AtomicBool::new(false));
    jobs.jobs().insert(id, Arc::clone(&cancel));
    let out = scan::measure(&PathBuf::from(dir), &cancel);
    jobs.jobs().remove(&id);
    out
}

#[tauri::command]
pub fn measure_cancel(jobs: tauri::State<'_, Measuring>, id: u64) {
    if let Some(cancel) = jobs.jobs().get(&id) {
        cancel.store(true, Ordering::Relaxed);
    }
}

/// What a search tells the window that started it.
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SearchEvent {
    /// More matches, in the order they were found.
    Hits { hits: Vec<Hit> },
    /// The search is over; no event follows.
    Done(search::Outcome),
}

/// The searches under way, by the id the window gave each, so it can stop them.
#[derive(Default)]
pub struct Searching(std::sync::Mutex<std::collections::HashMap<(String, u64), Arc<AtomicBool>>>);

impl Searching {
    /// The cancel flag of search `id` of the window called `window`. A cancel that comes
    /// before the search has started leaves the flag set, so the search ends at once.
    fn flag(&self, window: &str, id: u64) -> Arc<AtomicBool> {
        let mut jobs = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Arc::clone(jobs.entry((window.to_owned(), id)).or_default())
    }

    fn forget(&self, window: &str, id: u64) {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&(window.to_owned(), id));
    }
}

/// Searches a folder and everything below it by name and size. Matches stream to
/// `events` while the walk goes on; the last event is `Done`. `search_cancel` stops it.
#[tauri::command(async)]
pub fn search_dir(
    window: tauri::WebviewWindow,
    jobs: tauri::State<'_, Searching>,
    id: u64,
    dir: String,
    query: Query,
    events: tauri::ipc::Channel<SearchEvent>,
) {
    // Ids are counted per window, so the window's name is part of the key.
    let cancel = jobs.flag(window.label(), id);
    let outcome = search::search(&PathBuf::from(dir), &query, &cancel, &mut |hits| {
        events.send(SearchEvent::Hits { hits }).is_ok()
    });
    jobs.forget(window.label(), id);
    let _ = events.send(SearchEvent::Done(outcome));
}

#[tauri::command]
pub fn search_cancel(window: tauri::WebviewWindow, jobs: tauri::State<'_, Searching>, id: u64) {
    jobs.flag(window.label(), id).store(true, Ordering::Relaxed);
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

/// Installers and archives in the Downloads folder untouched for `older_than_days`, as a
/// cleanup suggestion list. Only suggestions; nothing is changed.
#[tauri::command(async)]
pub fn cleanup_old_downloads(
    app: tauri::AppHandle,
    older_than_days: u32,
    limit: u32,
) -> CmdResult<Vec<BigFile>> {
    let home = app.path().home_dir().ok();
    let dir = app.path().download_dir().map_err(|e| e.to_string())?;
    if !cleanup::downloads_folder_ok(&dir, home.as_deref()) {
        return Err(format!(
            "Your Downloads folder ({}) is a whole drive or your profile, so everything in it \
             would look like a download. Cleanup leaves it alone.",
            dir.display()
        ));
    }
    Ok(cleanup::old_downloads(
        &dir,
        cleanup_skips(home),
        cleanup::arrived,
        older_than_days,
        limit as usize,
        journal::unix_now(),
    ))
}

/// Copies of the same file among the scanned files of at least `min_bytes`, newest first
/// in each group. Only suggestions; nothing is changed.
#[tauri::command(async)]
pub fn cleanup_duplicates(
    app: tauri::AppHandle,
    scanner: tauri::State<'_, Scanner>,
    min_bytes: u64,
) -> CmdResult<cleanup::Duplicates> {
    let home = app.path().home_dir().ok();
    cleanup::duplicates(&scanner, cleanup_skips(home), min_bytes.max(1 << 20))
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

/// What cleanup never touches, for a file that stays where it is: it needs no Recycle Bin.
fn protected_only(home: Option<PathBuf>) -> impl Fn(&std::path::Path, u64) -> bool {
    move |path, _| cleanup::is_protected(path, home.as_deref())
}

/// Stops the duplicate search in progress (the view closed, or the setting changed).
#[tauri::command]
pub fn cleanup_duplicates_cancel() {
    cleanup::cancel_duplicates();
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
/// else is left alone, as is a file that is one of the `keep` paths (the copies groups of
/// duplicates keep) or the same file as another item. Which of them fit in the drive's bin
/// is decided when the job runs.
#[tauri::command(async)]
pub fn cleanup_trash(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    queue: tauri::State<'_, Arc<Queue>>,
    items: Vec<BigFile>,
    keep: Vec<String>,
) -> CleanupTrash {
    let home = app.path().home_dir().ok();
    // A copy or move builds its result under a temporary name, and what it copies from may
    // be what is listed here; nothing moves while one is going.
    if queue.copy_running() {
        return CleanupTrash {
            job: None,
            left_alone: items.len(),
        };
    }
    let still = cleanup::still_as_listed(&items, cleanup_skips(home.clone()));
    let ok: Vec<BigFile> = cleanup::without_aliases(still, &keep)
        .into_iter()
        .map(|f| BigFile {
            path: f.path.clone(),
            size: f.size,
            modified: f.modified,
        })
        .collect();
    let left_alone = items.len() - ok.len();
    if ok.is_empty() {
        return CleanupTrash {
            job: None,
            left_alone,
        };
    }
    let targets: Vec<PathBuf> = ok.iter().map(|f| PathBuf::from(&f.path)).collect();
    let pick: TrashFilter = Box::new(move |targets| {
        // Checked again now, then cut to what the bins can hold together.
        let wanted: Vec<BigFile> = ok
            .into_iter()
            .filter(|f| targets.iter().any(|t| t.to_string_lossy() == f.path))
            .collect();
        let still = cleanup::without_aliases(
            cleanup::still_as_listed(&wanted, cleanup_skips(home)),
            &keep,
        );
        let fits = cleanup::within_bin(still.clone(), crate::platform::recycle_limit);
        let no_room = still.len() - fits.len();
        Picked {
            paths: fits.iter().map(|f| PathBuf::from(&f.path)).collect(),
            no_room,
            last_look: None,
        }
    });
    let job = submit_trash(&app, &window, &queue, targets, Some(pick));
    CleanupTrash {
        job: Some(job),
        left_alone,
    }
}

/// Moves the copies marked in reviewed duplicate groups to the Recycle Bin. Each group is
/// checked as a whole, again when the job runs: a copy that stays must still be there as
/// listed, or nothing in the group moves. So the last copy is never moved, however many
/// are asked for.
#[tauri::command(async)]
pub fn cleanup_trash_duplicates(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    queue: tauri::State<'_, Arc<Queue>>,
    groups: Vec<cleanup::Reviewed>,
) -> CleanupTrash {
    let home = app.path().home_dir().ok();
    let asked: usize = groups.iter().map(|g| g.remove.len()).sum();
    let (ok, _) = cleanup::removable_copies(
        &groups,
        &cleanup_skips(home.clone()),
        &protected_only(home.clone()),
        false,
    );
    let left_alone = asked - ok.len();
    if ok.is_empty() {
        return CleanupTrash {
            job: None,
            left_alone,
        };
    }
    let targets: Vec<PathBuf> = ok.iter().map(|r| PathBuf::from(&r.file.path)).collect();
    let pick: TrashFilter = Box::new(move |targets| {
        // Looked at again now, group by group, then cut to what the bins can hold.
        let (again, _) = cleanup::removable_copies(
            &groups,
            &cleanup_skips(home.clone()),
            &protected_only(home),
            true,
        );
        let wanted: Vec<&cleanup::Removal> = again
            .iter()
            .filter(|r| targets.iter().any(|t| t.to_string_lossy() == r.file.path))
            .collect();
        let fits = cleanup::within_bin(
            wanted.iter().map(|r| &r.file).collect(),
            crate::platform::recycle_limit,
        );
        let no_room = wanted.len() - fits.len();
        let paths: Vec<PathBuf> = fits.iter().map(|f| PathBuf::from(&f.path)).collect();
        // The comparing above can take minutes; just before each copy goes, the copy that
        // stays for it must still be the same file.
        let guards: std::collections::HashMap<String, cleanup::Removal> = again
            .into_iter()
            .filter(|r| paths.iter().any(|p| p.to_string_lossy() == r.file.path))
            .map(|r| (r.file.path.clone(), r))
            .collect();
        Picked {
            paths,
            no_room,
            last_look: Some(Box::new(move |path| {
                guards
                    .get(path.to_string_lossy().as_ref())
                    .is_some_and(cleanup::keepers_still_there)
            })),
        }
    });
    let job = submit_trash(&app, &window, &queue, targets, Some(pick));
    CleanupTrash {
        job: Some(job),
        left_alone,
    }
}

/// Unfinished copies (`*.folio-partial`) among the scanned folders, and files of 1 MB or more,
/// that have been left alone for `older_than_days`, with their sizes. Only suggestions. Nothing is looked at
/// while a copy or move is running, since the one that is running is building such a
/// folder.
#[tauri::command(async)]
pub fn cleanup_partials(
    app: tauri::AppHandle,
    scanner: tauri::State<'_, Scanner>,
    queue: tauri::State<'_, Arc<Queue>>,
    older_than_days: u32,
    limit: u32,
) -> CmdResult<Vec<cleanup::PartialCopy>> {
    if queue.copy_running() {
        return Err(
            "A copy or move is running. Unfinished copies are not looked at until it is done, \
             since it is building one."
                .into(),
        );
    }
    let home = app.path().home_dir().ok();
    cleanup::stale_partials(
        &scanner,
        cleanup_skips(home),
        &cleanup::touched,
        older_than_days,
        limit as usize,
        journal::unix_now(),
    )
    .map_err(|e| e.to_string())
}

/// Moves unfinished copies from a cleanup list to the Recycle Bin, those still exactly as
/// listed and still left alone, and only while no copy or move is running. Anything else is
/// left alone. Which of them fit in the drive's bin is decided when the job runs.
#[tauri::command(async)]
pub fn cleanup_trash_partials(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    queue: tauri::State<'_, Arc<Queue>>,
    items: Vec<cleanup::PartialCopy>,
) -> CleanupTrash {
    let home = app.path().home_dir().ok();
    let now = journal::unix_now();
    let running = queue.copy_running();
    let ok: Vec<cleanup::PartialCopy> = if running {
        Vec::new()
    } else {
        cleanup::still_partials(&items, &cleanup_skips(home.clone()), &cleanup::touched, now)
            .into_iter()
            .cloned()
            .collect()
    };
    let left_alone = items.len() - ok.len();
    if ok.is_empty() {
        return CleanupTrash {
            job: None,
            left_alone,
        };
    }
    let targets: Vec<PathBuf> = ok.iter().map(|p| PathBuf::from(&p.path)).collect();
    let watch = Arc::clone(&queue);
    let pick: TrashFilter = Box::new(move |targets| {
        // Looked at again now, when the job runs: a copy may have started meanwhile, and the
        // room in the bin is whatever is left.
        let wanted: Vec<BigFile> = if watch.copy_running() {
            Vec::new()
        } else {
            let listed: Vec<cleanup::PartialCopy> = ok
                .into_iter()
                .filter(|p| targets.iter().any(|t| t.to_string_lossy() == p.path))
                .collect();
            cleanup::still_partials(
                &listed,
                &cleanup_skips(home),
                &cleanup::touched,
                journal::unix_now(),
            )
            .into_iter()
            .map(BigFile::from)
            .collect()
        };
        let fits = cleanup::within_bin(wanted.iter().collect(), crate::platform::recycle_limit);
        let no_room = wanted.len() - fits.len();
        Picked {
            paths: fits.iter().map(|f| PathBuf::from(&f.path)).collect(),
            no_room,
            last_look: None,
        }
    });
    let job = submit_trash(&app, &window, &queue, targets, Some(pick));
    CleanupTrash {
        job: Some(job),
        left_alone,
    }
}

/// The folders the list of temp places hangs from: the home folder and `Local AppData` as the
/// system names them, never from environment variables a program may have changed.
fn temp_env(app: &tauri::AppHandle) -> CmdResult<cleanup_temp::Env> {
    let home = app.path().home_dir().map_err(|e| e.to_string())?;
    Ok(cleanup_temp::Env::current(
        home,
        app.path().local_data_dir().ok(),
    ))
}

/// Files in the short list of places that programs keep throw-away files in (temporary
/// folders, browser and program caches) that have been left alone for that place's minimum
/// time, with the places looked in. Only suggestions.
#[tauri::command(async)]
pub fn cleanup_temp(app: tauri::AppHandle, limit: u32) -> CmdResult<cleanup_temp::Listing> {
    Ok(cleanup_temp::temp_files(
        &temp_env(&app)?,
        journal::unix_now(),
        limit as usize,
    ))
}

/// Moves files from a temp-files list to the Recycle Bin: only those that are, right now,
/// still in a place on the list, still exactly as listed and still left alone for long
/// enough. Anything else is left alone, whatever the list says. Which of them fit in the
/// drive's bin is decided when the job runs.
#[tauri::command(async)]
pub fn cleanup_trash_temp(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    queue: tauri::State<'_, Arc<Queue>>,
    items: Vec<cleanup_temp::TempFile>,
) -> CmdResult<CleanupTrash> {
    let env = temp_env(&app)?;
    // What comes back has its links resolved, and that is what is moved.
    let ok = cleanup_temp::still_listed(&items, &env, journal::unix_now());
    let left_alone = items.len() - ok.len();
    if ok.is_empty() {
        return Ok(CleanupTrash {
            job: None,
            left_alone,
        });
    }
    let targets: Vec<PathBuf> = ok.iter().map(|f| PathBuf::from(&f.path)).collect();
    let pick: TrashFilter = Box::new(move |targets| {
        // Looked at again now, when the job runs: a program may have used a file meanwhile,
        // and the room in the bin is whatever is left.
        let listed: Vec<cleanup_temp::TempFile> = ok
            .into_iter()
            .filter(|f| targets.iter().any(|t| t.to_string_lossy() == f.path))
            .collect();
        let wanted: Vec<BigFile> = cleanup_temp::still_listed(&listed, &env, journal::unix_now())
            .iter()
            .map(BigFile::from)
            .collect();
        let fits = cleanup::within_bin(wanted.iter().collect(), crate::platform::recycle_limit);
        let no_room = wanted.len() - fits.len();
        Picked {
            paths: fits.iter().map(|f| PathBuf::from(&f.path)).collect(),
            no_room,
            last_look: None,
        }
    });
    let job = submit_trash(&app, &window, &queue, targets, Some(pick));
    Ok(CleanupTrash {
        job: Some(job),
        left_alone,
    })
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

/// The result of a finished tool job (PDF or picture), for the UI.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MadeFile {
    path: String,
    /// Bytes of the inputs together, and of the new file.
    before: u64,
    after: u64,
    /// A single file was made smaller (not a merge, a turn or a crop).
    compressed: bool,
    /// Something to tell the user about, if the job did not go as it should.
    #[serde(skip_serializing_if = "Option::is_none")]
    note: Option<String>,
    /// What to say when it went well, in place of the usual "Saved <name>".
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

/// Runs `make` in the background and writes what it returns as `dir/name` (or the next free
/// name). One journal entry covers it, so undo moves the new file to the Recycle Bin.
fn submit_pdf(
    app: &tauri::AppHandle,
    window: &tauri::WebviewWindow,
    queue: &Arc<Queue>,
    label: String,
    sources: Vec<PathBuf>,
    name: String,
    make: impl FnOnce() -> std::io::Result<Vec<u8>> + Send + 'static,
) -> CmdResult<u64> {
    ops::validate_name(&name).map_err(|e| e.to_string())?;
    if !name.to_lowercase().ends_with(".pdf") || cleanup::is_partial_name(&name) {
        return Err(format!("\"{name}\" must end in .pdf"));
    }
    if ops::total_bytes(&sources) > ops::pdf::MAX_INPUT_BYTES {
        return Err("These PDFs are too big to work on (over 1 GB together)".into());
    }
    let dir = sources
        .first()
        .and_then(|p| p.parent())
        .ok_or("No folder to save in")?
        .to_path_buf();
    let lane = ops::volume_key(&dir);
    let title = label.clone();
    let handle = app.clone();
    let work: queue::Work = Box::new(move |progress| {
        let journal = handle.state::<Journal>();
        progress.set_totals(ops::total_bytes(&sources), sources.len() as u64);
        // Building only reads, so nothing is in the journal until there is a file to write:
        // a failed, cancelled or crashed build leaves no entry that could block undo.
        let bytes = make().map_err(|e| e.to_string())?;
        ops::progress::checkpoint().map_err(|e| e.to_string())?;
        let pending = begin(&journal, &label)?;
        let result = ops::made::write_new(&dir, &name, &bytes);
        let steps: Vec<Step> = result
            .as_ref()
            .map(|p| Step::made(p.clone()))
            .into_iter()
            .collect();
        journal.finish(pending, &steps);
        let output = result.map_err(|e| e.to_string())?;
        Ok(serde_json::to_value(MadeFile {
            before: sources
                .iter()
                .filter_map(|p| std::fs::metadata(p).ok())
                .map(|m| m.len())
                .sum(),
            after: std::fs::metadata(&output).map_or(0, |m| m.len()),
            // Merging needs two files, so one file is a compression.
            compressed: sources.len() == 1,
            note: None,
            message: None,
            path: output.to_string_lossy().into_owned(),
        })
        .unwrap_or_default())
    });
    Ok(queue.submit(app, window.label(), Kind::Edit, title, &lane, work))
}

/// Where an edited picture goes.
#[derive(Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SaveAs {
    /// Next to the original, under a new name.
    Copy,
    /// In place of the original, which goes to the Recycle Bin first.
    Replace,
}

/// What the file looked like when the edit was asked for: size and last change.
fn stamp(path: &std::path::Path) -> Option<(u64, std::time::SystemTime)> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    Some((meta.len(), meta.modified().ok()?))
}

/// The picture an edit replaces, as it was when the edit was asked for.
struct Original<'a> {
    path: &'a std::path::Path,
    dir: &'a std::path::Path,
    name: &'a str,
    stamp: (u64, std::time::SystemTime),
    identity: Option<String>,
}

/// What [`replace_original`] did.
struct Replaced {
    made: std::io::Result<PathBuf>,
    steps: Vec<Step>,
    /// The original has a Recycle Bin entry that Undo can restore.
    restorable: bool,
}

/// Puts `bytes` in the place of the original, which goes to the Recycle Bin. The edit is
/// complete on disk first, and once the original has left its place the edit is never
/// removed: it may be all there is. Fails, leaving the original alone, if it changed since
/// the edit was made.
fn replace_original(
    journal: &Journal,
    label: &str,
    original: &Original,
    bytes: &[u8],
) -> Result<Replaced, String> {
    let staged = ops::made::stage(original.dir, original.name, bytes).map_err(|e| e.to_string())?;
    let pending = match begin(journal, label) {
        Ok(pending) => pending,
        Err(e) => {
            let _ = std::fs::remove_file(&staged);
            return Err(e);
        }
    };
    // The original may have been edited or swapped meanwhile; the edit was made from an
    // older one. Looked at again as late as possible.
    if stamp(original.path) != Some(original.stamp)
        || ops::identity(original.path) != original.identity
    {
        let _ = std::fs::remove_file(&staged);
        journal.finish(pending, &[]);
        return Err(format!(
            "\"{}\" changed while it was being edited",
            original.name
        ));
    }
    let started = journal::unix_now() - 2;
    let trashed = ops::trash(std::slice::from_ref(&original.path.to_path_buf()));
    if std::fs::symlink_metadata(original.path).is_ok() {
        // Still where it was: there is nothing to put the edit in place of.
        let _ = std::fs::remove_file(&staged);
        journal.finish(pending, &[]);
        return Err(trashed.err().map_or_else(
            || "The original could not be moved".to_owned(),
            |e| e.to_string(),
        ));
    }
    let made = ops::made::finish(&staged, original.dir, original.name);
    // The bin is looked at last: listing it is slow, and the path is empty until the edit
    // is in it.
    let id = journal::trash_ids(
        std::slice::from_ref(&original.path.to_path_buf()),
        started,
        &journal.trash_ids_in_use(),
    )
    .into_iter()
    .next()
    .flatten();
    let restorable = id.is_some();
    let mut steps = vec![Step::Trash {
        original: original.path.to_path_buf(),
        id,
    }];
    if let Ok(path) = &made {
        steps.push(Step::made(path.clone()));
    }
    journal.finish(pending, &steps);
    Ok(Replaced {
        made,
        steps,
        restorable,
    })
}

/// Edits the picture at `path` in the background. As a copy it is written next to the
/// original; replacing puts the original in the Recycle Bin and the edit in its place, and
/// undo brings the original back. Returns the job id.
#[tauri::command(async)]
pub fn image_edit(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    queue: tauri::State<'_, Arc<Queue>>,
    path: String,
    edit: ops::image_edit::Edit,
    save: SaveAs,
) -> CmdResult<u64> {
    use ops::image_edit::Edit;
    let source = PathBuf::from(path);
    // The trash can't be asked for the original again on macOS, so a replace could not
    // be undone there.
    if matches!(save, SaveAs::Replace) && cfg!(target_os = "macos") {
        return Err("Replacing isn't available on this system yet; save a copy instead".into());
    }
    if ops::image_edit::Format::of(&source).is_none() {
        return Err("This kind of picture can't be edited".into());
    }
    let before = stamp(&source).ok_or("The picture isn't there any more")?;
    let identity = ops::identity(&source);
    if std::fs::symlink_metadata(&source).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err("A link can't be edited; open the picture it points to".into());
    }
    let dir = source.parent().ok_or("No folder to save in")?.to_path_buf();
    let file_name = source
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .ok_or("No file name")?;
    let (verb, tag) = match edit {
        Edit::Rotate { .. } => ("Rotate", "rotated"),
        Edit::Compress { .. } => ("Compress", "compressed"),
        Edit::Crop { .. } => ("Crop", "cropped"),
    };
    let copy_name = match file_name.rfind('.') {
        Some(dot) if dot > 0 => format!("{} ({tag}){}", &file_name[..dot], &file_name[dot..]),
        _ => format!("{file_name} ({tag})"),
    };
    let label = describe(verb, std::slice::from_ref(&source));
    let lane = ops::volume_key(&dir);
    let title = label.clone();
    let handle = app.clone();
    let work: queue::Work = Box::new(move |progress| {
        let journal = handle.state::<Journal>();
        progress.set_totals(before.0, 1);
        // Building only reads, so nothing is in the journal until there is a file to write.
        let bytes = ops::image_edit::apply(&source, edit).map_err(|e| e.to_string())?;
        ops::progress::checkpoint().map_err(|e| e.to_string())?;
        let mut restorable = true;
        let (result, steps) = match save {
            SaveAs::Copy => {
                let pending = begin(&journal, &label)?;
                let result = ops::made::write_new(&dir, &copy_name, &bytes);
                let steps: Vec<Step> = result
                    .as_ref()
                    .map(|p| Step::made(p.clone()))
                    .into_iter()
                    .collect();
                journal.finish(pending, &steps);
                (result, steps)
            }
            SaveAs::Replace => {
                let original = Original {
                    path: &source,
                    dir: &dir,
                    name: &file_name,
                    stamp: before,
                    identity: identity.clone(),
                };
                let replaced = replace_original(&journal, &label, &original, &bytes)?;
                restorable = replaced.restorable;
                (replaced.made, replaced.steps)
            }
        };
        drop(steps);
        let made = result.map_err(|e| e.to_string())?;
        Ok(serde_json::to_value(MadeFile {
            before: before.0,
            after: std::fs::metadata(&made).map_or(0, |m| m.len()),
            path: made.to_string_lossy().into_owned(),
            compressed: matches!(edit, Edit::Compress { .. }),
            note: (!restorable).then(|| {
                "The original is in the Recycle Bin, but Folio could not find it there, so Undo \
                 can't bring it back. Restore it from the Recycle Bin."
                    .to_owned()
            }),
            message: None,
        })
        .unwrap_or_default())
    });
    Ok(queue.submit(&app, window.label(), Kind::Edit, title, &lane, work))
}

/// How big the picture at `path` would be after `preset`, in bytes, or why it can't be made
/// smaller that way.
#[tauri::command]
pub async fn image_estimate(path: String, preset: ops::image_edit::Preset) -> CmdResult<u64> {
    blocking(move || {
        ops::image_edit::apply(
            &PathBuf::from(path),
            ops::image_edit::Edit::Compress { preset },
        )
        .map(|bytes| bytes.len() as u64)
        .map_err(|e| e.to_string())
    })
    .await
}

/// Joins `paths`, in the order given, into a new PDF named `name` next to the first one.
/// Returns the job id.
#[tauri::command(async)]
pub fn pdf_merge(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    queue: tauri::State<'_, Arc<Queue>>,
    paths: Vec<String>,
    name: String,
) -> CmdResult<u64> {
    let sources = self::paths(paths);
    let label = format!("Merge {} PDFs into \"{name}\"", sources.len());
    let inputs = sources.clone();
    submit_pdf(&app, &window, &queue, label, sources, name, move || {
        ops::pdf::merge(&inputs)
    })
}

/// Makes a smaller copy of the PDF at `path` as "`name`", next to it. Returns the job id.
#[tauri::command(async)]
pub fn pdf_compress(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    queue: tauri::State<'_, Arc<Queue>>,
    path: String,
    preset: ops::pdf::Preset,
    name: String,
) -> CmdResult<u64> {
    let source = PathBuf::from(path);
    let label = describe("Compress", std::slice::from_ref(&source));
    let input = source.clone();
    submit_pdf(
        &app,
        &window,
        &queue,
        label,
        vec![source],
        name,
        move || ops::pdf::compress(&input, preset),
    )
}

/// The name of an archive made from `sources`: the item's own name, or the folder's for
/// several.
fn archive_name(sources: &[PathBuf]) -> String {
    let stem = |p: &std::path::Path| {
        let name = p.file_name().map(|n| n.to_string_lossy().into_owned());
        // A file loses its extension; a folder keeps what looks like one.
        match (name, p.is_dir()) {
            (Some(n), true) => Some(n),
            (Some(n), false) => Some(
                n.rsplit_once('.')
                    .filter(|(s, _)| !s.is_empty())
                    .map_or(n.clone(), |(s, _)| s.to_owned()),
            ),
            _ => None,
        }
    };
    let base = match sources {
        [one] => stem(one),
        _ => sources.first().and_then(|p| p.parent()).and_then(stem),
    };
    format!("{}.zip", base.unwrap_or_else(|| "Archive".into()))
}

/// Packs `paths` (from one folder) into a new zip next to them, called `name` or named after
/// them. Links are left out. Undo moves the archive to the Recycle Bin. Returns the job id.
#[tauri::command(async)]
pub fn zip_compress(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    queue: tauri::State<'_, Arc<Queue>>,
    paths: Vec<String>,
    name: Option<String>,
) -> CmdResult<u64> {
    let sources = self::paths(paths);
    let dir = sources
        .first()
        .and_then(|p| p.parent())
        .ok_or("Nothing to compress")?
        .to_path_buf();
    if sources.iter().any(|p| p.parent() != Some(dir.as_path())) {
        return Err("Compress items from one folder at a time".into());
    }
    let name = name.unwrap_or_else(|| archive_name(&sources));
    ops::validate_name(&name).map_err(|e| e.to_string())?;
    if !name.to_lowercase().ends_with(".zip") || cleanup::is_partial_name(&name) {
        return Err(format!("\"{name}\" must end in .zip"));
    }
    let lane = ops::volume_key(&dir);
    let label = describe("Compress", &sources);
    let title = label.clone();
    let handle = app.clone();
    let work: queue::Work = Box::new(move |_progress| {
        let journal = handle.state::<Journal>();
        let pending = begin(&journal, &label)?;
        let result = ops::archive::compress(&sources, &dir, &name);
        let steps: Vec<Step> = result
            .as_ref()
            .map(|p| Step::made_from(p.path.clone(), sources.clone()))
            .into_iter()
            .collect();
        journal.finish(pending, &steps);
        let packed = result.map_err(|e| e.to_string())?;
        let before = ops::total_bytes(&sources);
        let after = std::fs::metadata(&packed.path).map_or(0, |m| m.len());
        let note = (packed.skipped > 0).then(|| {
            format!(
                "Compressed, but left out {} item{}: links, and names that are not text, can't go in an archive.",
                packed.skipped,
                if packed.skipped == 1 { "" } else { "s" }
            )
        });
        Ok(serde_json::to_value(MadeFile {
            before,
            after,
            compressed: true,
            note,
            message: None,
            path: packed.path.to_string_lossy().into_owned(),
        })
        .unwrap_or_default())
    });
    Ok(queue.submit(&app, window.label(), Kind::Edit, title, &lane, work))
}

/// Unpacks the zip at `path` into a new folder next to it, named after it. Entries that are
/// links or point outside the folder are left out. Undo moves the folder to the Recycle Bin.
/// Returns the job id.
#[tauri::command(async)]
pub fn zip_extract(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    queue: tauri::State<'_, Arc<Queue>>,
    path: String,
) -> CmdResult<u64> {
    let archive = PathBuf::from(path);
    let dir = archive
        .parent()
        .ok_or("No folder to unpack into")?
        .to_path_buf();
    let lane = ops::volume_key(&dir);
    let label = describe("Extract", std::slice::from_ref(&archive));
    let title = label.clone();
    let handle = app.clone();
    let work: queue::Work = Box::new(move |_progress| {
        let journal = handle.state::<Journal>();
        let pending = begin(&journal, &label)?;
        let result = ops::archive::extract(&archive);
        let steps: Vec<Step> = result
            .as_ref()
            .map(|u| Step::made_from(u.path.clone(), vec![archive.clone()]))
            .into_iter()
            .collect();
        journal.finish(pending, &steps);
        let unpacked = result.map_err(|e| e.to_string())?;
        let name = unpacked
            .path
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let note = (unpacked.skipped > 0).then(|| {
            format!(
                "Extracted to \"{name}\", but left out {} entr{} that were links or pointed outside the folder.",
                unpacked.skipped,
                if unpacked.skipped == 1 { "y" } else { "ies" }
            )
        });
        Ok(serde_json::to_value(MadeFile {
            before: std::fs::metadata(&archive).map_or(0, |m| m.len()),
            after: ops::total_bytes(std::slice::from_ref(&unpacked.path)),
            compressed: false,
            note,
            message: Some(format!(
                "Extracted {} to \"{name}\"",
                if unpacked.files == 1 {
                    "1 file".to_owned()
                } else {
                    format!("{} files", unpacked.files)
                }
            )),
            path: unpacked.path.to_string_lossy().into_owned(),
        })
        .unwrap_or_default())
    });
    Ok(queue.submit(&app, window.label(), Kind::Edit, title, &lane, work))
}

/// Whether an update may be installed now. Installing can end the app (the Windows installer
/// closes it), so it must not start while a copy, move or delete is going: that would leave
/// a half-done job for the next start to find.
#[tauri::command]
pub fn update_gate(queue: tauri::State<'_, Arc<Queue>>) -> Result<(), String> {
    if queue.is_busy() {
        return Err("Folio is still working on files. Install the update when it is done.".into());
    }
    Ok(())
}

/// Starts the app again after an update was installed (Windows already did, by itself).
#[tauri::command]
pub fn update_restart(
    app: tauri::AppHandle,
    queue: tauri::State<'_, Arc<Queue>>,
) -> Result<(), String> {
    update_gate(queue)?;
    app.restart()
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replacing_an_original_that_changed_leaves_it_alone_and_no_staged_file_behind() {
        let dir = tempfile::tempdir().unwrap();
        let photo = dir.path().join("photo.jpg");
        std::fs::write(&photo, "old").unwrap();
        let original = Original {
            path: &photo,
            dir: dir.path(),
            name: "photo.jpg",
            stamp: stamp(&photo).unwrap(),
            identity: ops::identity(&photo),
        };
        // Edited by another program after the edit was asked for.
        std::fs::write(&photo, "edited meanwhile, and longer").unwrap();
        let journal = Journal::open(None).unwrap();
        let e = replace_original(&journal, "Rotate", &original, b"new")
            .err()
            .unwrap();
        assert!(e.contains("changed while"), "{e}");
        assert_eq!(
            std::fs::read(&photo).unwrap(),
            b"edited meanwhile, and longer"
        );
        let names: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["photo.jpg"]);
        // The journal entry is closed with nothing in it, so undo is not blocked.
        assert!(journal.undo().is_err());
    }
}
