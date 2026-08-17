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
    SetTimer, SetWindowLongPtrW, SetWindowPos, ShowWindow, SystemParametersInfoW, TranslateMessage,
    GWLP_USERDATA, HWND_TOPMOST, IDC_ARROW, MA_NOACTIVATE, MSG, SPI_GETWORKAREA, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOSIZE, SW_HIDE, SW_SHOWNOACTIVATE, WM_APP, WM_CLOSE, WM_DESTROY,
    WM_DISPLAYCHANGE, WM_DPICHANGED, WM_HOTKEY, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEACTIVATE,
    WM_MOUSEMOVE, WM_RBUTTONUP, WM_TIMER, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOREDIRECTIONBITMAP,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

use crate::flyout;
use crate::hotkey;
use crate::modular_paint::{ModularScene, Overlays};
use crate::paint;
use crate::sprites::Sprites;
use crate::surface::Surface;
use crate::text::{wide, TextRenderer};
use crate::{Config, Frame, Host};
use deskfolk_engine::modular::CharacterState;

/// A frame is waiting in `Shared::pending`.
const WM_FRAME: u32 = WM_APP + 1;
/// Shut the window down from another thread.
const WM_QUIT_COMPANION: u32 = WM_APP + 2;
/// He was moved by something other than a drag — a walk, or a ledge shifting
/// under him. Position changed, the frame did not.
const WM_MOVED: u32 = WM_APP + 3;
/// Drives modular animation at a steady rate, independent of engine frames
/// (which are change-gated and stop arriving when he's idle).
const ANIM_TIMER_ID: usize = 1;
const ANIM_MS: u32 = 66; // ~15 fps

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
    /// Screen pixels per stage unit, times 1000 — the value actually being
    /// drawn with, after DPI *and* pixel snapping.
    ///
    /// Published because it cannot be recomputed from the outside: snapping
    /// rounds it to a whole number, so at scale 1.3 the window is drawn at 1.0
    /// and anything deriving a size from 1.3 is wrong by a third. It follows
    /// the monitor, so it changes when he is dragged to another display.
    pub unit_milli: AtomicU32,
    pub host: Arc<dyn Host>,
}

struct Drag {
    /// Cursor and window position when the button went down, in screen pixels.
    cursor: POINT,
    origin: (i32, i32),
    moved: bool,
    /// When the button went down — a long still press is a pet, not a click.
    pressed: std::time::Instant,
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
    /// Dev-only modular render path (behind `DESKFOLK_MODULAR=1`). When present,
    /// the window assembles this character from parts instead of drawing the
    /// classic single-sprite frame.
    modular: Option<ModularScene>,
    modular_state: Option<CharacterState>,
    /// Modular animation clock + derived idle-bob offset (art units), and the
    /// latest engine signals the driver reacts to (speaking, loudness).
    anim_tick: u32,
    bob: i32,
    blink_ticks: u32,
    spk_speaking: bool,
    spk_level: u8,
    /// Walk state, inferred from the window's own position deltas (he's moved by
    /// `move_to`), so no engine plumbing is needed for facing/locomotion.
    facing_left: bool,
    /// Whether to mirror the sprite this frame. The side art faces LEFT
    /// natively, so we flip only when he walks to the RIGHT.
    flip: bool,
    walking: bool,
    walk_phase: u32,
    last_x: i32,
    still_ticks: u32,
}

impl WindowState {
    fn resize_for_dpi(&mut self, hwnd: HWND) {
        let dpi = unsafe { GetDpiForWindow(hwnd) };
        let dpi = if dpi == 0 { 96 } else { dpi };
        self.shared.dpi.store(dpi, Ordering::Relaxed);
        let factor = dpi as f64 / 96.0;
        self.unit = snap_unit(self.scale * factor, self.pixel_snap);
        self.shared
            .unit_milli
            .store((self.unit * 1000.0).round() as u32, Ordering::Relaxed);
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
        // Modular dev path assembles from parts and needs no incoming frame; the
        // classic path draws the last frame it was sent.
        let modular = self.modular.is_some();
        if !modular && self.last.is_none() {
            return;
        }
        let Some(surface) = self.surface.as_mut() else { return };
        {
            let mut canvas = surface.canvas();
            if let (Some(scene), Some(st)) = (self.modular.as_ref(), self.modular_state.as_ref()) {
                canvas.clear();
                scene.paint(&mut canvas, st, self.unit, Overlays::default(), self.bob, self.flip);
            } else if let Some(frame) = self.last.as_ref() {
                paint::paint(
                    &mut canvas,
                    &self.sprites,
                    self.text.as_mut(),
                    frame,
                    &self.stage,
                    self.portal,
                    self.unit,
                );
            }
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

    /// Advance the modular animation one timer tick: idle bob, periodic blink,
    /// and a talk mouth-flap while speaking. Mutates `modular_state`; a no-op
    /// unless the modular path is active.
    fn tick_anim(&mut self) {
        if self.modular.is_none() {
            return;
        }
        self.anim_tick = self.anim_tick.wrapping_add(1);
        let t = self.anim_tick;

        // Infer walk state from the window's own horizontal motion.
        let x = self.shared.x.load(Ordering::Relaxed);
        let dx = x - self.last_x;
        self.last_x = x;
        if dx.abs() >= 1 {
            self.walking = true;
            self.still_ticks = 0;
            if dx < 0 {
                self.facing_left = true;
            } else if dx > 0 {
                self.facing_left = false;
            }
        } else {
            self.still_ticks = self.still_ticks.saturating_add(1);
            if self.still_ticks > 3 {
                self.walking = false;
            }
        }

        // Choose the pose (compute into locals first to avoid overlapping borrows).
        let head: String;
        let torso: String;
        let leg: String;
        if self.walking {
            self.bob = 0;
            self.flip = false; // classic frames are already directional
            self.walk_phase = self.walk_phase.wrapping_add(1);
            // Real walk: play the classic 8-frame directional cycle as a full
            // body on the torso slot, and hide the modular head/legs so only
            // that frame shows. ~7 ticks/frame reads as a relaxed amble.
            let dir = if self.facing_left { 'l' } else { 'r' };
            let n = (self.walk_phase / 3) % 8;
            head = "without_head".to_string();
            torso = format!("walk{dir}{n}");
            leg = "without_leg".to_string();
        } else {
            // Idle: face forward (never mirrored), gentle breathing bob.
            self.flip = false;
            const BOB: [i32; 8] = [0, 0, -1, -1, -2, -1, -1, 0];
            self.bob = BOB[((t / 3) % 8) as usize];
            // Blink/talk head swaps need their own sliced heads; until those
            // exist in the sheet set, hold the neutral front head so a missing
            // variant can never blank the face.
            head = "front".to_string();
            torso = "idle_front".to_string();
            leg = "stand".to_string();
        }
        if let Some(st) = self.modular_state.as_mut() {
            st.variants.insert("head_base".to_string(), head);
            st.variants.insert("torso".to_string(), torso);
            st.variants.insert("left_leg".to_string(), leg);
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
    modular: Option<ModularScene>,
    host: Arc<dyn Host>,
) -> Result<Spawned, String> {
    let shared = Arc::new(Shared {
        pending: Mutex::new(None),
        queued: AtomicBool::new(false),
        x: AtomicI32::new(0),
        y: AtomicI32::new(0),
        dpi: AtomicU32::new(96),
        unit_milli: AtomicU32::new(1000),
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

            // In modular mode the character is authored on the 320×320 canvas,
            // so the window is sized to that and starts in the canonical pose.
            let modular_state = modular
                .as_ref()
                .map(|s| CharacterState::canonical_yasser(&s.definition));
            let mut stage = config.stage;
            let mut scale = config.scale;
            if modular.is_some() {
                stage.width = 320;
                stage.height = 320;
                // The sheet-sliced character is ~300 art units tall — about the
                // same as the classic 292px sprite — so the classic scale keeps
                // him the same on-screen size. Override with DESKFOLK_MODULAR_SCALE.
                scale = std::env::var("DESKFOLK_MODULAR_SCALE")
                    .ok()
                    .and_then(|s| s.parse::<f64>().ok())
                    .unwrap_or(scale);
            }

            let mut state = Box::new(WindowState {
                shared: thread_shared,
                sprites,
                surface: None,
                text: None,
                stage,
                portal: config.portal,
                scale,
                name: config.name.clone(),
                pixel_snap: config.pixel_snap,
                unit: scale,
                size: (0, 0),
                drag: None,
                shown: false,
                last: None,
                modular,
                modular_state,
                anim_tick: 0,
                bob: 0,
                blink_ticks: 0,
                spk_speaking: false,
                spk_level: 0,
                facing_left: false,
                flip: false,
                walking: false,
                walk_phase: 0,
                last_x: 0,
                still_ticks: 99,
            });
            state.resize_for_dpi(hwnd);
            let (x, y) = default_position(state.size.0, state.size.1);
            state.shared.x.store(x, Ordering::Relaxed);
            state.shared.y.store(y, Ordering::Relaxed);
            let animate = state.modular.is_some();

            unsafe {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(state) as isize);
                // Modular characters animate on their own clock, since engine
                // frames stop arriving when he's idle.
                if animate {
                    SetTimer(hwnd, ANIM_TIMER_ID, ANIM_MS, None);
                }
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
                    // Feed engine signals to the modular animation driver.
                    state.spk_speaking = frame.speaking;
                    state.spk_level = frame.level;
                    state.last = Some(frame);
                }
                // The modular path repaints on its own animation timer; the
                // classic path repaints once per delivered frame.
                if state.modular.is_none() {
                    state.repaint(hwnd);
                }
            }
            return 0;
        }

        WM_TIMER => {
            if wparam == ANIM_TIMER_ID {
                if let Some(state) = state_of(hwnd) {
                    state.tick_anim();
                    state.repaint(hwnd);
                }
                return 0;
            }
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
                    pressed: std::time::Instant::now(),
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
                    // Holding a still hand on him for a beat is petting, not
                    // clicking — a different gesture with a different meaning.
                    Some(d) if d.pressed.elapsed() >= std::time::Duration::from_millis(900) => {
                        state.shared.host.on_pet()
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
pub(crate) fn ledges_excluding(hwnd: isize, headroom: i32) -> Vec<crate::ledges::Ledge> {
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
        headroom,
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
