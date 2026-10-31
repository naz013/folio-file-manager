//! Creates additional browser windows (restored at launch or torn off from a tab).
//! They match the main window in `tauri.conf.json`: no native title bar, since the
//! tab strip and window controls are drawn by the UI.

use tauri::{AppHandle, WebviewUrl, WebviewWindowBuilder};

pub fn open(app: &AppHandle, label: &str, position: Option<(f64, f64)>) -> tauri::Result<()> {
    let builder = WebviewWindowBuilder::new(app, label, WebviewUrl::App("index.html".into()))
        .title("Folio")
        .inner_size(1280.0, 800.0)
        .min_inner_size(900.0, 560.0)
        .decorations(false);
    let builder = match position {
        Some((x, y)) => builder.position(x, y),
        None => builder.center(),
    };
    builder.build()?;
    Ok(())
}
