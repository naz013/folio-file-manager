//! Thin Tauri command layer. Validation and error mapping only; logic lives in modules.

use crate::fs::{self, Entry};
use std::path::PathBuf;
use tauri::Manager;

#[tauri::command]
pub fn list_dir(path: String) -> Result<Vec<Entry>, String> {
    fs::list_dir(&PathBuf::from(&path)).map_err(|e| format!("Cannot open {path}: {e}"))
}

#[tauri::command]
pub fn home_dir(app: tauri::AppHandle) -> Result<String, String> {
    app.path()
        .home_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .map_err(|e| e.to_string())
}
