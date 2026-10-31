//! Native Windows shell context menu (the classic "Show more options" menu),
//! so third-party entries like 7-Zip or "Open with Code" work in Folio.

use std::cell::RefCell;
use std::mem::size_of;
use std::path::PathBuf;
use std::ptr::null_mut;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::{
    BHID_SFUIObject, CMF_EXPLORE, CMF_EXTENDEDVERBS, CMF_NORMAL, CMIC_MASK_PTINVOKE,
    CMINVOKECOMMANDINFO, CMINVOKECOMMANDINFOEX, DefSubclassProc, IContextMenu, IContextMenu2,
    IContextMenu3, ILFree, RemoveWindowSubclass, SHCreateShellItemArrayFromIDLists,
    SHParseDisplayName, SetWindowSubclass,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreatePopupMenu, DestroyMenu, GetCursorPos, SW_SHOWNORMAL, TPM_RETURNCMD, TPM_RIGHTBUTTON,
    TrackPopupMenuEx, WM_DRAWITEM, WM_INITMENUPOPUP, WM_MEASUREITEM, WM_MENUCHAR,
};
use windows::core::{HSTRING, Interface, PCSTR, PCWSTR};

const CMIC_MASK_UNICODE: u32 = 0x4000;
const FIRST_CMD: u32 = 1;
const LAST_CMD: u32 = 0x7FFF;
const SUBCLASS_ID: usize = 0xF011;

thread_local! {
    /// The menu being tracked, so the window subclass can forward owner-draw
    /// messages ("Open with" and "Send to" submenus are filled lazily).
    static ACTIVE: RefCell<Option<IContextMenu2>> = const { RefCell::new(None) };
}

/// Shows the shell menu for `paths` at the cursor and runs the chosen command.
/// Must be called on the UI thread that owns `hwnd`. `extended` adds Shift+right-click verbs.
pub fn show(hwnd: HWND, paths: &[PathBuf], extended: bool) -> windows::core::Result<()> {
    // SAFETY: called on the UI thread that owns `hwnd` (see above); the popup menu is
    // destroyed and the window subclass removed before returning, and `pt` outlives the calls.
    unsafe {
        let shell_menu = context_menu_for(paths)?;
        let menu = CreatePopupMenu()?;
        let mut flags = CMF_NORMAL | CMF_EXPLORE;
        if extended {
            flags |= CMF_EXTENDEDVERBS;
        }
        let result = (|| {
            shell_menu
                .QueryContextMenu(menu, 0, FIRST_CMD, LAST_CMD, flags)
                .ok()?;
            let mut pt = POINT::default();
            GetCursorPos(&raw mut pt)?;

            let helper = shell_menu.cast::<IContextMenu2>().ok();
            let subclassed = helper.is_some()
                && SetWindowSubclass(hwnd, Some(subclass_proc), SUBCLASS_ID, 0).as_bool();
            ACTIVE.with(|a| *a.borrow_mut() = helper);
            // With TPM_RETURNCMD the result is the chosen command id, or 0 for none.
            let cmd = u32::try_from(
                TrackPopupMenuEx(
                    menu,
                    (TPM_RETURNCMD | TPM_RIGHTBUTTON).0,
                    pt.x,
                    pt.y,
                    hwnd,
                    None,
                )
                .0,
            )
            .unwrap_or(0);
            ACTIVE.with(|a| *a.borrow_mut() = None);
            if subclassed {
                let _ = RemoveWindowSubclass(hwnd, Some(subclass_proc), SUBCLASS_ID);
            }

            if cmd >= FIRST_CMD {
                invoke(&shell_menu, hwnd, cmd - FIRST_CMD, pt)?;
            }
            Ok(())
        })();
        let _ = DestroyMenu(menu);
        result
    }
}

/// Builds the shell's `IContextMenu` for one or more items (multi-selection aware).
///
/// # Safety
/// COM must be initialized on the calling thread.
unsafe fn context_menu_for(paths: &[PathBuf]) -> windows::core::Result<IContextMenu> {
    let mut pidls: Vec<*mut ITEMIDLIST> = Vec::with_capacity(paths.len());
    // SAFETY: each path is passed as an owned HSTRING; every PIDL the shell returns is kept in
    // `pidls` and stays valid until it is freed below, after the shell item array is built.
    let result = (|| unsafe {
        for path in paths {
            let mut pidl = null_mut();
            SHParseDisplayName(
                &HSTRING::from(path.as_os_str()),
                None,
                &raw mut pidl,
                0,
                None,
            )?;
            pidls.push(pidl);
        }
        let ids: Vec<*const ITEMIDLIST> = pidls.iter().map(|p| (*p).cast_const()).collect();
        let items = SHCreateShellItemArrayFromIDLists(&ids)?;
        items.BindToHandler::<_, IContextMenu>(None, &BHID_SFUIObject)
    })();
    for pidl in pidls {
        // SAFETY: `pidl` came from SHParseDisplayName and is freed exactly once.
        unsafe { ILFree(Some(pidl)) };
    }
    result
}

/// Runs the menu command at `offset`.
///
/// # Safety
/// `hwnd` must be a valid window owned by the calling thread.
#[expect(
    clippy::cast_possible_truncation,
    reason = "the struct size is a small compile-time constant"
)]
unsafe fn invoke(
    menu: &IContextMenu,
    hwnd: HWND,
    offset: u32,
    pt: POINT,
) -> windows::core::Result<()> {
    // Verbs are passed as MAKEINTRESOURCE(offset).
    let info = CMINVOKECOMMANDINFOEX {
        cbSize: size_of::<CMINVOKECOMMANDINFOEX>() as u32,
        fMask: CMIC_MASK_UNICODE | CMIC_MASK_PTINVOKE,
        hwnd,
        lpVerb: PCSTR(offset as usize as *const u8),
        lpVerbW: PCWSTR(offset as usize as *const u16),
        nShow: SW_SHOWNORMAL.0,
        ptInvoke: pt,
        ..Default::default()
    };
    // SAFETY: CMINVOKECOMMANDINFOEX starts with the CMINVOKECOMMANDINFO layout and `cbSize`
    // tells the shell which one it got; `info` lives until the call returns.
    unsafe { menu.InvokeCommand((&raw const info).cast::<CMINVOKECOMMANDINFO>()) }
}

unsafe extern "system" fn subclass_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    if matches!(
        msg,
        WM_INITMENUPOPUP | WM_DRAWITEM | WM_MEASUREITEM | WM_MENUCHAR
    ) {
        let handled = ACTIVE.with(|active| {
            let menu = active.borrow().clone()?;
            // SAFETY: the window procedure runs on the UI thread with the message parameters
            // Windows passed in; `result` lives until the call returns.
            unsafe {
                if let Ok(menu3) = menu.cast::<IContextMenu3>() {
                    let mut result = LRESULT(0);
                    menu3
                        .HandleMenuMsg2(msg, wparam, lparam, Some(&raw mut result))
                        .ok()
                        .map(|()| result)
                } else {
                    menu.HandleMenuMsg(msg, wparam, lparam)
                        .ok()
                        .map(|()| LRESULT(0))
                }
            }
        });
        if let Some(result) = handled {
            return result;
        }
    }
    // SAFETY: forwards the unchanged message to the next procedure in the subclass chain.
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}
