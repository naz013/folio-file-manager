// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // The elevated helper of the fast scan is this same program (docs/decisions/0004).
    if let Some(code) = folio_lib::helper_entry() {
        std::process::exit(code);
    }
    folio_lib::run();
}
