//! Living *in* the desktop rather than on top of it.
//!
//! Windows keeps the wallpaper in a `WorkerW` window that sits behind the icon
//! layer (`SHELLDLL_DefView`). Poking `Progman` with the undocumented `0x052C`
//! message makes the shell spawn that WorkerW; re-parenting into it is how live
//! wallpapers put content on the desktop itself.
//!
//! The trade-off is real and the caller should mean it: on the desktop layer
//! the companion is genuinely part of the desktop — but he is behind every
//! application window, so he is only visible when the desktop is.

use windows_sys::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    FindWindowExW, FindWindowW, SendMessageTimeoutW, SetParent, SMTO_NORMAL,
};

use crate::text::wide;

/// Adopt the companion into the desktop's own window hierarchy.
pub fn parent_to_desktop(hwnd: HWND) -> bool {
    unsafe {
        let progman = FindWindowW(wide("Progman").as_ptr(), std::ptr::null());
        if progman.is_null() {
            tracing::warn!("Progman not found; staying a normal window");
            return false;
        }

        // Ask the shell to split the wallpaper into its own WorkerW.
        let mut result: usize = 0;
        SendMessageTimeoutW(
            progman,
            0x052C,
            0x0D as WPARAM,
            0x01 as LPARAM,
            SMTO_NORMAL,
            1000,
            &mut result,
        );

        // Windows 10 splits the wallpaper into its own WorkerW. Windows 11
        // often keeps the icon host (SHELLDLL_DefView) directly under Progman
        // and never spawns one, so fall back to Progman itself — content
        // parented there still draws on the desktop surface.
        let (host, which) = match find_wallpaper_worker() {
            Some(w) => (w, "WorkerW"),
            None => (progman, "Progman"),
        };

        if SetParent(hwnd, host).is_null() {
            tracing::warn!("SetParent into the desktop layer failed");
            return false;
        }
        tracing::info!("companion adopted into the desktop layer via {which}");
    }
    true
}

/// The WorkerW we want is the one that is *not* the icon host — i.e. the
/// sibling that has no `SHELLDLL_DefView` child.
unsafe fn find_wallpaper_worker() -> Option<HWND> {
    let mut worker: HWND = std::ptr::null_mut();
    loop {
        worker = FindWindowExW(
            std::ptr::null_mut(),
            worker,
            wide("WorkerW").as_ptr(),
            std::ptr::null(),
        );
        if worker.is_null() {
            return None;
        }
        let defview = FindWindowExW(
            worker,
            std::ptr::null_mut(),
            wide("SHELLDLL_DefView").as_ptr(),
            std::ptr::null(),
        );
        if defview.is_null() {
            // No icon host under this one: it is the wallpaper surface.
            continue;
        }
        // The wallpaper WorkerW is the next sibling after the icon host.
        let sibling = FindWindowExW(
            std::ptr::null_mut(),
            worker,
            wide("WorkerW").as_ptr(),
            std::ptr::null(),
        );
        if !sibling.is_null() {
            return Some(sibling);
        }
    }
}
