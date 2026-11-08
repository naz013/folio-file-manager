//! macOS: Launch Services' default handler for `public.folder`. Finder keeps its own windows
//! and "Show in Finder"; this is only where an app asks for a folder to be opened. The app
//! that held it before (Finder, normally) is kept in the app's folder and put back when the
//! setting is turned off.

use std::path::Path;

use core_foundation::base::TCFType;
use core_foundation::string::{CFString, CFStringRef};

/// This app's bundle identifier (`identifier` in `tauri.conf.json`).
const OURS: &str = "app.folio.desktop";

/// What opens folders unless something else is chosen.
const FINDER: &str = "com.apple.finder";

const FOLDER: &str = "public.folder";

/// Where the previous handler is kept, in the app's folder.
const KEPT: &str = "previous-folder-handler";

/// `kLSRolesAll`: every role (viewer, editor and so on).
const ROLES_ALL: u32 = 0xFFFF_FFFF;

#[link(name = "CoreServices", kind = "framework")]
unsafe extern "C" {
    fn LSSetDefaultRoleHandlerForContentType(
        content_type: CFStringRef,
        role: u32,
        handler: CFStringRef,
    ) -> i32;
    fn LSCopyDefaultRoleHandlerForContentType(content_type: CFStringRef, role: u32) -> CFStringRef;
}

fn handler() -> Option<String> {
    let kind = CFString::new(FOLDER);
    // SAFETY: `kind` is a live CFString; the call returns an owned (+1) string or null.
    let found =
        unsafe { LSCopyDefaultRoleHandlerForContentType(kind.as_concrete_TypeRef(), ROLES_ALL) };
    if found.is_null() {
        return None;
    }
    // SAFETY: `found` is non-null and was returned by a "Copy" function, so we own it.
    Some(unsafe { CFString::wrap_under_create_rule(found) }.to_string())
}

fn set_handler(bundle: &str) -> Result<(), String> {
    let kind = CFString::new(FOLDER);
    let app = CFString::new(bundle);
    // SAFETY: both arguments are live CFStrings for the whole call.
    let status = unsafe {
        LSSetDefaultRoleHandlerForContentType(
            kind.as_concrete_TypeRef(),
            ROLES_ALL,
            app.as_concrete_TypeRef(),
        )
    };
    if status == 0 {
        Ok(())
    } else {
        Err(format!("macOS refused the change (error {status})"))
    }
}

pub fn is_default() -> bool {
    handler().is_some_and(|h| h.eq_ignore_ascii_case(OURS))
}

pub fn enable(data: &Path) -> Result<(), String> {
    if let Some(before) = handler().filter(|h| !h.eq_ignore_ascii_case(OURS)) {
        std::fs::create_dir_all(data).map_err(|e| e.to_string())?;
        std::fs::write(data.join(KEPT), before).map_err(|e| e.to_string())?;
    }
    set_handler(OURS)
}

pub fn disable(data: &Path) -> Result<(), String> {
    let previous = std::fs::read_to_string(data.join(KEPT))
        .ok()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case(OURS))
        .unwrap_or_else(|| FINDER.to_owned());
    if is_default() {
        set_handler(&previous)?;
    }
    let _ = std::fs::remove_file(data.join(KEPT));
    Ok(())
}
