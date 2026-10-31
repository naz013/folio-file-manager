//! Creates additional browser windows (restored at launch or torn off from a tab) and
//! keeps track of where every window sits. They match the main window in
//! `tauri.conf.json`: no native title bar, since the tab strip and window controls are
//! drawn by the UI.

use crate::session::{Area, Geometry, Session};
use std::time::Duration;
use tauri::{
    AppHandle, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder, Window,
};

/// How long after a move or resize the window's place is saved.
const SAVE_DELAY: Duration = Duration::from_millis(400);

/// Where a new window opens.
pub enum Place {
    /// Centered at the default size.
    Default,
    /// Torn off a tab: its top-left corner at a logical screen position, at the size of
    /// the window it came from.
    TornOff {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
    },
    /// Where a window was when the app last closed.
    Saved(Geometry),
}

pub fn open(app: &AppHandle, label: &str, place: &Place) -> tauri::Result<()> {
    let builder = WebviewWindowBuilder::new(app, label, WebviewUrl::App("index.html".into()))
        .title("Folio")
        .inner_size(1280.0, 800.0)
        .min_inner_size(900.0, 560.0)
        .decorations(false);
    let builder = match place {
        Place::Default => builder.center(),
        Place::TornOff {
            x,
            y,
            width,
            height,
        } => builder.position(*x, *y).inner_size(*width, *height),
        // Hidden until it is where it belongs, so it doesn't flash at the default spot.
        Place::Saved(_) => builder.visible(false),
    };
    let window = builder.build()?;
    if let Place::Saved(saved) = place {
        restore(&window, Some(*saved))?;
    }
    Ok(())
}

/// Shows `window`, which was created hidden, back where it was (if known), clamped to
/// the monitors that exist now.
pub fn restore(window: &WebviewWindow, saved: Option<Geometry>) -> tauri::Result<()> {
    let place = saved.and_then(|s| s.fit(&areas(window.app_handle())));
    if let Some(place) = place {
        // A failure leaves the window where it opened, which is fine.
        // Position first: moving to a monitor with another scale can rescale the size.
        let _ = window.set_position(PhysicalPosition::new(place.x, place.y));
        let _ = window.set_size(PhysicalSize::new(place.width, place.height));
    }
    window.show()?;
    // Only a shown window remembers these bounds to come back to.
    if place.is_some_and(|p| p.maximized) {
        // Best effort: the window is already shown where it was.
        let _ = window.maximize();
    }
    Ok(())
}

/// The usable area of each monitor, the primary one first.
fn areas(app: &AppHandle) -> Vec<Area> {
    let mut monitors = app.available_monitors().unwrap_or_default();
    if let Ok(Some(primary)) = app.primary_monitor() {
        monitors.sort_by_key(|m| m.position() != primary.position());
    }
    monitors
        .iter()
        .map(|m| {
            let rect = m.work_area();
            Area {
                x: rect.position.x,
                y: rect.position.y,
                width: rect.size.width,
                height: rect.size.height,
            }
        })
        .collect()
}

/// Notes that a window moved or resized. Where every window ended up is read once the
/// movement has stopped, so the in-between states of a drag, or a maximize that reports
/// the full-screen bounds first, never replace the bounds to come back to.
pub fn track(window: &Window) {
    // Events can arrive before the session is set up.
    let Some(session) = window.try_state::<Session>() else {
        return;
    };
    if session.schedule_flush() {
        let app = window.app_handle().clone();
        std::thread::spawn(move || {
            std::thread::sleep(SAVE_DELAY);
            for window in app.webview_windows().values() {
                settle(&window.as_ref().window());
            }
            app.state::<Session>().flush();
        });
    }
}

/// Records where `window` is now.
pub fn settle(window: &Window) {
    let session = window.state::<Session>();
    if let Some(geometry) = current(window, &session) {
        session.set_geometry(window.label(), geometry);
    }
}

/// Where `window` is now; `None` while it is minimized, when its position is meaningless.
fn current(window: &Window, session: &Session) -> Option<Geometry> {
    if window.is_minimized().ok()? || window.is_fullscreen().ok()? {
        return None;
    }
    let maximized = window.is_maximized().ok()?;
    if maximized {
        // Keep the bounds to come back to, not the full-screen ones; with none known
        // yet there is nothing worth saving.
        let before = session.geometry(window.label())?;
        return Some(Geometry {
            maximized,
            ..before
        });
    }
    let position = window.outer_position().ok()?;
    let size = window.inner_size().ok()?;
    Some(Geometry {
        x: position.x,
        y: position.y,
        width: size.width,
        height: size.height,
        maximized,
    })
}
