mod bin;
mod cleanup;
#[cfg(test)]
mod cleanup_corpus;
mod commands;
mod crash;
mod fs;
mod journal;
mod ops;
mod perf;
mod platform;
mod preview;
mod queue;
mod scan;
mod session;
mod shelf;
mod tags;
mod thumbs;
mod watch;
mod window;

use std::sync::Arc;
use tauri::{Emitter, Manager, WindowEvent};

/// Opens the undo journal. A file that can't be opened (damaged, say) is put aside and a
/// new journal started, so one bad file doesn't stop the app from starting.
fn open_journal(file: Option<&std::path::Path>) -> rusqlite::Result<journal::Journal> {
    use rusqlite::ErrorCode::{DatabaseCorrupt, NotADatabase};
    match journal::Journal::open(file) {
        // Only a file that is not a usable journal is replaced; a busy or unwritable
        // one is left alone and the error stops the start.
        Err(rusqlite::Error::SqliteFailure(failure, _))
            if matches!(failure.code, DatabaseCorrupt | NotADatabase) && file.is_some() =>
        {
            let file = file.unwrap_or_else(|| std::path::Path::new(""));
            eprintln!("the undo journal is damaged; starting a new one");
            let stamp = journal::unix_now();
            // Kept, with anything SQLite left beside it, under a name that is never reused.
            for suffix in ["", "-journal", "-wal", "-shm"] {
                let mut from = file.as_os_str().to_owned();
                from.push(suffix);
                let mut to = file.as_os_str().to_owned();
                to.push(format!("{suffix}.damaged-{stamp}"));
                let _ = std::fs::rename(from, to);
            }
            journal::Journal::open(Some(file))
        }
        other => other,
    }
}

/// Opens the tag index in the app data directory.
fn manage_tags(app: &tauri::App) {
    let file = app
        .path()
        .app_data_dir()
        .ok()
        .map(|d| d.join("tags.sqlite"));
    app.manage(open_tags(file.as_deref()));
}

/// Opens the tag index and the clipboard shelf, which live beside the session and journal.
fn manage_stores(app: &tauri::App) {
    manage_tags(app);
    manage_shelf(app);
    manage_scanner(app);
}

/// Opens the size cache in the app data directory. Without it sizes are just not kept
/// between runs.
fn manage_scanner(app: &tauri::App) {
    let file = app
        .path()
        .app_data_dir()
        .ok()
        .map(|d| d.join("scan.sqlite"));
    let scanner = scan::Scanner::open(file.as_deref()).unwrap_or_else(|e| {
        eprintln!("the size cache can't be opened ({e}); sizes won't be kept");
        scan::Scanner::open(None).expect("an in-memory cache opens")
    });
    app.manage(scanner);
    // Roots scanned in an earlier run are watched again, off the start-up path.
    let handle = app.handle().clone();
    std::thread::spawn(move || handle.state::<scan::Scanner>().resume_watching());
}

/// Opens the clipboard shelf in the app data directory. Like tags, a shelf that can't be
/// opened is not worth stopping for: it just isn't kept.
fn manage_shelf(app: &tauri::App) {
    let file = app
        .path()
        .app_data_dir()
        .ok()
        .map(|d| d.join("shelf.sqlite"));
    let shelf = shelf::Shelf::open(file.as_deref()).unwrap_or_else(|e| {
        eprintln!("the shelf can't be opened ({e}); it won't be kept");
        shelf::Shelf::open(None).expect("an in-memory shelf opens")
    });
    app.manage(shelf);
}

/// Tags are not worth stopping for: without an index they just aren't kept.
fn open_tags(file: Option<&std::path::Path>) -> tags::Tags {
    tags::Tags::open(file).unwrap_or_else(|e| {
        eprintln!("the tag index can't be opened ({e}); tags won't be kept");
        tags::Tags::open(None).expect("an in-memory index opens")
    })
}

/// The custom URL schemes the webview loads files through.
trait Protocols {
    fn with_protocols(self) -> Self;
}

impl<R: tauri::Runtime> Protocols for tauri::Builder<R> {
    fn with_protocols(self) -> Self {
        self.register_asynchronous_uri_scheme_protocol(
            preview::SCHEME,
            |ctx, request, responder| {
                let app = ctx.app_handle().clone();
                tauri::async_runtime::spawn_blocking(move || {
                    let current = app.state::<preview::Current>();
                    responder.respond(preview::handle(&current, &request));
                });
            },
        )
        .register_asynchronous_uri_scheme_protocol(
            thumbs::SCHEME,
            |_ctx, request, responder| {
                tauri::async_runtime::spawn_blocking(move || {
                    responder.respond(thumbs::handle(&request));
                });
            },
        )
    }
}

/// Starts the app and blocks until the last window closes.
///
/// # Panics
/// If the Tauri runtime fails to start; there is nothing to recover to.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
#[expect(
    clippy::too_many_lines,
    reason = "the list of commands handed to Tauri grows with every feature"
)]
pub fn run() {
    perf::mark_start();
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(watch::DirWatcher::default())
        .manage(preview::Current::default())
        .setup(|app| {
            let file = app
                .path()
                .app_data_dir()
                .ok()
                .map(|d| d.join("session.json"));
            let (session, extra) = session::Session::load(file);
            app.manage(session);
            app.manage(queue::Queue::start(app.handle()));
            let journal_file = app
                .path()
                .app_data_dir()
                .ok()
                .map(|d| d.join("journal.sqlite"));
            // Without a journal nothing could be undone, and operations are refused, so
            // a journal that can't open is a failed start.
            app.manage(open_journal(journal_file.as_deref())?);
            manage_stores(app);
            // Always managed, so the commands never meet missing state; without an app
            // data folder the files simply fail to write.
            let dir = app
                .path()
                .app_data_dir()
                .unwrap_or_else(|_| std::env::temp_dir().join("folio"))
                .join("crash");
            crash::init(&dir);
            app.manage(commands::CrashDir(dir));
            // The main window comes from tauri.conf.json, hidden until it is back where
            // it was. Other saved windows open in the background so they don't hold up
            // the main window's start.
            if let Some(main) = app.get_webview_window(session::MAIN_LABEL) {
                let saved = app
                    .state::<session::Session>()
                    .geometry(session::MAIN_LABEL);
                window::restore(&main, saved)?;
            }
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                for label in extra {
                    let place = handle
                        .state::<session::Session>()
                        .geometry(&label)
                        .map_or(window::Place::Default, window::Place::Saved);
                    if let Err(e) = window::open(&handle, &label, &place) {
                        eprintln!("restoring window {label} failed: {e}");
                        handle.state::<session::Session>().close(&label);
                    }
                }
            });
            Ok(())
        })
        .on_window_event(|window, event| match event {
            WindowEvent::CloseRequested { api, .. } => {
                // Closing would stop the window's copies and moves half way.
                let working = window
                    .try_state::<Arc<queue::Queue>>()
                    .is_some_and(|q| q.has_active(window.label()));
                if working {
                    api.prevent_close();
                    let _ = window.emit("queue-close-blocked", ());
                    return;
                }
                let session = window.state::<session::Session>();
                window::settle(window);
                session.close(window.label());
                // The last window's place is kept, and may still be waiting to be saved.
                session.flush();
            }
            WindowEvent::Moved(_) | WindowEvent::Resized(_) => window::track(window),
            WindowEvent::Destroyed => window.state::<watch::DirWatcher>().unwatch(window.label()),
            _ => {}
        })
        .with_protocols()
        .invoke_handler(tauri::generate_handler![
            commands::list_dir,
            commands::home_dir,
            commands::watch_dir,
            commands::window_tabs,
            commands::save_tabs,
            commands::open_window,
            commands::open_path,
            commands::start_drag,
            commands::modifier_keys,
            commands::preview_allow,
            commands::tag_list,
            commands::tag_create,
            commands::tag_delete,
            commands::tag_toggle,
            commands::tag_files,
            commands::clipboard_mirror,
            commands::clipboard_take,
            commands::shelf_list,
            commands::shelf_add,
            commands::shelf_set_mode,
            commands::shelf_remove,
            commands::shelf_clear,
            commands::tags_in_dir,
            commands::read_text_preview,
            commands::show_context_menu,
            commands::trash_paths,
            commands::rename_path,
            commands::check_renames,
            commands::rename_many,
            commands::create_folder,
            commands::create_file,
            commands::open_terminal,
            commands::copy_paths,
            commands::move_paths,
            commands::find_conflicts,
            commands::queue_pause,
            commands::queue_resume,
            commands::queue_cancel,
            commands::queue_dismiss,
            commands::queue_jobs,
            commands::undo,
            commands::redo,
            commands::undo_state,
            commands::perf_config,
            commands::perf_report,
            commands::bin_list,
            commands::bin_sizes,
            commands::bin_restore,
            commands::bin_purge,
            commands::scan_start,
            commands::scan_cancel,
            commands::scan_children,
            commands::scan_large,
            commands::cleanup_large_old,
            commands::cleanup_old_downloads,
            commands::cleanup_trash,
            commands::cleanup_partials,
            commands::cleanup_trash_partials,
            commands::cleanup_duplicates,
            commands::cleanup_duplicates_cancel,
            commands::cleanup_trash_duplicates,
            commands::scan_running,
            commands::scan_size,
            commands::scan_roots,
            commands::scan_categories,
            commands::list_drives,
            commands::crash_state,
            commands::crash_set,
            commands::crash_clear,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event
                && let Some(dir) = app.try_state::<commands::CrashDir>()
            {
                crash::end_session(&dir.0);
            }
        });
}
