mod commands;
mod fs;
mod ops;
mod perf;
mod platform;
mod thumbs;
mod watch;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    perf::mark_start();
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(watch::DirWatcher::default())
        .register_asynchronous_uri_scheme_protocol(thumbs::SCHEME, |_ctx, request, responder| {
            tauri::async_runtime::spawn_blocking(move || {
                responder.respond(thumbs::handle(&request))
            });
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_dir,
            commands::home_dir,
            commands::watch_dir,
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
