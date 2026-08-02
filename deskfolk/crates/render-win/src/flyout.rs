//! The companion's own right-click menu.
//!
//! A second layered window, drawn by the same compositor that draws him: a
//! dark panel in his palette that rises into place, staggers its rows in,
//! lights them under the cursor, and expands the device lists inline rather
//! than throwing a second panel across the desktop.
//!
//! It is a menu with no system chrome in it at all — which is the same reason
//! the companion stopped being a webview. A `TrackPopupMenu` with Segoe UI on
//! a grey slab was the last piece of Windows visibly bolted onto a character
//! who is meant to be standing on the desktop, not running in it.
//!
//! Everything is drawn from primitives — rounded rects, lines, glyph coverage
//! — so it stays sharp at any DPI and a character package does not have to
//! ship menu artwork to get a menu that looks deliberate. Icons are line art
//! for the same reason.

use std::f32::consts::PI;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Once};
use std::time::Instant;

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture, SetFocus};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetCursorPos, GetWindowLongPtrW, KillTimer,
    LoadCursorW, RegisterClassW, SetForegroundWindow, SetTimer, SetWindowLongPtrW, SetWindowPos,
    ShowWindow, SystemParametersInfoW, GWLP_USERDATA, HWND_TOPMOST, IDC_ARROW, SPI_GETWORKAREA,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SW_SHOWNOACTIVATE, WM_CAPTURECHANGED, WM_DESTROY,
    WM_KEYDOWN, WM_KILLFOCUS, WM_LBUTTONUP, WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_RBUTTONUP,
    WM_TIMER, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_POPUP,
};

use crate::canvas::{rgba, Canvas, Px};
use crate::menu::{flatten, Icon, MenuEntry, Row, RowKind};
use crate::surface::Surface;
use crate::text::{wide, TextRenderer};
use crate::Host;

const CLASS_NAME: &str = "DeskfolkMenu";
static REGISTER: Once = Once::new();
/// One menu at a time. Everything here runs on the window thread, so a plain
/// flag is enough and a second right-click simply replaces the first.
static OPEN: AtomicBool = AtomicBool::new(false);

const TIMER_ID: usize = 1;
const VK_ESCAPE: usize = 0x1B;

// -- palette ----------------------------------------------------------------
// His colours, not the system's: the same warm dark and amber the speech
// bubble uses, so the menu reads as part of the same object.

const PANEL_FILL: Px = rgba(18, 15, 13, 246);
const PANEL_EDGE: Px = rgba(240, 160, 48, 76);
const HEADER_TEXT: Px = rgba(240, 160, 48, 215);
const ROW_TEXT: Px = rgba(233, 228, 218, 255);
const ROW_TEXT_DIM: Px = rgba(126, 118, 108, 255);
const HOVER_FILL: Px = rgba(240, 160, 48, 38);
const ACCENT: Px = rgba(240, 160, 48, 255);
const SEPARATOR: Px = rgba(255, 255, 255, 20);

// -- animation --------------------------------------------------------------

/// Seconds. Short enough to feel immediate, long enough to read as motion.
const OPEN_SECS: f32 = 0.17;
const CLOSE_SECS: f32 = 0.11;
const HOVER_SECS: f32 = 0.11;
const EXPAND_SECS: f32 = 0.19;
/// How far apart the rows arrive, as a fraction of the open animation.
const STAGGER: f32 = 0.055;

fn ease_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

/// Move `current` toward `target` at a rate that covers 0..1 in `secs`.
fn approach(current: f32, target: f32, dt: f32, secs: f32) -> f32 {
    let step = if secs <= 0.0 { 1.0 } else { dt / secs };
    if current < target {
        (current + step).min(target)
    } else {
        (current - step).max(target)
    }
}

/// How far into its entrance row `i` is.
///
/// Each row's remaining time is normalised against its own delay, so *every*
/// row reaches 1.0 exactly when the panel is open. Dividing by a fixed span
/// instead leaves the rows near the bottom of a long menu — the device lists —
/// permanently dimmed and offset, having never finished arriving. The delay is
/// capped for the same reason: a fifteen-row menu should not spend most of its
/// entrance waiting.
fn reveal_of(open_t: f32, i: usize) -> f32 {
    let delay = (i as f32 * STAGGER).min(0.5);
    ease_out((open_t - delay) / (1.0 - delay))
}

// -- layout -----------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub struct Metrics {
    pub row_h: f32,
    pub sep_h: f32,
    pub pad: f32,
    pub header_h: f32,
}

impl Metrics {
    fn new(unit: f64) -> Self {
        let u = unit as f32;
        Self {
            row_h: 31.0 * u,
            sep_h: 9.0 * u,
            pad: 7.0 * u,
            header_h: 25.0 * u,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct RowAnim {
    hover: f32,
    expand: f32,
    expand_target: f32,
}

/// Lay the rows out top to bottom, scaling each child's height by how far its
/// parent has expanded. Returns each row's `(y, height)` and the panel height.
///
/// Kept free of Win32 so the growth behaviour — the part that is easy to get
/// subtly wrong — can be tested directly.
fn layout(rows: &[Row], anim: &[RowAnim], m: &Metrics) -> (Vec<(f32, f32)>, f32) {
    let mut out = Vec::with_capacity(rows.len());
    let mut y = m.pad + m.header_h;
    for (i, row) in rows.iter().enumerate() {
        let vis = match row.parent {
            None => 1.0,
            Some(p) => anim.get(p).map(|a| a.expand).unwrap_or(0.0),
        };
        let base = match row.kind {
            RowKind::Separator => m.sep_h,
            _ => m.row_h,
        };
        let h = base * vis.clamp(0.0, 1.0);
        out.push((y, h));
        y += h;
        let _ = i;
    }
    (out, y + m.pad)
}

/// Which row is under `py`, in panel-local pixels.
fn row_at(rows: &[Row], boxes: &[(f32, f32)], py: f32) -> Option<usize> {
    for (i, row) in rows.iter().enumerate() {
        let (y, h) = boxes[i];
        // A row mid-collapse is too small to aim at; ignore it rather than
        // letting a 2px sliver swallow the click.
        if h > 6.0 && py >= y && py < y + h && row.selectable() {
            return Some(i);
        }
    }
    None
}

// -- the window -------------------------------------------------------------

struct Flyout {
    rows: Vec<Row>,
    anim: Vec<RowAnim>,
    /// Cached glyph coverage per row: rendering text every frame would put GDI
    /// in the middle of a 60Hz animation for labels that never change.
    text: Vec<Option<(Vec<u8>, i32, i32)>>,
    header: Option<(Vec<u8>, i32, i32)>,
    metrics: Metrics,
    unit: f32,
    open_t: f32,
    closing: bool,
    hover: Option<usize>,
    surface: Option<Surface>,
    width: i32,
    max_height: i32,
    /// Panels that open upward grow from a fixed bottom edge.
    flip_up: bool,
    origin: (i32, i32),
    last_tick: Instant,
    chosen: Option<String>,
    host: Arc<dyn Host>,
}

/// Is a menu on screen right now?
///
/// The companion re-asserts always-on-top on a slow tick, and doing that while
/// the menu is up puts *him* at the top of the topmost band — on top of his own
/// menu. So the raise stands down while this is true.
pub(crate) fn is_open() -> bool {
    OPEN.load(Ordering::SeqCst)
}

/// Open the menu at a screen position.
pub(crate) fn open(entries: &[MenuEntry], name: &str, x: i32, y: i32, host: Arc<dyn Host>) {
    if entries.is_empty() {
        return;
    }
    // A second right-click while one is up: the old one goes away first.
    if OPEN.swap(true, Ordering::SeqCst) {
        return;
    }
    if let Err(e) = create(entries, name, x, y, host) {
        OPEN.store(false, Ordering::SeqCst);
        tracing::warn!("menu: {e}");
    }
}

fn create(
    entries: &[MenuEntry],
    name: &str,
    x: i32,
    y: i32,
    host: Arc<dyn Host>,
) -> Result<(), String> {
    let class = wide(CLASS_NAME);
    REGISTER.call_once(|| unsafe {
        let mut wc: WNDCLASSW = std::mem::zeroed();
        wc.lpfnWndProc = Some(wndproc);
        wc.hInstance = GetModuleHandleW(std::ptr::null());
        wc.lpszClassName = class.as_ptr();
        wc.hCursor = LoadCursorW(std::ptr::null_mut(), IDC_ARROW);
        RegisterClassW(&wc);
    });

    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOREDIRECTIONBITMAP,
            class.as_ptr(),
            wide("Deskfolk menu").as_ptr(),
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
        return Err("CreateWindowExW returned null for the menu".into());
    }

    let dpi = unsafe { GetDpiForWindow(hwnd) };
    let unit = if dpi == 0 { 1.0 } else { dpi as f32 / 96.0 };
    let metrics = Metrics::new(unit as f64);

    let rows = flatten(entries);
    let anim = vec![RowAnim::default(); rows.len()];

    // Measure everything up front: the panel's width is fixed for its whole
    // life, because a menu that resizes as you open a submenu is a menu that
    // moves the row you were about to click.
    let mut row_text = TextRenderer::new((13.0 * unit).round() as i32)
        .ok_or_else(|| "no text renderer for the menu".to_string())?;
    let mut header_text = TextRenderer::new((10.0 * unit).round() as i32)
        .ok_or_else(|| "no header renderer for the menu".to_string())?;

    let text_left = metrics.pad + 18.0 * unit + 10.0 * unit;
    let gutter = 30.0 * unit;
    let max_label = (330.0 * unit - text_left - gutter) as i32;

    let mut widest = 0i32;
    let mut text = Vec::with_capacity(rows.len());
    for row in &rows {
        if matches!(row.kind, RowKind::Separator) {
            text.push(None);
            continue;
        }
        let (w, h) = row_text.measure(&row.label, max_label);
        widest = widest.max(w);
        // DT_LEFT | DT_SINGLELINE | DT_NOPREFIX
        text.push(row_text.coverage(&row.label, w.max(1), h.max(1), 0x20 | 0x800).map(|c| (c, w, h)));
    }
    let header = {
        let label = name.to_uppercase();
        let (w, h) = header_text.measure(&label, max_label);
        widest = widest.max(w);
        header_text.coverage(&label, w.max(1), h.max(1), 0x20 | 0x800).map(|c| (c, w, h))
    };

    let width = ((text_left + widest as f32 + gutter).clamp(190.0 * unit, 340.0 * unit)).round() as i32;

    let mut flyout = Box::new(Flyout {
        rows,
        anim,
        text,
        header,
        metrics,
        unit,
        open_t: 0.0,
        closing: false,
        hover: None,
        surface: None,
        width,
        max_height: 0,
        flip_up: false,
        origin: (x, y),
        last_tick: Instant::now(),
        chosen: None,
        host,
    });

    // Full height with every submenu open, so the window never has to resize
    // and an upward-opening panel has a stable bottom edge to grow from.
    let full: Vec<RowAnim> = flyout
        .rows
        .iter()
        .map(|_| RowAnim { hover: 0.0, expand: 1.0, expand_target: 1.0 })
        .collect();
    let (_, max_h) = layout(&flyout.rows, &full, &flyout.metrics);
    flyout.max_height = max_h.ceil() as i32;

    let area = work_area();
    if x + width > area.right {
        flyout.origin.0 = (x - width).max(area.left);
    }
    if y + flyout.max_height > area.bottom {
        flyout.origin.1 = (y - flyout.max_height).max(area.top);
        flyout.flip_up = true;
    }

    flyout.surface = Surface::new(width, flyout.max_height);
    if flyout.surface.is_none() {
        unsafe { DestroyWindow(hwnd) };
        return Err("could not allocate the menu surface".into());
    }

    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(flyout) as isize);
        // Capture means a click anywhere — including outside the panel —
        // comes to us, which is how the menu dismisses itself. Foreground is
        // what makes Escape work.
        SetForegroundWindow(hwnd);
        SetFocus(hwnd);
        SetCapture(hwnd);
        SetTimer(hwnd, TIMER_ID, 16, None);
        ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        // Explicitly to the top of the topmost band. The companion is topmost
        // too, and whichever of the two asked most recently wins — so the menu
        // has to ask *after* it exists, or it opens behind the character.
        SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
    }
    if let Some(state) = unsafe { state_of(hwnd) } {
        state.paint(hwnd);
    }
    Ok(())
}

fn work_area() -> RECT {
    let mut area = RECT { left: 0, top: 0, right: 1920, bottom: 1080 };
    unsafe {
        SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            &mut area as *mut RECT as *mut std::ffi::c_void,
            0,
        );
    }
    area
}

impl Flyout {
    /// Advance every animation. Returns false once everything has settled and
    /// the timer can stop — an open menu sitting still should cost nothing.
    fn tick(&mut self) -> bool {
        let now = Instant::now();
        let dt = (now - self.last_tick).as_secs_f32().min(0.1);
        self.last_tick = now;

        let mut busy = false;
        let target = if self.closing { 0.0 } else { 1.0 };
        let secs = if self.closing { CLOSE_SECS } else { OPEN_SECS };
        let before = self.open_t;
        self.open_t = approach(self.open_t, target, dt, secs);
        busy |= self.open_t != before || self.open_t != target;

        for (i, a) in self.anim.iter_mut().enumerate() {
            let want = if self.hover == Some(i) { 1.0 } else { 0.0 };
            let h = approach(a.hover, want, dt, HOVER_SECS);
            busy |= h != a.hover;
            a.hover = h;

            let e = approach(a.expand, a.expand_target, dt, EXPAND_SECS);
            busy |= e != a.expand;
            a.expand = e;
        }
        busy
    }

    fn finished_closing(&self) -> bool {
        self.closing && self.open_t <= 0.0
    }

    fn begin_close(&mut self, chosen: Option<String>) {
        if self.closing {
            return;
        }
        self.chosen = chosen;
        self.closing = true;
        self.hover = None;
    }

    fn to_panel(&self, screen_x: i32, screen_y: i32) -> (f32, f32) {
        let (_, oy) = self.panel_offset();
        (
            (screen_x - self.origin.0) as f32,
            (screen_y - self.origin.1) as f32 - oy,
        )
    }

    /// Where the panel sits inside the window: it slides up into place, and
    /// an upward-opening menu is bottom-aligned so it grows away from the
    /// cursor rather than under it.
    fn panel_offset(&self) -> (f32, f32) {
        let e = ease_out(self.open_t);
        let (_, panel_h) = layout(&self.rows, &self.anim, &self.metrics);
        let base = if self.flip_up {
            self.max_height as f32 - panel_h
        } else {
            0.0
        };
        (0.0, base + (1.0 - e) * 10.0 * self.unit)
    }

    fn on_move(&mut self, screen_x: i32, screen_y: i32) {
        if self.closing {
            return;
        }
        let (px, py) = self.to_panel(screen_x, screen_y);
        let (boxes, panel_h) = layout(&self.rows, &self.anim, &self.metrics);
        let inside = px >= 0.0 && px < self.width as f32 && py >= 0.0 && py < panel_h;
        self.hover = if inside { row_at(&self.rows, &boxes, py) } else { None };
    }

    fn on_click(&mut self, screen_x: i32, screen_y: i32) {
        if self.closing {
            return;
        }
        let (px, py) = self.to_panel(screen_x, screen_y);
        let (boxes, panel_h) = layout(&self.rows, &self.anim, &self.metrics);
        if px < 0.0 || px >= self.width as f32 || py < 0.0 || py >= panel_h {
            // Outside the panel: dismiss, exactly like any other menu.
            self.begin_close(None);
            return;
        }
        let Some(i) = row_at(&self.rows, &boxes, py) else { return };
        match self.rows[i].kind {
            RowKind::Submenu => {
                let open = self.anim[i].expand_target > 0.5;
                // Only one list open at a time: two expanded device lists is
                // taller than the screen and harder to read than either alone.
                for (j, a) in self.anim.iter_mut().enumerate() {
                    if matches!(self.rows[j].kind, RowKind::Submenu) {
                        a.expand_target = 0.0;
                    }
                }
                self.anim[i].expand_target = if open { 0.0 } else { 1.0 };
            }
            RowKind::Item { .. } => {
                let id = self.rows[i].id.clone();
                self.begin_close(Some(id));
            }
            RowKind::Separator => {}
        }
    }

    fn paint(&mut self, hwnd: HWND) {
        let Some(surface) = self.surface.as_mut() else { return };
        let (boxes, panel_h) = layout(&self.rows, &self.anim, &self.metrics);
        let e = ease_out(self.open_t);
        let (_, oy) = {
            let base = if self.flip_up {
                self.max_height as f32 - panel_h
            } else {
                0.0
            };
            (0.0f32, base + (1.0 - e) * 10.0 * self.unit)
        };

        let u = self.unit;
        let w = self.width as f32;
        let m = self.metrics;
        let alpha = (e * 255.0) as u8;

        let mut canvas = surface.canvas();
        canvas.clear();

        // Panel
        let radius = 12.0 * u;
        canvas.fill_round_rect(0.0, oy, w, panel_h, radius, crate::canvas::scale_alpha(PANEL_FILL, alpha));
        canvas.stroke_round_rect(
            0.5,
            oy + 0.5,
            w - 1.0,
            panel_h - 1.0,
            radius,
            1.0 * u,
            crate::canvas::scale_alpha(PANEL_EDGE, alpha),
        );

        // Header: his name, so the menu is plainly *his* and not the app's.
        if let Some((cov, cw, ch)) = &self.header {
            let x = m.pad + 8.0 * u;
            let y = oy + m.pad + (m.header_h - *ch as f32) / 2.0;
            canvas.blit_coverage(
                cov,
                *cw,
                *ch,
                x.round() as i32,
                y.round() as i32,
                crate::canvas::scale_alpha(HEADER_TEXT, alpha),
            );
            // A short rule under the name, drawn to the width of the text.
            canvas.stroke_line(
                x,
                y + *ch as f32 + 3.0 * u,
                x + *cw as f32,
                y + *ch as f32 + 3.0 * u,
                1.0 * u,
                crate::canvas::scale_alpha(ACCENT, (alpha as f32 * 0.45) as u8),
            );
        }

        let text_left = m.pad + 18.0 * u + 10.0 * u;

        for (i, row) in self.rows.iter().enumerate() {
            let (ry, rh) = boxes[i];
            if rh <= 0.5 {
                continue;
            }
            let ry = ry + oy;
            let reveal = reveal_of(self.open_t, i);
            if reveal <= 0.0 {
                continue;
            }
            let row_alpha = (reveal * 255.0) as u8;
            // Rows arrive from slightly right of where they land.
            let slide = (1.0 - reveal) * 9.0 * u;

            if matches!(row.kind, RowKind::Separator) {
                let y = ry + rh / 2.0;
                canvas.stroke_line(
                    m.pad + 6.0 * u,
                    y,
                    w - m.pad - 6.0 * u,
                    y,
                    1.0 * u,
                    crate::canvas::scale_alpha(SEPARATOR, row_alpha),
                );
                continue;
            }

            let hover = self.anim[i].hover;
            let indent = if row.parent.is_some() { 12.0 * u } else { 0.0 };

            if hover > 0.0 {
                canvas.fill_round_rect(
                    m.pad * 0.6 + indent,
                    ry + 1.0 * u,
                    w - m.pad * 1.2 - indent,
                    rh - 2.0 * u,
                    8.0 * u,
                    crate::canvas::scale_alpha(
                        HOVER_FILL,
                        (hover * reveal * 255.0) as u8,
                    ),
                );
                // An accent bar that grows out of the left edge — the one
                // piece of motion that tracks the cursor rather than the
                // panel's own entrance.
                let bar_h = rh * (0.35 + 0.35 * hover);
                canvas.fill_round_rect(
                    m.pad * 0.6 + indent + 1.0 * u,
                    ry + (rh - bar_h) / 2.0,
                    2.5 * u,
                    bar_h,
                    1.5 * u,
                    crate::canvas::scale_alpha(ACCENT, (hover * reveal * 255.0) as u8),
                );
            }

            let color = if row.enabled { ROW_TEXT } else { ROW_TEXT_DIM };
            let color = crate::canvas::scale_alpha(color, row_alpha);

            if row.icon != Icon::None {
                draw_icon(
                    &mut canvas,
                    row.icon,
                    m.pad + 9.0 * u + indent + slide,
                    ry + rh / 2.0,
                    16.0 * u,
                    crate::canvas::scale_alpha(
                        if row.enabled { ACCENT } else { ROW_TEXT_DIM },
                        (row_alpha as f32 * (0.72 + 0.28 * hover)) as u8,
                    ),
                );
            }

            if let Some((cov, cw, ch)) = &self.text[i] {
                let x = text_left + indent + slide;
                let y = ry + (rh - *ch as f32) / 2.0;
                canvas.blit_coverage(cov, *cw, *ch, x.round() as i32, y.round() as i32, color);
            }

            match row.kind {
                RowKind::Submenu => {
                    // The chevron rotates from ">" to "v" as the list opens.
                    let angle = self.anim[i].expand * PI / 2.0;
                    draw_chevron(
                        &mut canvas,
                        w - m.pad - 12.0 * u,
                        ry + rh / 2.0,
                        4.5 * u,
                        angle,
                        1.6 * u,
                        crate::canvas::scale_alpha(ACCENT, (row_alpha as f32 * 0.8) as u8),
                    );
                }
                RowKind::Item { checked: true } => {
                    draw_check(
                        &mut canvas,
                        w - m.pad - 14.0 * u,
                        ry + rh / 2.0,
                        5.0 * u,
                        1.8 * u,
                        crate::canvas::scale_alpha(ACCENT, row_alpha),
                    );
                }
                _ => {}
            }
        }

        surface.present(hwnd, self.origin.0, self.origin.1);
    }
}

// -- glyphs -----------------------------------------------------------------

fn draw_chevron(c: &mut Canvas<'_>, cx: f32, cy: f32, r: f32, angle: f32, t: f32, color: Px) {
    let tip = (cx + r * angle.cos(), cy + r * angle.sin());
    let a = (cx + r * (angle + 2.36).cos(), cy + r * (angle + 2.36).sin());
    let b = (cx + r * (angle - 2.36).cos(), cy + r * (angle - 2.36).sin());
    c.stroke_line(a.0, a.1, tip.0, tip.1, t, color);
    c.stroke_line(b.0, b.1, tip.0, tip.1, t, color);
}

fn draw_check(c: &mut Canvas<'_>, cx: f32, cy: f32, r: f32, t: f32, color: Px) {
    c.stroke_line(cx - r, cy, cx - r * 0.25, cy + r * 0.7, t, color);
    c.stroke_line(cx - r * 0.25, cy + r * 0.7, cx + r, cy - r * 0.7, t, color);
}

/// Line-art icons, sized to `size` and centred on `(cx, cy)`.
fn draw_icon(c: &mut Canvas<'_>, icon: Icon, cx: f32, cy: f32, size: f32, color: Px) {
    let s = size / 2.0;
    let t = (size / 9.0).max(1.0);
    match icon {
        Icon::None => {}
        Icon::Talk => {
            // A speech bubble with a tail — the same shape he talks in.
            c.stroke_round_rect(cx - s, cy - s * 0.85, size, size * 0.8, s * 0.45, t, color);
            c.stroke_line(cx - s * 0.35, cy - s * 0.05, cx - s * 0.6, cy + s * 0.75, t, color);
        }
        Icon::Mic => {
            c.fill_round_rect(cx - s * 0.32, cy - s, s * 0.64, size * 0.5, s * 0.32, color);
            c.stroke_line(cx - s * 0.62, cy + s * 0.05, cx - s * 0.62, cy + s * 0.3, t, color);
            c.stroke_line(cx + s * 0.62, cy + s * 0.05, cx + s * 0.62, cy + s * 0.3, t, color);
            c.stroke_line(cx - s * 0.62, cy + s * 0.3, cx + s * 0.62, cy + s * 0.3, t, color);
            c.stroke_line(cx, cy + s * 0.3, cx, cy + s * 0.85, t, color);
        }
        Icon::Speaker => {
            c.fill_round_rect(cx - s, cy - s * 0.35, s * 0.55, s * 0.7, t * 0.4, color);
            c.stroke_line(cx - s * 0.45, cy - s * 0.35, cx + s * 0.05, cy - s * 0.85, t, color);
            c.stroke_line(cx - s * 0.45, cy + s * 0.35, cx + s * 0.05, cy + s * 0.85, t, color);
            c.stroke_line(cx + s * 0.05, cy - s * 0.85, cx + s * 0.05, cy + s * 0.85, t, color);
            c.stroke_line(cx + s * 0.5, cy - s * 0.35, cx + s * 0.75, cy - s * 0.15, t, color);
            c.stroke_line(cx + s * 0.5, cy + s * 0.35, cx + s * 0.75, cy + s * 0.15, t, color);
        }
        Icon::Sleep => {
            // Two z's, the smaller one drifting up and away.
            zed(c, cx - s * 0.15, cy + s * 0.15, s * 0.75, t, color);
            zed(c, cx + s * 0.6, cy - s * 0.55, s * 0.4, t, color);
        }
        Icon::Wake => {
            c.stroke_round_rect(cx - s * 0.4, cy - s * 0.4, s * 0.8, s * 0.8, s * 0.4, t, color);
            for k in 0..4 {
                let a = k as f32 * PI / 2.0 + PI / 4.0;
                c.stroke_line(
                    cx + a.cos() * s * 0.62,
                    cy + a.sin() * s * 0.62,
                    cx + a.cos() * s * 0.95,
                    cy + a.sin() * s * 0.95,
                    t,
                    color,
                );
            }
        }
        Icon::Panel => {
            c.stroke_round_rect(cx - s, cy - s * 0.8, size, size * 0.8, s * 0.25, t, color);
            c.stroke_line(cx - s, cy - s * 0.35, cx + s, cy - s * 0.35, t, color);
        }
        Icon::Quit => {
            c.stroke_line(cx - s * 0.7, cy - s * 0.7, cx + s * 0.7, cy + s * 0.7, t, color);
            c.stroke_line(cx + s * 0.7, cy - s * 0.7, cx - s * 0.7, cy + s * 0.7, t, color);
        }
    }
}

fn zed(c: &mut Canvas<'_>, cx: f32, cy: f32, r: f32, t: f32, color: Px) {
    c.stroke_line(cx - r, cy - r, cx + r, cy - r, t, color);
    c.stroke_line(cx + r, cy - r, cx - r, cy + r, t, color);
    c.stroke_line(cx - r, cy + r, cx + r, cy + r, t, color);
}

// -- plumbing ---------------------------------------------------------------

unsafe fn state_of<'a>(hwnd: HWND) -> Option<&'a mut Flyout> {
    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Flyout;
    if ptr.is_null() {
        None
    } else {
        Some(&mut *ptr)
    }
}

unsafe fn cursor() -> (i32, i32) {
    let mut p = POINT { x: 0, y: 0 };
    GetCursorPos(&mut p);
    (p.x, p.y)
}

unsafe extern "system" fn wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_MOUSEACTIVATE => return 3, /* MA_NOACTIVATE */

        WM_TIMER => {
            if let Some(state) = state_of(hwnd) {
                let busy = state.tick();
                state.paint(hwnd);
                if state.finished_closing() {
                    DestroyWindow(hwnd);
                } else if !busy {
                    // Settled: stop burning a timer on a menu sitting still.
                    KillTimer(hwnd, TIMER_ID);
                }
            }
            return 0;
        }

        WM_MOUSEMOVE => {
            if let Some(state) = state_of(hwnd) {
                let (x, y) = cursor();
                let before = state.hover;
                state.on_move(x, y);
                if state.hover != before {
                    SetTimer(hwnd, TIMER_ID, 16, None);
                }
            }
            return 0;
        }

        WM_LBUTTONUP | WM_RBUTTONUP => {
            if let Some(state) = state_of(hwnd) {
                let (x, y) = cursor();
                state.on_click(x, y);
                SetTimer(hwnd, TIMER_ID, 16, None);
            }
            return 0;
        }

        WM_KEYDOWN => {
            if wparam == VK_ESCAPE {
                if let Some(state) = state_of(hwnd) {
                    state.begin_close(None);
                    SetTimer(hwnd, TIMER_ID, 16, None);
                }
            }
            return 0;
        }

        // Losing capture or focus means something else took over — a menu
        // that outlives that is a menu stuck on the desktop.
        WM_CAPTURECHANGED | WM_KILLFOCUS => {
            if let Some(state) = state_of(hwnd) {
                if !state.closing {
                    state.begin_close(None);
                    SetTimer(hwnd, TIMER_ID, 16, None);
                }
            }
            return 0;
        }

        WM_DESTROY => {
            KillTimer(hwnd, TIMER_ID);
            ReleaseCapture();
            let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Flyout;
            if !ptr.is_null() {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                let state = Box::from_raw(ptr);
                // Act *after* the window is gone, so anything the action opens
                // is not fighting a dying menu for the foreground.
                if let Some(id) = &state.chosen {
                    state.host.on_menu(id);
                }
            }
            OPEN.store(false, Ordering::SeqCst);
            return 0;
        }
        _ => {}
    }
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu::MenuEntry;

    fn metrics() -> Metrics {
        Metrics { row_h: 30.0, sep_h: 10.0, pad: 8.0, header_h: 24.0 }
    }

    fn sample() -> Vec<MenuEntry> {
        vec![
            MenuEntry::item("talk", "Talk"),
            MenuEntry::Separator,
            MenuEntry::submenu(
                "Speakers",
                Icon::Speaker,
                vec![MenuEntry::item("out::a", "A"), MenuEntry::item("out::b", "B")],
            ),
            MenuEntry::item("quit", "Quit"),
        ]
    }

    #[test]
    fn a_collapsed_submenu_takes_up_no_height() {
        let rows = flatten(&sample());
        let anim = vec![RowAnim::default(); rows.len()];
        let (boxes, h) = layout(&rows, &anim, &metrics());
        // talk + separator + Speakers + quit, children collapsed to zero.
        assert_eq!(h, 8.0 + 24.0 + 30.0 + 10.0 + 30.0 + 30.0 + 8.0);
        let speakers = rows.iter().position(|r| r.label == "Speakers").unwrap();
        assert_eq!(boxes[speakers + 1].1, 0.0, "child has no height");
    }

    #[test]
    fn expanding_a_submenu_grows_the_panel_by_its_children() {
        let rows = flatten(&sample());
        let mut anim = vec![RowAnim::default(); rows.len()];
        let speakers = rows.iter().position(|r| r.label == "Speakers").unwrap();
        let (_, closed) = layout(&rows, &anim, &metrics());
        anim[speakers].expand = 1.0;
        let (_, open) = layout(&rows, &anim, &metrics());
        assert_eq!(open - closed, 60.0, "two 30px children");
    }

    #[test]
    fn a_half_expanded_submenu_is_half_as_tall() {
        // The whole point of animating height rather than snapping to it.
        let rows = flatten(&sample());
        let mut anim = vec![RowAnim::default(); rows.len()];
        let speakers = rows.iter().position(|r| r.label == "Speakers").unwrap();
        anim[speakers].expand = 0.5;
        let (boxes, _) = layout(&rows, &anim, &metrics());
        assert_eq!(boxes[speakers + 1].1, 15.0);
    }

    #[test]
    fn rows_below_an_expanding_submenu_move_down() {
        let rows = flatten(&sample());
        let mut anim = vec![RowAnim::default(); rows.len()];
        let quit = rows.iter().position(|r| r.id == "quit").unwrap();
        let (before, _) = layout(&rows, &anim, &metrics());
        let speakers = rows.iter().position(|r| r.label == "Speakers").unwrap();
        anim[speakers].expand = 1.0;
        let (after, _) = layout(&rows, &anim, &metrics());
        assert_eq!(after[quit].0 - before[quit].0, 60.0);
    }

    #[test]
    fn clicks_land_on_the_row_under_the_cursor() {
        let rows = flatten(&sample());
        let anim = vec![RowAnim::default(); rows.len()];
        let (boxes, _) = layout(&rows, &anim, &metrics());
        let talk = rows.iter().position(|r| r.id == "talk").unwrap();
        let mid = boxes[talk].0 + boxes[talk].1 / 2.0;
        assert_eq!(row_at(&rows, &boxes, mid), Some(talk));
    }

    #[test]
    fn a_separator_never_catches_a_click() {
        let rows = flatten(&sample());
        let anim = vec![RowAnim::default(); rows.len()];
        let (boxes, _) = layout(&rows, &anim, &metrics());
        let sep = rows.iter().position(|r| r.kind == RowKind::Separator).unwrap();
        let mid = boxes[sep].0 + boxes[sep].1 / 2.0;
        assert_eq!(row_at(&rows, &boxes, mid), None);
    }

    #[test]
    fn a_row_mid_collapse_is_too_small_to_click() {
        // Otherwise a 2px sliver of a closing list swallows the click meant
        // for whatever is sliding into that spot.
        let rows = flatten(&sample());
        let mut anim = vec![RowAnim::default(); rows.len()];
        let speakers = rows.iter().position(|r| r.label == "Speakers").unwrap();
        anim[speakers].expand = 0.05; // 1.5px children
        let (boxes, _) = layout(&rows, &anim, &metrics());
        let child = speakers + 1;
        let mid = boxes[child].0 + boxes[child].1 / 2.0;
        assert_ne!(row_at(&rows, &boxes, mid), Some(child));
    }

    #[test]
    fn clicking_past_the_last_row_selects_nothing() {
        let rows = flatten(&sample());
        let anim = vec![RowAnim::default(); rows.len()];
        let (boxes, h) = layout(&rows, &anim, &metrics());
        assert_eq!(row_at(&rows, &boxes, h + 50.0), None);
    }

    #[test]
    fn rows_arrive_in_order() {
        // The stagger is the whole character of the entrance; if later rows
        // were not behind earlier ones it would just be a fade.
        assert!(reveal_of(0.3, 0) > reveal_of(0.3, 3));
        assert_eq!(reveal_of(0.0, 5), 0.0, "nothing is visible at the start");
        assert_eq!(reveal_of(1.0, 0), 1.0, "everything has arrived at the end");
    }

    #[test]
    fn every_row_is_fully_revealed_once_the_panel_is_open() {
        for i in 0..12 {
            assert_eq!(reveal_of(1.0, i), 1.0, "row {i} never finished arriving");
        }
    }

    #[test]
    fn approach_lands_exactly_on_the_target() {
        // Floating-point drift here would leave the timer running forever on
        // a menu that has visibly stopped moving.
        let mut v = 0.0;
        for _ in 0..100 {
            v = approach(v, 1.0, 0.016, 0.17);
        }
        assert_eq!(v, 1.0);
        for _ in 0..100 {
            v = approach(v, 0.0, 0.016, 0.11);
        }
        assert_eq!(v, 0.0);
    }

    #[test]
    fn easing_is_bounded() {
        assert_eq!(ease_out(0.0), 0.0);
        assert_eq!(ease_out(1.0), 1.0);
        assert_eq!(ease_out(-5.0), 0.0);
        assert_eq!(ease_out(5.0), 1.0);
    }
}
