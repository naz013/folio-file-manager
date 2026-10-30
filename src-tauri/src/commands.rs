//! Thin Tauri command layer. Validation and error mapping only; logic lives in modules.
//!
//! File operations run on the blocking thread pool so the UI never freezes.

use crate::fs::{self, Entry};
use crate::ops;
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
    watcher: tauri::State<'_, DirWatcher>,
    path: String,
) -> CmdResult<()> {
    watcher
        .watch(app, &PathBuf::from(path))
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn open_path(path: String) -> CmdResult<()> {
    tauri_plugin_opener::open_path(&path, None::<&str>).map_err(|e| e.to_string())
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
