//! The macOS application menu. Every command is a shortcut of the page (`src/lib/shortcuts.ts`)
//! under the same id; choosing one sends that id to the focused window, which runs it as if its
//! keys had been pressed. Other systems keep the window without a menu bar.

/// One command in the menu: the id of the page shortcut, its label, and the key bound to it in
/// the menu. A key bound here reaches the page only as a menu action, so the page applies the
/// same rules it would to the key press. File commands that are not editing keep their keys in
/// the page.
pub struct Entry {
    pub id: &'static str,
    pub label: &'static str,
    pub accelerator: Option<&'static str>,
}

const fn entry(id: &'static str, label: &'static str, accelerator: Option<&'static str>) -> Entry {
    Entry {
        id,
        label,
        accelerator,
    }
}

pub const SETTINGS: Entry = entry("settings", "Settings…", Some("CmdOrCtrl+,"));

pub const FILE: &[Entry] = &[
    entry("tabNew", "New Tab", Some("CmdOrCtrl+T")),
    entry("tabClose", "Close Tab", Some("CmdOrCtrl+W")),
    entry("newFolder", "New Folder", None),
    entry("open", "Open", None),
    entry("quickLook", "Quick Look", None),
    entry("rename", "Rename", None),
    entry("trash", "Move to Trash", None),
];

/// The editing commands. They carry the usual keys, which the menu takes before the page sees
/// them; the page then copies files, or edits the text when a text field has focus.
pub const EDIT: &[Entry] = &[
    entry("undo", "Undo", Some("CmdOrCtrl+Z")),
    entry("redo", "Redo", Some("CmdOrCtrl+Shift+Z")),
    entry("cut", "Cut", Some("CmdOrCtrl+X")),
    entry("copy", "Copy", Some("CmdOrCtrl+C")),
    entry("paste", "Paste", Some("CmdOrCtrl+V")),
    entry("selectAll", "Select All", Some("CmdOrCtrl+A")),
];

pub const GO: &[Entry] = &[
    entry("back", "Back", None),
    entry("forward", "Forward", None),
    entry("up", "Enclosing Folder", None),
    entry("goTo", "Go to Folder…", Some("CmdOrCtrl+L")),
    entry("search", "Search", Some("CmdOrCtrl+F")),
    entry("palette", "Command Palette", Some("CmdOrCtrl+K")),
];

pub const VIEW: &[Entry] = &[
    entry("hidden", "Show Hidden Files", Some("CmdOrCtrl+Shift+.")),
    entry("inspector", "Show Inspector", Some("CmdOrCtrl+I")),
    entry("panes", "Show Second Pane", Some("F8")),
    entry("paneSwitch", "Switch Pane", Some("F6")),
];

pub const HELP: &[Entry] = &[
    entry("shortcuts", "Keyboard Shortcuts", Some("F1")),
    entry("help", "Folio Help", Some("Shift+F1")),
];

#[cfg(target_os = "macos")]
pub fn install(app: &tauri::App) -> tauri::Result<()> {
    use tauri::menu::{Menu, MenuItemBuilder, MenuItemKind, SubmenuBuilder};
    use tauri::{Emitter, Manager};

    let handle = app.handle();
    let item = |e: &Entry| -> tauri::Result<tauri::menu::MenuItem<tauri::Wry>> {
        let mut b = MenuItemBuilder::with_id(e.id, e.label);
        if let Some(a) = e.accelerator {
            b = b.accelerator(a);
        }
        b.build(handle)
    };
    let submenu = |name: &str, entries: &[Entry]| -> tauri::Result<_> {
        let mut b = SubmenuBuilder::new(handle, name);
        for e in entries {
            b = b.item(&item(e)?);
        }
        b.build()
    };

    let settings = item(&SETTINGS)?;
    let app_menu = SubmenuBuilder::new(handle, "Folio")
        .about(None)
        .separator()
        .item(&settings)
        .separator()
        .services()
        .separator()
        .hide()
        .hide_others()
        .show_all()
        .separator()
        .quit()
        .build()?;
    let window = SubmenuBuilder::new(handle, "Window")
        .minimize()
        .maximize()
        .separator()
        .fullscreen()
        .build()?;
    let menu = Menu::with_items(
        handle,
        &[
            &MenuItemKind::Submenu(app_menu),
            &MenuItemKind::Submenu(submenu("File", FILE)?),
            &MenuItemKind::Submenu(submenu("Edit", EDIT)?),
            &MenuItemKind::Submenu(submenu("Go", GO)?),
            &MenuItemKind::Submenu(submenu("View", VIEW)?),
            &MenuItemKind::Submenu(window),
            &MenuItemKind::Submenu(submenu("Help", HELP)?),
        ],
    )?;
    app.set_menu(menu)?;
    app.on_menu_event(|app, event| {
        let id = event.id().as_ref();
        // Predefined items (Quit, Copy, ...) have ids of their own and are the system's.
        let ours = std::iter::once(&SETTINGS)
            .chain(FILE)
            .chain(EDIT)
            .chain(GO)
            .chain(VIEW)
            .chain(HELP)
            .any(|e| e.id == id);
        if !ours {
            return;
        }
        if let Some(window) = app.get_focused_window() {
            let _ = app.emit_to(window.label(), "menu-action", id);
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all() -> impl Iterator<Item = &'static Entry> {
        std::iter::once(&SETTINGS)
            .chain(FILE)
            .chain(EDIT)
            .chain(GO)
            .chain(VIEW)
            .chain(HELP)
    }

    #[test]
    fn ids_are_unique_and_labelled() {
        let mut seen = std::collections::HashSet::new();
        for e in all() {
            assert!(seen.insert(e.id), "{} appears twice", e.id);
            assert_ne!(e.label, "");
        }
    }

    #[test]
    fn keys_are_bound_once() {
        let mut seen = std::collections::HashSet::new();
        for a in all().filter_map(|e| e.accelerator) {
            assert!(seen.insert(a), "{a} is bound twice");
        }
    }
}
