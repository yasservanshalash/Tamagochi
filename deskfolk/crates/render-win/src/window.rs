//! The companion window: a layered popup we own end to end.
//!
//! Everything the old webview companion needed a workaround for is simply
//! absent here. There is no host window drawing caption buttons, so there is
//! nothing to strip; no non-client area, so nothing to collapse; no Mica
//! backdrop, so nothing to disable. `WS_POPUP` plus `WS_EX_LAYERED` is a bare
//! rectangle of pixels that we fill ourselves.
//!
//! Two consequences worth stating plainly, because they delete a lot of code:
//!
//! * **Click-through is free.** Windows routes mouse input on a layered
//!   window by the alpha we last presented, so a click on a transparent pixel
//!   lands on the desktop with no cursor polling and no
//!   `set_ignore_cursor_events` at all. The 60Hz hit-test loop existed only
//!   because a webview window is all-or-nothing.
//! * **Dragging cannot tear.** `UpdateLayeredWindow` moves and repaints in one
//!   call, so there is never a frame where the window has moved but the
//!   character has not.

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, Once};

use deskfolk_engine::Portal;
use deskfolk_package::Stage;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetCursorPos,
    GetMessageW, GetWindowLongPtrW, LoadCursorW, PostMessageW, PostQuitMessage, RegisterClassW,
    SetWindowLongPtrW, SetWindowPos, ShowWindow, SystemParametersInfoW, TranslateMessage,
    GWLP_USERDATA, HWND_TOPMOST, IDC_ARROW, MA_NOACTIVATE, MSG, SPI_GETWORKAREA, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOSIZE, SW_HIDE, SW_SHOWNOACTIVATE, WM_APP, WM_CLOSE, WM_DESTROY,
    WM_DISPLAYCHANGE, WM_DPICHANGED, WM_HOTKEY, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEACTIVATE,
    WM_MOUSEMOVE, WM_RBUTTONUP, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOREDIRECTIONBITMAP,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

use crate::flyout;
use crate::hotkey;
use crate::paint;
use crate::sprites::Sprites;
use crate::surface::Surface;
use crate::text::{wide, TextRenderer};
use crate::{Config, Frame, Host};

/// A frame is waiting in `Shared::pending`.
const WM_FRAME: u32 = WM_APP + 1;
/// Shut the window down from another thread.
const WM_QUIT_COMPANION: u32 = WM_APP + 2;
/// He was moved by something other than a drag — a walk, or a ledge shifting
/// under him. Position changed, the frame did not.
const WM_MOVED: u32 = WM_APP + 3;

const CLASS_NAME: &str = "DeskfolkCompanion";
static REGISTER: Once = Once::new();

pub(crate) struct Shared {
    pub pending: Mutex<Option<Frame>>,
    /// Set while a `WM_FRAME` is in flight, so a 60Hz producer cannot flood
    /// the message queue faster than the window can paint.
    pub queued: AtomicBool,
    pub x: AtomicI32,
    pub y: AtomicI32,
    /// The monitor's DPI, so the app can map screen pixels back into stage
    /// units without guessing at the display scaling.
    pub dpi: AtomicU32,
    pub host: Arc<dyn Host>,
}

struct Drag {
    /// Cursor and window position when the button went down, in screen pixels.
    cursor: POINT,
    origin: (i32, i32),
    moved: bool,
}

struct WindowState {
    shared: Arc<Shared>,
    sprites: Sprites,
    surface: Option<Surface>,
    text: Option<TextRenderer>,
    stage: Stage,
    portal: Portal,
    scale: f64,
    /// The character's name, which heads his menu.
    name: String,
    pixel_snap: bool,
    /// Device pixels per stage unit — `scale` times the monitor's DPI factor.
    unit: f64,
    size: (i32, i32),
    drag: Option<Drag>,
    shown: bool,
    last: Option<Frame>,
}

impl WindowState {
    fn resize_for_dpi(&mut self, hwnd: HWND) {
        let dpi = unsafe { GetDpiForWindow(hwnd) };
        let dpi = if dpi == 0 { 96 } else { dpi };
        self.shared.dpi.store(dpi, Ordering::Relaxed);
        let factor = dpi as f64 / 96.0;
        self.unit = snap_unit(self.scale * factor, self.pixel_snap);
        let w = (self.stage.width as f64 * self.unit).round() as i32;
        let h = (self.stage.height as f64 * self.unit).round() as i32;
        if self.size != (w, h) || self.surface.is_none() {
            self.size = (w, h);
            self.surface = Surface::new(w, h);
            // Font size follows the companion, not the desktop: he is drawn at
            // `scale`, and a caption that ignored it would look pasted on.
            let font_px = (12.0 * self.unit).round().max(11.0) as i32;
            self.text = TextRenderer::new(font_px);
            if self.surface.is_none() {
                tracing::error!("could not allocate a {w}x{h} drawing surface");
            }
        }
    }

    fn repaint(&mut self, hwnd: HWND) {
        let Some(frame) = self.last.clone() else { return };
        let Some(surface) = self.surface.as_mut() else { return };
        {
            let mut canvas = surface.canvas();
            paint::paint(
                &mut canvas,
                &self.sprites,
                self.text.as_mut(),
                &frame,
                &self.stage,
                self.portal,
                self.unit,
            );
        }
        let (x, y) = (
            self.shared.x.load(Ordering::Relaxed),
            self.shared.y.load(Ordering::Relaxed),
        );
        if !surface.present(hwnd, x, y) {
            tracing::warn!("UpdateLayeredWindow failed");
            return;
        }
        if !self.shown {
            self.shown = true;
            // Only now is there anything to look at. Showing earlier flashes an
            // empty rectangle on the desktop.
            unsafe { ShowWindow(hwnd, SW_SHOWNOACTIVATE) };
            tracing::info!(
                "companion window shown at ({x}, {y}), {}x{} device px, \
                 {} screen px per art pixel",
                self.size.0,
                self.size.1,
                self.unit
            );
        }
    }
}

/// Round the scale to a whole number of screen pixels per art pixel.
///
/// This is the difference between pixel art and mush. At 1.3x, ten source
/// pixels become thirteen screen pixels — so three in every ten are doubled,
/// at irregular intervals. A 1px outline is then 1px thick along some of its
/// length and 2px along the rest, which is exactly the ragged, "dirty" edge
/// that gets blamed on the artwork. Only whole-number scaling keeps every
/// source pixel the same size on screen.
///
/// The cost is that the companion can only be sized in whole multiples, which
/// is a real constraint and why it can be turned off.
fn snap_unit(raw: f64, snap: bool) -> f64 {
    if !snap {
        return raw.max(0.1);
    }
    raw.round().max(1.0)
}

/// Park him in the bottom-right of the work area, clear of the taskbar.
fn default_position(w: i32, h: i32) -> (i32, i32) {
    let mut area = RECT {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1080,
    };
    unsafe {
        SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            &mut area as *mut RECT as *mut std::ffi::c_void,
            0,
        );
    }
    // The work area already excludes the taskbar, so a small margin is enough.
    ((area.right - w - 24).max(area.left), (area.bottom - h - 24).max(area.top))
}

pub(crate) struct Spawned {
    pub hwnd: isize,
    pub shared: Arc<Shared>,
}

/// Create the window on its own thread and return once it exists.
///
/// The window lives on a dedicated thread with its own message pump, so it is
/// completely independent of Tauri's event loop — Tauri stays behind for the
/// Control Center and the tray, and neither can stall the other.
pub(crate) fn spawn(
    config: Config,
    sprites: Sprites,
    host: Arc<dyn Host>,
) -> Result<Spawned, String> {
    let shared = Arc::new(Shared {
        pending: Mutex::new(None),
        queued: AtomicBool::new(false),
        x: AtomicI32::new(0),
        y: AtomicI32::new(0),
        dpi: AtomicU32::new(96),
        host,
    });
    let thread_shared = shared.clone();
    let (tx, rx) = mpsc::channel::<Result<isize, String>>();

    std::thread::Builder::new()
        .name("deskfolk-companion-window".into())
        .spawn(move || {
            let hwnd = match create_window(config.on_desktop) {
                Ok(h) => h,
                Err(e) => {
                    let _ = tx.send(Err(e));
                    return;
                }
            };
            // Claimed on this thread, because WM_HOTKEY is delivered to the
            // queue of whoever registered it.
            if let Some(spec) = config.hotkey.as_deref() {
                unsafe { hotkey::register(hwnd, spec) };
            }

            let mut state = Box::new(WindowState {
                shared: thread_shared,
                sprites,
                surface: None,
                text: None,
                stage: config.stage,
                portal: config.portal,
                scale: config.scale,
                name: config.name.clone(),
                pixel_snap: config.pixel_snap,
                unit: config.scale,
                size: (0, 0),
                drag: None,
                shown: false,
                last: None,
            });
            state.resize_for_dpi(hwnd);
            let (x, y) = default_position(state.size.0, state.size.1);
            state.shared.x.store(x, Ordering::Relaxed);
            state.shared.y.store(y, Ordering::Relaxed);

            unsafe {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(state) as isize);
            }
            let _ = tx.send(Ok(hwnd as isize));

            // Standard pump. `GetMessageW` blocks, so an idle companion costs
            // nothing until a frame or an input arrives.
            unsafe {
                let mut msg: MSG = std::mem::zeroed();
                while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
        })
        .map_err(|e| format!("could not start the companion window thread: {e}"))?;

    match rx.recv() {
        Ok(Ok(hwnd)) => Ok(Spawned { hwnd, shared }),
        Ok(Err(e)) => Err(e),
        Err(_) => Err("the companion window thread died during startup".into()),
    }
}

fn create_window(on_desktop: bool) -> Result<HWND, String> {
    let class = wide(CLASS_NAME);
    REGISTER.call_once(|| unsafe {
        let mut wc: WNDCLASSW = std::mem::zeroed();
        wc.lpfnWndProc = Some(wndproc);
        wc.hInstance = GetModuleHandleW(std::ptr::null());
        wc.lpszClassName = class.as_ptr();
        wc.hCursor = LoadCursorW(std::ptr::null_mut(), IDC_ARROW);
        RegisterClassW(&wc);
    });

    let title = wide("Deskfolk");
    // Always-on-top is meaningless once he lives under the icon layer, and
    // asking for both confuses the shell about where he belongs.
    let topmost = if on_desktop { 0 } else { WS_EX_TOPMOST };
    let hwnd = unsafe {
        CreateWindowExW(
            // LAYERED gives per-pixel alpha; TOOLWINDOW keeps him out of
            // Alt-Tab and the taskbar (he is furniture, not an app);
            // NOREDIRECTIONBITMAP stops Windows allocating a redirection
            // surface we never draw into.
            WS_EX_LAYERED | WS_EX_TOOLWINDOW | topmost | WS_EX_NOREDIRECTIONBITMAP,
            class.as_ptr(),
            title.as_ptr(),
            WS_POPUP,
            0,
            0,
            10,
            10,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            GetModuleHandleW(std::ptr::null()),
            std::ptr::null(),
        )
    };
    if hwnd.is_null() {
        return Err("CreateWindowExW returned null for the companion".into());
    }
    if on_desktop {
        crate::desktop::parent_to_desktop(hwnd);
    }
    Ok(hwnd)
}

unsafe fn state_of<'a>(hwnd: HWND) -> Option<&'a mut WindowState> {
    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut WindowState;
    if ptr.is_null() {
        None
    } else {
        Some(&mut *ptr)
    }
}

unsafe extern "system" fn wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        // Never steal focus. Clicking the character must not pull the user out
        // of whatever they were typing in.
        WM_MOUSEACTIVATE => return MA_NOACTIVATE as LRESULT,

        WM_FRAME => {
            if let Some(state) = state_of(hwnd) {
                state.shared.queued.store(false, Ordering::SeqCst);
                if let Some(frame) = state.shared.pending.lock().ok().and_then(|mut p| p.take()) {
                    state.last = Some(frame);
                }
                state.repaint(hwnd);
            }
            return 0;
        }

        WM_LBUTTONDOWN => {
            if let Some(state) = state_of(hwnd) {
                let mut cursor = POINT { x: 0, y: 0 };
                GetCursorPos(&mut cursor);
                state.drag = Some(Drag {
                    cursor,
                    origin: (
                        state.shared.x.load(Ordering::Relaxed),
                        state.shared.y.load(Ordering::Relaxed),
                    ),
                    moved: false,
                });
                // Capture keeps the moves coming even when the cursor slips
                // onto transparent pixels mid-drag, which is what used to drop
                // the character out from under the mouse.
                SetCapture(hwnd);
            }
            return 0;
        }

        WM_MOUSEMOVE => {
            if let Some(state) = state_of(hwnd) {
                if let Some(drag) = state.drag.as_mut() {
                    let mut cursor = POINT { x: 0, y: 0 };
                    GetCursorPos(&mut cursor);
                    let dx = cursor.x - drag.cursor.x;
                    let dy = cursor.y - drag.cursor.y;
                    // A few pixels of slop, so a click with a shaky hand is
                    // still a click.
                    if !drag.moved && dx.abs() + dy.abs() < 4 {
                        return 0;
                    }
                    drag.moved = true;
                    let (x, y) = (drag.origin.0 + dx, drag.origin.1 + dy);
                    state.shared.x.store(x, Ordering::Relaxed);
                    state.shared.y.store(y, Ordering::Relaxed);
                    state.repaint(hwnd);
                }
            }
            return 0;
        }

        WM_LBUTTONUP => {
            if let Some(state) = state_of(hwnd) {
                let drag = state.drag.take();
                ReleaseCapture();
                match drag {
                    Some(d) if d.moved => {
                        let (x, y) = (
                            state.shared.x.load(Ordering::Relaxed),
                            state.shared.y.load(Ordering::Relaxed),
                        );
                        state.shared.host.on_moved(x, y);
                    }
                    Some(_) => state.shared.host.on_click(),
                    None => {}
                }
            }
            return 0;
        }

        WM_RBUTTONUP => {
            if let Some(state) = state_of(hwnd) {
                let entries = state.shared.host.menu();
                if !entries.is_empty() {
                    // The cards are dealt from *him*, not from the cursor, so
                    // the menu belongs to the character rather than to the
                    // click — and his position decides which side they fly to.
                    let (x, y) = (
                        state.shared.x.load(Ordering::Relaxed),
                        state.shared.y.load(Ordering::Relaxed),
                    );
                    let rect = RECT {
                        left: x,
                        top: y,
                        right: x + state.size.0,
                        bottom: y + state.size.1,
                    };
                    flyout::open(&entries, &state.name, rect, state.shared.host.clone());
                }
            }
            return 0;
        }

        WM_HOTKEY => {
            if let Some(state) = state_of(hwnd) {
                state.shared.host.on_hotkey();
            }
            return 0;
        }

        WM_DPICHANGED | WM_DISPLAYCHANGE => {
            if let Some(state) = state_of(hwnd) {
                state.resize_for_dpi(hwnd);
                state.repaint(hwnd);
            }
            return 0;
        }

        WM_MOVED => {
            if let Some(state) = state_of(hwnd) {
                state.repaint(hwnd);
            }
            return 0;
        }

        WM_QUIT_COMPANION => {
            DestroyWindow(hwnd);
            return 0;
        }

        WM_CLOSE => {
            ShowWindow(hwnd, SW_HIDE);
            return 0;
        }

        WM_DESTROY => {
            hotkey::unregister(hwnd);
            let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut WindowState;
            if !ptr.is_null() {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                drop(Box::from_raw(ptr));
            }
            PostQuitMessage(0);
            return 0;
        }
        _ => {}
    }
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

/// Push a frame to the window thread. Cheap and non-blocking: the frame is
/// parked in `Shared` and a single wake-up message is posted for it.
pub(crate) fn present(hwnd: isize, shared: &Shared, frame: Frame) {
    if let Ok(mut slot) = shared.pending.lock() {
        *slot = Some(frame);
    }
    // Already-queued means the window has a wake-up pending and will pick up
    // whatever is newest when it gets there.
    if !shared.queued.swap(true, Ordering::SeqCst) {
        unsafe {
            PostMessageW(hwnd as HWND, WM_FRAME, 0, 0);
        }
    }
}

/// Move him, from any thread.
///
/// Writes the same two atomics a drag does and then asks the window thread to
/// repaint, so walking and dragging cannot end up with different ideas about
/// where he is. `WM_MOVED` rather than `WM_FRAME` because there may be no new
/// frame to show — standing still on a moving ledge is a position change and
/// nothing else.
pub(crate) fn move_to(hwnd: isize, shared: &Shared, x: i32, y: i32) {
    let same = shared.x.load(Ordering::Relaxed) == x && shared.y.load(Ordering::Relaxed) == y;
    if same {
        return;
    }
    shared.x.store(x, Ordering::Relaxed);
    shared.y.store(y, Ordering::Relaxed);
    unsafe {
        PostMessageW(hwnd as HWND, WM_MOVED, 0, 0);
    }
}

/// The desktop's ledges, with his own window left out of them.
pub(crate) fn ledges_excluding(hwnd: isize) -> Vec<crate::ledges::Ledge> {
    let mut area = RECT { left: 0, top: 0, right: 1920, bottom: 1080 };
    unsafe {
        SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            &mut area as *mut RECT as *mut std::ffi::c_void,
            0,
        );
    }
    crate::ledges::scan(
        hwnd,
        crate::ledges::RectLike {
            left: area.left,
            top: area.top,
            right: area.right,
            bottom: area.bottom,
        },
    )
}

pub(crate) fn close(hwnd: isize) {
    unsafe {
        PostMessageW(hwnd as HWND, WM_QUIT_COMPANION, 0, 0);
    }
}

/// Keep him above ordinary windows. Re-asserted rather than set once, because
/// a full-screen app can quietly take the top slot.
pub(crate) fn raise(hwnd: isize) {
    // Not while his own menu is up: the menu is topmost too, and raising him
    // now would put the character in front of the list he just opened.
    if flyout::is_open() {
        return;
    }
    unsafe {
        SetWindowPos(
            hwnd as HWND,
            HWND_TOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
    }
}

/// Cursor position in screen pixels.
pub fn cursor_pos() -> (i32, i32) {
    let mut p = POINT { x: 0, y: 0 };
    unsafe {
        GetCursorPos(&mut p);
    }
    (p.x, p.y)
}

#[cfg(test)]
mod tests {
    use super::snap_unit;

    #[test]
    fn a_fractional_scale_rounds_to_whole_pixels() {
        // 1.3 was the default, and it is why a 1px outline came out 1px thick
        // along part of its length and 2px along the rest.
        assert_eq!(snap_unit(1.3, true), 1.0);
        assert_eq!(snap_unit(1.6, true), 2.0);
        assert_eq!(snap_unit(2.0, true), 2.0);
    }

    #[test]
    fn snapping_never_scales_below_one_to_one() {
        // Rounding 0.4 to zero would divide by zero downstream, and rounding
        // it to nothing visible is not a size anyone asked for.
        assert_eq!(snap_unit(0.4, true), 1.0);
        assert_eq!(snap_unit(0.0, true), 1.0);
    }

    #[test]
    fn a_high_dpi_display_still_lands_on_whole_pixels() {
        // 1.0 companion scale on a 150% display: 1.5 device px per art px is
        // exactly the case that has to round rather than pass through.
        assert_eq!(snap_unit(1.0 * 1.5, true), 2.0);
        assert_eq!(snap_unit(1.0 * 1.25, true), 1.0);
    }

    #[test]
    fn snapping_can_be_turned_off_for_an_arbitrary_size() {
        assert_eq!(snap_unit(1.3, false), 1.3);
        // Still guarded, so a nonsense scale cannot collapse the window.
        assert_eq!(snap_unit(0.0, false), 0.1);
    }
}
