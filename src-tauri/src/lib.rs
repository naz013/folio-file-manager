mod commands;
mod fs;
mod ops;
mod perf;
mod platform;
mod session;
mod thumbs;
mod watch;
mod window;

use tauri::{Manager, WindowEvent};

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
            // The main window comes from tauri.conf.json. Other saved windows open in the
            // background so they don't hold up the main window's start.
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                for label in extra {
                    if let Err(e) = window::open(&handle, &label, None) {
                        eprintln!("restoring window {label} failed: {e}");
                        handle.state::<session::Session>().close(&label);
                    }
                }
            });
            Ok(())
        })
        .on_window_event(|window, event| match event {
            WindowEvent::CloseRequested { .. } => {
                window.state::<session::Session>().close(window.label());
            }
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
            commands::show_context_menu,
            commands::trash_paths,
            commands::rename_path,
            commands::create_folder,
            commands::copy_paths,
            commands::move_paths,
            commands::perf_config,
            commands::perf_report,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
