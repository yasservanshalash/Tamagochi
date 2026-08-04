//! The companion's own right-click menu: cards thrown out from his shoulder.
//!
//! Not a panel and deliberately not a list. Right-click and a stack of leaning
//! chips flies out of him — to his right if he is standing on the left of the
//! screen, to his left if he is on the right — bowing outward in the middle,
//! each one arriving a beat after the last and overshooting slightly before it
//! settles. Hovering one snaps it further out and inverts it to solid amber.
//! Opening a device list throws the whole stack back into him and deals a new
//! one, with a BACK card at the bottom.
//!
//! It is drawn by the same compositor that draws him, on a second layered
//! window, which is the point: a `TrackPopupMenu` was the last piece of Windows
//! visibly bolted onto a character who exists because we refused the system's
//! window chrome.
//!
//! Everything is primitives — skewed quads, lines, glyph coverage sheared to
//! match the lean — so it stays sharp at any DPI and a character package needs
//! no menu artwork at all.

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

use crate::canvas::{rgba, scale_alpha, Canvas, Px};
use crate::menu::{flatten, Icon, MenuEntry, Row, RowKind};
use crate::surface::Surface;
use crate::text::{wide, TextRenderer};
use crate::Host;

const CLASS_NAME: &str = "DeskfolkMenu";
static REGISTER: Once = Once::new();
static OPEN: AtomicBool = AtomicBool::new(false);

const TIMER_ID: usize = 1;
const VK_ESCAPE: usize = 0x1B;
/// The BACK card, which has no row of its own.
const BACK: usize = usize::MAX;

// -- palette ----------------------------------------------------------------

const CARD_FILL: Px = rgba(20, 17, 14, 242);
const CARD_EDGE: Px = rgba(240, 160, 48, 150);
const CARD_SHADOW: Px = rgba(0, 0, 0, 120);
const CARD_TEXT: Px = rgba(236, 231, 221, 255);
const CARD_TEXT_DIM: Px = rgba(128, 120, 110, 255);
/// Hover inverts the card: solid amber, dark text. Loud on purpose.
const HOT_FILL: Px = rgba(240, 160, 48, 250);
const HOT_TEXT: Px = rgba(24, 18, 10, 255);
const TITLE_TEXT: Px = rgba(240, 160, 48, 240);

// -- motion -----------------------------------------------------------------

const DEAL_SECS: f32 = 0.30;
const CLOSE_SECS: f32 = 0.14;
const HOVER_SECS: f32 = 0.09;
/// Gap between one card arriving and the next, as a fraction of the deal.
const STAGGER: f32 = 0.07;
/// Longest a card will wait before starting, so a ten-device list still snaps.
const MAX_DELAY: f32 = 0.55;

fn ease_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

/// Overshoot-and-settle. The cards fly slightly past their slot and come back,
/// which is what makes them feel thrown rather than placed.
fn back_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    // Pinned at the ends: the polynomial is only *algebraically* 0 and 1 there,
    // and in f32 it lands a whisker off — enough to leave a settled card a
    // fraction of a pixel from its slot and the animation never quite still.
    if t <= 0.0 {
        return 0.0;
    }
    if t >= 1.0 {
        return 1.0;
    }
    const C1: f32 = 1.9;
    const C3: f32 = C1 + 1.0;
    1.0 + C3 * (t - 1.0).powi(3) + C1 * (t - 1.0).powi(2)
}

fn approach(current: f32, target: f32, dt: f32, secs: f32) -> f32 {
    let step = if secs <= 0.0 { 1.0 } else { dt / secs };
    if current < target {
        (current + step).min(target)
    } else {
        (current - step).max(target)
    }
}

/// How far into its entrance card `i` is.
///
/// Normalised against its own delay so *every* card reaches 1.0 when the deal
/// finishes. Dividing by a fixed span instead leaves the last cards of a long
/// device list permanently short of their slot, having never arrived.
fn deal_of(open_t: f32, i: usize) -> f32 {
    let delay = (i as f32 * STAGGER).min(MAX_DELAY);
    ((open_t - delay) / (1.0 - delay)).clamp(0.0, 1.0)
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Side {
    Left,
    Right,
}

#[derive(Debug, Clone, Copy)]
struct Slot {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

impl Slot {
    /// Is `(px, py)` inside this card, given its lean?
    fn contains(&self, px: f32, py: f32, skew: f32) -> bool {
        if py < self.y || py >= self.y + self.h {
            return false;
        }
        let frac = (py - self.y) / self.h;
        let left = self.x + skew * (1.0 - frac);
        px >= left && px < left + self.w
    }
}

struct Card {
    row: usize,
    hover: f32,
}

// -- layout -----------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
struct Metrics {
    card_w: f32,
    card_h: f32,
    spacing: f32,
    skew: f32,
    arc: f32,
    margin: f32,
    title_h: f32,
}

/// Where each card rests, bowed outward through the middle of the stack.
///
/// The bow is the difference between "a column of buttons" and "a hand of
/// cards fanned out at you" — it costs one sine and reads immediately.
fn slots(n: usize, m: &Metrics, side: Side, width: f32, height: f32) -> Vec<Slot> {
    let mut out = Vec::with_capacity(n);
    if n == 0 {
        return out;
    }
    // A menu built from a package's own contents has no fixed length, and a
    // card laid out past the bottom edge is one nobody can click. A list that
    // fits keeps its normal spacing and sits centred; one that does not is
    // spread from under the title to the bottom edge, overlapping as much as
    // it must. Overlapping cards are ugly; unreachable ones are broken.
    let room = (height - m.title_h).max(m.card_h);
    let natural = n as f32 * m.spacing;
    let ys: Vec<f32> = if natural <= room {
        let top = m.title_h + (room - natural) / 2.0;
        (0..n)
            .map(|i| top + i as f32 * m.spacing + (m.spacing - m.card_h) / 2.0)
            .collect()
    } else {
        let step = if n > 1 { (room - m.card_h) / (n - 1) as f32 } else { 0.0 };
        (0..n).map(|i| m.title_h + i as f32 * step).collect()
    };
    for i in 0..n {
        let bow = (PI * (i as f32 + 0.5) / n as f32).sin() * m.arc;
        let x = match side {
            // Away from the character is rightward when he is to our left.
            Side::Right => m.margin + bow,
            Side::Left => width - m.card_w - m.margin - m.skew - bow,
        };
        out.push(Slot {
            x,
            y: ys[i],
            w: m.card_w,
            h: m.card_h,
        });
    }
    out
}

/// Where a card actually is right now: flown from the anchor toward its slot,
/// and pushed a little further out while hovered.
fn placement(slot: &Slot, anchor: (f32, f32), t: f32, hover: f32, side: Side) -> Slot {
    let e = back_out(t);
    let lift = hover * 10.0;
    let dir = if side == Side::Right { 1.0 } else { -1.0 };
    Slot {
        x: anchor.0 + (slot.x - anchor.0) * e + lift * dir,
        y: anchor.1 + (slot.y - anchor.1) * e,
        w: slot.w,
        h: slot.h,
    }
}

// -- the window -------------------------------------------------------------

struct Flyout {
    rows: Vec<Row>,
    /// Cached glyph coverage per row; re-rendering text inside a 60Hz
    /// animation would put GDI in the middle of every frame.
    text: Vec<Option<(Vec<u8>, i32, i32)>>,
    back_text: Option<(Vec<u8>, i32, i32)>,
    titles: Vec<(String, Option<(Vec<u8>, i32, i32)>)>,

    /// `None` at the top level, `Some(row)` inside that submenu.
    level: Option<usize>,
    cards: Vec<Card>,

    metrics: Metrics,
    unit: f32,
    side: Side,
    width: i32,
    height: i32,
    origin: (i32, i32),
    anchor: (f32, f32),

    open_t: f32,
    closing: bool,
    hover: Option<usize>,
    surface: Option<Surface>,
    last_tick: Instant,
    chosen: Option<String>,
    host: Arc<dyn Host>,
}

pub(crate) fn is_open() -> bool {
    OPEN.load(Ordering::SeqCst)
}

/// Deal the menu beside the character. `anchor` is his window rect in screen
/// pixels, which decides which side the cards come out of.
pub(crate) fn open(
    entries: &[MenuEntry],
    name: &str,
    anchor: RECT,
    host: Arc<dyn Host>,
) {
    if entries.is_empty() {
        return;
    }
    if OPEN.swap(true, Ordering::SeqCst) {
        return;
    }
    if let Err(e) = create(entries, name, anchor, host) {
        OPEN.store(false, Ordering::SeqCst);
        tracing::warn!("menu: {e}");
    }
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

fn create(
    entries: &[MenuEntry],
    name: &str,
    anchor: RECT,
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

    let rows = flatten(entries);
    // Bold, and a little larger than a list would need: these labels carry a
    // whole card each, and light type on a leaning slab reads as an accident.
    let mut label = TextRenderer::bold((15.0 * unit).round() as i32)
        .ok_or_else(|| "no text renderer for the menu".to_string())?;
    let mut title_font = TextRenderer::bold((18.0 * unit).round() as i32)
        .ok_or_else(|| "no title renderer for the menu".to_string())?;

    // Measure everything once. The cards are all one width so the fan reads
    // as a stack rather than a ragged list.
    let cap = (300.0 * unit) as i32;
    let mut widest = 0i32;
    let mut text = Vec::with_capacity(rows.len());
    for row in &rows {
        if matches!(row.kind, RowKind::Separator) {
            text.push(None);
            continue;
        }
        let up = row.label.to_uppercase();
        let (w, h) = label.measure(&up, cap);
        widest = widest.max(w);
        text.push(label.coverage(&up, w.max(1), h.max(1), 0x20 | 0x800).map(|c| (c, w, h)));
    }
    let back_text = {
        let (w, h) = label.measure("BACK", cap);
        widest = widest.max(w);
        label.coverage("BACK", w.max(1), h.max(1), 0x20 | 0x800).map(|c| (c, w, h))
    };

    // One title per level: the character's name at the top, the list's name
    // inside it.
    let mut titles = vec![(name.to_uppercase(), None)];
    for row in &rows {
        if matches!(row.kind, RowKind::Submenu) {
            titles.push((row.label.to_uppercase(), None));
        }
    }
    for t in titles.iter_mut() {
        let (w, h) = title_font.measure(&t.0, cap);
        t.1 = title_font.coverage(&t.0, w.max(1), h.max(1), 0x20 | 0x800).map(|c| (c, w, h));
    }

    let card_h = 40.0 * unit;
    let icon_slot = 26.0 * unit;
    let metrics = Metrics {
        card_w: (widest as f32 + icon_slot + 34.0 * unit).clamp(170.0 * unit, 330.0 * unit),
        card_h,
        spacing: card_h * 1.14,
        skew: 9.0 * unit,
        arc: 16.0 * unit,
        margin: 10.0 * unit,
        title_h: 34.0 * unit,
    };

    // Big enough for the busiest level, so the window never resizes under the
    // cursor mid-navigation.
    let top_count = rows.iter().filter(|r| r.parent.is_none() && r.selectable()).count();
    let mut most = top_count;
    for (i, row) in rows.iter().enumerate() {
        if matches!(row.kind, RowKind::Submenu) {
            let kids = rows.iter().filter(|r| r.parent == Some(i)).count();
            most = most.max(kids + 1); // + BACK
        }
    }

    let area = work_area();
    let avail_h = (area.bottom - area.top) as f32 - 24.0 * unit;
    let mut metrics = metrics;
    let wanted = most as f32 * metrics.spacing + metrics.title_h;
    if wanted > avail_h {
        // A long device list tightens up rather than running off the screen.
        metrics.spacing = ((avail_h - metrics.title_h) / most as f32).max(card_h * 0.72);
        metrics.card_h = (metrics.spacing * 0.88).min(card_h);
    }

    let width = (metrics.card_w + metrics.skew + metrics.arc + metrics.margin * 2.0
        + 6.0 * unit)
        .ceil() as i32;
    let height = (most as f32 * metrics.spacing + metrics.title_h + metrics.margin * 2.0)
        .min(avail_h)
        .ceil() as i32;

    // Which side of him has room? Falling back to the side with more space
    // keeps the cards on screen when he is parked in a corner.
    let cx = (anchor.left + anchor.right) / 2;
    let side = if (area.right - cx) >= width + 8 {
        Side::Right
    } else if (cx - area.left) >= width + 8 {
        Side::Left
    } else if (area.right - cx) >= (cx - area.left) {
        Side::Right
    } else {
        Side::Left
    };

    // Overlap him slightly so the cards look like they come out of him.
    let overlap = (36.0 * unit) as i32;
    let mut ox = match side {
        Side::Right => anchor.right - overlap,
        Side::Left => anchor.left + overlap - width,
    };
    let mut oy = (anchor.top + anchor.bottom) / 2 - height / 2;
    ox = ox.clamp(area.left, (area.right - width).max(area.left));
    oy = oy.clamp(area.top, (area.bottom - height).max(area.top));

    // The cards fly from the edge nearest him. Using his exact centre would
    // put the start point outside the window, where it would be clipped.
    let anchor_pt = match side {
        Side::Right => (0.0, height as f32 / 2.0),
        Side::Left => (width as f32 - metrics.card_w, height as f32 / 2.0),
    };

    let mut flyout = Box::new(Flyout {
        rows,
        text,
        back_text,
        titles,
        level: None,
        cards: Vec::new(),
        metrics,
        unit,
        side,
        width,
        height,
        origin: (ox, oy),
        anchor: anchor_pt,
        open_t: 0.0,
        closing: false,
        hover: None,
        surface: Surface::new(width, height),
        last_tick: Instant::now(),
        chosen: None,
        host,
    });
    if flyout.surface.is_none() {
        unsafe { DestroyWindow(hwnd) };
        return Err("could not allocate the menu surface".into());
    }
    flyout.deal();

    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(flyout) as isize);
        // Capture so a click anywhere — including well outside the cards —
        // comes back to us; foreground so Escape works.
        SetForegroundWindow(hwnd);
        SetFocus(hwnd);
        SetCapture(hwnd);
        SetTimer(hwnd, TIMER_ID, 16, None);
        ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        // Explicitly to the top of the topmost band: the companion is topmost
        // too, and whichever asked most recently wins.
        SetWindowPos(hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
    }
    if let Some(state) = unsafe { state_of(hwnd) } {
        state.paint(hwnd);
    }
    Ok(())
}

impl Flyout {
    /// Build the hand for the current level and start the deal again.
    fn deal(&mut self) {
        let mut cards: Vec<Card> = Vec::new();
        match self.level {
            None => {
                for (i, row) in self.rows.iter().enumerate() {
                    if row.parent.is_none() && row.selectable() {
                        cards.push(Card { row: i, hover: 0.0 });
                    }
                }
            }
            Some(parent) => {
                for (i, row) in self.rows.iter().enumerate() {
                    if row.parent == Some(parent) && row.selectable() {
                        cards.push(Card { row: i, hover: 0.0 });
                    }
                }
                cards.push(Card { row: BACK, hover: 0.0 });
            }
        }
        self.cards = cards;
        self.hover = None;
        self.open_t = 0.0;
    }

    /// Titles were built in submenu order, so a submenu's title is found by
    /// counting the submenus before it.
    fn title_index(&self) -> usize {
        match self.level {
            None => 0,
            Some(parent) => {
                1 + self
                    .rows
                    .iter()
                    .take(parent)
                    .filter(|r| matches!(r.kind, RowKind::Submenu))
                    .count()
            }
        }
    }

    fn slots(&self) -> Vec<Slot> {
        slots(
            self.cards.len(),
            &self.metrics,
            self.side,
            self.width as f32,
            self.height as f32,
        )
    }

    fn tick(&mut self) -> bool {
        let now = Instant::now();
        let dt = (now - self.last_tick).as_secs_f32().min(0.1);
        self.last_tick = now;

        let target = if self.closing { 0.0 } else { 1.0 };
        let secs = if self.closing { CLOSE_SECS } else { DEAL_SECS };
        let before = self.open_t;
        self.open_t = approach(self.open_t, target, dt, secs);
        let mut busy = self.open_t != before || self.open_t != target;

        for (i, c) in self.cards.iter_mut().enumerate() {
            let want = if self.hover == Some(i) { 1.0 } else { 0.0 };
            let h = approach(c.hover, want, dt, HOVER_SECS);
            busy |= h != c.hover;
            c.hover = h;
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

    fn card_at(&self, px: f32, py: f32) -> Option<usize> {
        let slots = self.slots();
        // Front to back: later cards are drawn over earlier ones.
        for i in (0..self.cards.len()).rev() {
            let t = deal_of(self.open_t, i);
            if t < 0.35 {
                continue;
            }
            let p = placement(&slots[i], self.anchor, t, self.cards[i].hover, self.side);
            if p.contains(px, py, self.metrics.skew) {
                return Some(i);
            }
        }
        None
    }

    fn on_move(&mut self, sx: i32, sy: i32) {
        if self.closing {
            return;
        }
        let (px, py) = ((sx - self.origin.0) as f32, (sy - self.origin.1) as f32);
        self.hover = self.card_at(px, py);
    }

    fn on_click(&mut self, sx: i32, sy: i32) {
        if self.closing {
            return;
        }
        let (px, py) = ((sx - self.origin.0) as f32, (sy - self.origin.1) as f32);
        let Some(i) = self.card_at(px, py) else {
            // Anywhere off the cards dismisses, like any other menu.
            self.begin_close(None);
            return;
        };
        let row = self.cards[i].row;
        if row == BACK {
            self.level = None;
            self.deal();
            return;
        }
        match self.rows[row].kind {
            RowKind::Submenu => {
                self.level = Some(row);
                self.deal();
            }
            RowKind::Item { .. } => {
                let id = self.rows[row].id.clone();
                self.begin_close(Some(id));
            }
            RowKind::Separator => {}
        }
    }

    fn paint(&mut self, hwnd: HWND) {
        let slots = self.slots();
        let m = self.metrics;
        let u = self.unit;
        let side = self.side;
        let anchor = self.anchor;
        let width = self.width as f32;
        let open = ease_out(self.open_t);
        let open_t = self.open_t;
        let title_idx = self.title_index();
        let origin = self.origin;

        // Split the borrows by field: the canvas holds `surface` mutably for
        // the whole paint, so everything it reads has to come from a sibling
        // field rather than back through `self`.
        let Flyout { surface, titles, cards, rows, text, back_text, .. } = self;
        let Some(surface) = surface.as_mut() else { return };
        let title = titles.get(title_idx).and_then(|t| t.1.as_ref());

        let mut canvas = surface.canvas();
        canvas.clear();

        // Title, leaning with the cards, over a heavy amber rule.
        if let Some((cov, cw, ch)) = title {
            let a = (open * 255.0) as u8;
            let x = match side {
                Side::Right => m.margin + m.arc * 0.5,
                Side::Left => width - m.margin - m.arc * 0.5 - *cw as f32,
            };
            let y = m.margin;
            canvas.blit_coverage_skewed(
                cov,
                *cw,
                *ch,
                x,
                y + (1.0 - open) * 6.0 * u,
                m.skew * 0.5,
                scale_alpha(TITLE_TEXT, a),
            );
            canvas.fill_skewed(
                x,
                y + *ch as f32 + 4.0 * u,
                (*cw as f32) * open,
                3.0 * u,
                m.skew * 0.35,
                scale_alpha(TITLE_TEXT, a),
            );
        }

        for (i, card) in cards.iter().enumerate() {
            let t = deal_of(open_t, i);
            if t <= 0.0 {
                continue;
            }
            let p = placement(&slots[i], anchor, t, card.hover, side);
            let a = (t.min(1.0) * 255.0) as u8;
            let hot = card.hover;

            // A hard offset shadow rather than a blur: it belongs to the same
            // flat, high-contrast language as the rest of this menu.
            canvas.fill_skewed(
                p.x + 4.0 * u,
                p.y + 4.0 * u,
                p.w,
                p.h,
                m.skew,
                scale_alpha(CARD_SHADOW, (a as f32 * 0.8) as u8),
            );

            let fill = if hot > 0.0 {
                mix(CARD_FILL, HOT_FILL, hot)
            } else {
                CARD_FILL
            };
            canvas.fill_skewed(p.x, p.y, p.w, p.h, m.skew, scale_alpha(fill, a));
            canvas.stroke_skewed(
                p.x,
                p.y,
                p.w,
                p.h,
                m.skew,
                (1.0 + hot) * u,
                scale_alpha(CARD_EDGE, a),
            );

            // The label only fades in once the card is most of the way out,
            // so text never smears across the screen behind it.
            let text_a = (((t - 0.5) / 0.5).clamp(0.0, 1.0) * 255.0) as u8;
            let enabled = card.row == BACK || rows[card.row].enabled;
            let base = if !enabled {
                CARD_TEXT_DIM
            } else if hot > 0.5 {
                HOT_TEXT
            } else {
                CARD_TEXT
            };
            let ink = scale_alpha(base, text_a);

            let icon = if card.row == BACK {
                Icon::None
            } else {
                rows[card.row].icon
            };
            let icon_cx = p.x + m.skew * 0.5 + 20.0 * u;
            let icon_cy = p.y + p.h / 2.0;
            if card.row == BACK {
                // A left-pointing chevron reads as "back" without a word.
                draw_chevron(&mut canvas, icon_cx, icon_cy, 5.0 * u, PI, 2.0 * u, ink);
            } else if icon != Icon::None {
                draw_icon(&mut canvas, icon, icon_cx, icon_cy, 17.0 * u, ink);
            }

            let cov = if card.row == BACK {
                back_text.as_ref()
            } else {
                text[card.row].as_ref()
            };
            if let Some((cov, cw, ch)) = cov {
                let tx = p.x + m.skew * 0.5 + 36.0 * u;
                let ty = p.y + (p.h - *ch as f32) / 2.0;
                canvas.blit_coverage_skewed(cov, *cw, *ch, tx, ty, m.skew * 0.5, ink);
            }

            // Checked device: a filled pip on the trailing edge.
            if let Some(RowKind::Item { checked: true }) = rows.get(card.row).map(|r| &r.kind) {
                draw_check(
                    &mut canvas,
                    p.x + p.w - 18.0 * u,
                    p.y + p.h / 2.0,
                    5.0 * u,
                    2.0 * u,
                    ink,
                );
            }
            // A submenu card carries the chevron that opens it.
            if matches!(rows.get(card.row).map(|r| &r.kind), Some(RowKind::Submenu)) {
                draw_chevron(
                    &mut canvas,
                    p.x + p.w - 16.0 * u,
                    p.y + p.h / 2.0,
                    5.0 * u,
                    0.0,
                    2.0 * u,
                    ink,
                );
            }
        }

        surface.present(hwnd, origin.0, origin.1);
    }
}

/// Blend two premultiplied colours.
fn mix(a: Px, b: Px, t: f32) -> Px {
    let t = t.clamp(0.0, 1.0);
    let ch = |shift: u32| {
        let x = ((a >> shift) & 0xff) as f32;
        let y = ((b >> shift) & 0xff) as f32;
        ((x + (y - x) * t).round() as u32).min(255) << shift
    };
    ch(24) | ch(16) | ch(8) | ch(0)
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
                    // Escape backs out one level before it closes, which is
                    // what a menu you can navigate should do.
                    if state.level.is_some() {
                        state.level = None;
                        state.deal();
                    } else {
                        state.begin_close(None);
                    }
                    SetTimer(hwnd, TIMER_ID, 16, None);
                }
            }
            return 0;
        }

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
                // Act after the window is gone, so anything the action opens
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

    fn metrics() -> Metrics {
        Metrics {
            card_w: 200.0,
            card_h: 40.0,
            spacing: 46.0,
            skew: 10.0,
            arc: 16.0,
            margin: 10.0,
            title_h: 34.0,
        }
    }

    #[test]
    fn a_long_menu_stays_on_screen() {
        // Menus are built from a package's own contents, so their length is
        // not fixed. Laid out at full spacing, a list this long runs off the
        // bottom and those cards can never be clicked.
        let m = metrics();
        let height = 500.0;
        let s = slots(30, &m, Side::Right, 260.0, height);
        assert_eq!(s.len(), 30);
        let last = s.last().unwrap();
        assert!(last.y + last.h <= height, "last card at {} overflows", last.y + last.h);
        assert!(s[0].y >= m.title_h, "first card clears the title");
        // Still in order, and still evenly spread.
        assert!(s.windows(2).all(|w| w[1].y > w[0].y), "cards keep their order");
    }

    #[test]
    fn a_short_menu_keeps_its_full_spacing() {
        // The compression is a safety net; it must not tighten menus that fit.
        let m = metrics();
        let s = slots(4, &m, Side::Right, 260.0, 500.0);
        assert!((s[1].y - s[0].y - m.spacing).abs() < 0.01, "spacing unchanged");
    }

    #[test]
    fn the_stack_bows_outward_through_its_middle() {
        // The fan is what makes it a hand of cards rather than a column of
        // buttons; a flat left edge means the bow was lost.
        let s = slots(5, &metrics(), Side::Right, 260.0, 400.0);
        assert!(s[2].x > s[0].x, "middle card reaches further out");
        assert!(s[2].x > s[4].x);
    }

    #[test]
    fn a_left_side_menu_bows_the_other_way() {
        let s = slots(5, &metrics(), Side::Left, 260.0, 400.0);
        assert!(s[2].x < s[0].x, "outward is leftward on this side");
        assert!(s[2].x < s[4].x);
    }

    #[test]
    fn cards_are_stacked_in_order_and_clear_the_title() {
        let s = slots(4, &metrics(), Side::Right, 260.0, 400.0);
        for i in 1..s.len() {
            assert!(s[i].y > s[i - 1].y, "card {i} should sit below its predecessor");
        }
        assert!(s[0].y >= metrics().title_h, "the first card must clear the title");
    }

    #[test]
    fn an_empty_hand_lays_out_nothing() {
        assert!(slots(0, &metrics(), Side::Right, 260.0, 400.0).is_empty());
    }

    #[test]
    fn a_card_starts_at_the_anchor_and_ends_in_its_slot() {
        let s = slots(3, &metrics(), Side::Right, 260.0, 400.0);
        let anchor = (0.0, 200.0);
        let start = placement(&s[1], anchor, 0.0, 0.0, Side::Right);
        assert_eq!((start.x, start.y), anchor);
        let end = placement(&s[1], anchor, 1.0, 0.0, Side::Right);
        assert!((end.x - s[1].x).abs() < 0.01);
        assert!((end.y - s[1].y).abs() < 0.01);
    }

    #[test]
    fn hovering_pushes_a_card_further_out() {
        let s = slots(3, &metrics(), Side::Right, 260.0, 400.0);
        let anchor = (0.0, 200.0);
        let cold = placement(&s[1], anchor, 1.0, 0.0, Side::Right);
        let hot = placement(&s[1], anchor, 1.0, 1.0, Side::Right);
        assert!(hot.x > cold.x, "on the right, out is further right");

        let cold = placement(&s[1], anchor, 1.0, 0.0, Side::Left);
        let hot = placement(&s[1], anchor, 1.0, 1.0, Side::Left);
        assert!(hot.x < cold.x, "on the left, out is further left");
    }

    #[test]
    fn the_throw_overshoots_before_it_settles() {
        // Without the overshoot the cards look placed, not thrown.
        assert!(back_out(0.75) > 1.0, "should pass its slot on the way in");
        assert_eq!(back_out(1.0), 1.0);
        assert_eq!(back_out(0.0), 0.0);
    }

    #[test]
    fn hit_testing_follows_the_lean() {
        // A point near the bottom-left is inside a leaning card; the same
        // point near the top is not, because the card has moved right.
        let slot = Slot { x: 100.0, y: 100.0, w: 60.0, h: 40.0 };
        assert!(slot.contains(105.0, 138.0, 20.0), "bottom edge sits left");
        assert!(!slot.contains(105.0, 102.0, 20.0), "top edge has leaned away");
        assert!(slot.contains(125.0, 102.0, 20.0), "top edge is further right");
    }

    #[test]
    fn a_point_outside_the_card_is_never_a_hit() {
        let slot = Slot { x: 100.0, y: 100.0, w: 60.0, h: 40.0 };
        assert!(!slot.contains(50.0, 120.0, 10.0));
        assert!(!slot.contains(300.0, 120.0, 10.0));
        assert!(!slot.contains(120.0, 50.0, 10.0));
        assert!(!slot.contains(120.0, 300.0, 10.0));
    }

    #[test]
    fn every_card_finishes_arriving() {
        // The regression that leaves the bottom of a long device list
        // permanently short of its slot.
        for i in 0..14 {
            assert_eq!(deal_of(1.0, i), 1.0, "card {i} never arrived");
        }
        assert_eq!(deal_of(0.0, 0), 0.0);
    }

    #[test]
    fn cards_arrive_one_after_another() {
        assert!(deal_of(0.4, 0) > deal_of(0.4, 3));
    }

    #[test]
    fn approach_lands_exactly_on_its_target() {
        let mut v = 0.0;
        for _ in 0..200 {
            v = approach(v, 1.0, 0.016, DEAL_SECS);
        }
        assert_eq!(v, 1.0);
        for _ in 0..200 {
            v = approach(v, 0.0, 0.016, CLOSE_SECS);
        }
        assert_eq!(v, 0.0);
    }

    #[test]
    fn mixing_a_colour_with_itself_changes_nothing() {
        let c = rgba(240, 160, 48, 255);
        assert_eq!(mix(c, c, 0.5), c);
        assert_eq!(mix(c, rgba(0, 0, 0, 255), 0.0), c);
    }
}
