mod commands;
mod fs;
mod journal;
mod ops;
mod perf;
mod platform;
mod queue;
mod session;
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

/// Starts the app and blocks until the last window closes.
///
/// # Panics
/// If the Tauri runtime fails to start; there is nothing to recover to.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    perf::mark_start();
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(watch::DirWatcher::default())
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
        .register_asynchronous_uri_scheme_protocol(thumbs::SCHEME, |_ctx, request, responder| {
            tauri::async_runtime::spawn_blocking(move || {
                responder.respond(thumbs::handle(&request));
            });
        })
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
            commands::show_context_menu,
            commands::trash_paths,
            commands::rename_path,
            commands::create_folder,
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
