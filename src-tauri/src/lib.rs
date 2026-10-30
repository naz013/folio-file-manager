mod commands;
mod fs;
mod ops;
mod watch;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(watch::DirWatcher::default())
        .invoke_handler(tauri::generate_handler![
            commands::list_dir,
            commands::home_dir,
            commands::watch_dir,
            commands::open_path,
            commands::trash_paths,
            commands::rename_path,
            commands::create_folder,
            commands::copy_paths,
            commands::move_paths,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
