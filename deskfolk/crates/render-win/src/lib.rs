//! A native layered-window renderer for a Deskfolk companion.
//!
//! # Why this crate exists
//!
//! The companion used to be a transparent WebView2 window drawing into a
//! canvas. It looked right for about a second at a time: WebView2's host
//! window draws its own minimise/maximise/close buttons and a border on hover,
//! and nothing removes them. That was chased to the end — `WS_CAPTION`,
//! `WS_SYSMENU`, `WS_MINIMIZEBOX` and `WS_MAXIMIZEBOX` all stripped, a
//! `WM_NCCALCSIZE` subclass leaving *zero* non-client area (window rect ==
//! client rect, 536x536), `DWMWA_BORDER_COLOR = NONE`,
//! `DWMWA_WINDOW_CORNER_PREFERENCE = DONOTROUND`, the accent policy disabled,
//! and a periodic re-strip that found nothing left to strip. The buttons kept
//! rendering, which leaves only one explanation: the WebView2 host draws them
//! itself, below the level any of those knobs reach.
//!
//! So the companion stops being a web page. This crate owns a plain
//! `WS_EX_LAYERED` popup and composites the character into it in software —
//! true per-pixel alpha, no HTML, and no host chrome that could exist in the
//! first place.
//!
//! # What did *not* change
//!
//! The engine, the package format, the AI layer, audio, and config are all
//! untouched. This crate consumes exactly what the webview consumed — a
//! `Composition` of already-positioned rectangles from
//! `deskfolk_engine::compose` — so the boundary that made this swap a contained
//! change is the same boundary that keeps the character's behaviour identical.
//!
//! # Windows only
//!
//! Layered windows are a Win32 concept. On other platforms this crate builds
//! to nothing and the app runs without a companion, which is honest about
//! where Deskfolk actually ships today.

#[cfg(windows)]
pub mod canvas;
#[cfg(windows)]
mod desktop;
#[cfg(windows)]
mod flyout;
#[cfg(windows)]
mod hotkey;
/// The desktop's own windows, as surfaces he can stand on.
pub mod ledges;
/// Pure data — no Win32 — so the menu a host describes typechecks everywhere.
mod menu;
#[cfg(windows)]
mod paint;
#[cfg(windows)]
mod sprites;
#[cfg(windows)]
mod surface;
#[cfg(windows)]
mod text;
#[cfg(windows)]
mod window;

pub use menu::{Icon, MenuEntry};
#[cfg(windows)]
pub use window::cursor_pos;

#[cfg(windows)]
use std::sync::atomic::Ordering;
#[cfg(windows)]
use std::sync::Arc;

use deskfolk_engine::layout::Composition;
use deskfolk_engine::Portal;
use deskfolk_package::{CharacterPackage, Stage};

/// Everything to draw for one frame.
///
/// Deliberately a flat snapshot rather than a handle to the engine: the window
/// lives on another thread, and a value it can own outright means no lock is
/// ever held across a paint.
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub comp: Composition,
    pub subtitle: Option<String>,
    pub speaking: bool,
    pub thinking: bool,
    pub listening: bool,
    /// Voice loudness 0..=100, for the listening pulse.
    pub level: u8,
}

#[derive(Debug, Clone)]
pub struct Config {
    /// The character's name — it heads his menu.
    pub name: String,
    pub stage: Stage,
    /// Stage units to logical pixels, before display scaling.
    pub scale: f64,
    pub portal: Portal,
    /// Parent him into the wallpaper surface instead of floating above every
    /// window. He becomes a genuine resident of the desktop — and is only
    /// visible when the desktop is.
    pub on_desktop: bool,
    /// Round the scale so one art pixel is a whole number of screen pixels.
    /// Off means arbitrary sizes and a visibly ragged silhouette.
    pub pixel_snap: bool,
    /// A system-wide chord like `ctrl+alt+y` that starts him listening from
    /// inside whatever you are working in. `None` registers nothing.
    pub hotkey: Option<String>,
}

/// What the window needs from the application.
///
/// Called on the window's own thread, so implementations must not block on a
/// lock the render loop might be holding.
pub trait Host: Send + Sync + 'static {
    /// Build the right-click menu, fresh — audio devices come and go.
    fn menu(&self) -> Vec<MenuEntry>;
    fn on_click(&self);
    fn on_menu(&self, id: &str);
    /// The global hotkey was pressed, from wherever the user happened to be.
    fn on_hotkey(&self) {}
    /// He was dragged somewhere new, in screen pixels.
    fn on_moved(&self, _x: i32, _y: i32) {}
}

/// A running companion window.
#[cfg(windows)]
pub struct Companion {
    hwnd: isize,
    shared: Arc<window::Shared>,
    size: (i32, i32),
    on_desktop: bool,
}

#[cfg(windows)]
impl Companion {
    /// Decode the package's sprites and put a window on the desktop.
    pub fn spawn(
        pkg: &CharacterPackage,
        config: Config,
        host: Arc<dyn Host>,
    ) -> Result<Self, String> {
        let sprites = sprites::Sprites::load(pkg);
        if sprites.is_empty() {
            return Err("no sprites decoded; refusing to open an empty window".into());
        }
        tracing::info!(
            "{} sprites decoded ({} KiB) for the native renderer",
            sprites.len(),
            sprites.bytes() / 1024
        );

        let stage = config.stage;
        let scale = config.scale;
        let on_desktop = config.on_desktop;
        let spawned = window::spawn(config, sprites, host)?;
        Ok(Self {
            hwnd: spawned.hwnd,
            shared: spawned.shared,
            size: (
                (stage.width as f64 * scale).round() as i32,
                (stage.height as f64 * scale).round() as i32,
            ),
            on_desktop,
        })
    }

    /// Hand the window a frame to draw. Never blocks on painting.
    pub fn present(&self, frame: Frame) {
        window::present(self.hwnd, &self.shared, frame);
    }

    /// Top-left of the window in screen pixels.
    pub fn position(&self) -> (i32, i32) {
        (
            self.shared.x.load(Ordering::Relaxed),
            self.shared.y.load(Ordering::Relaxed),
        )
    }

    /// Logical size, before display scaling.
    pub fn size(&self) -> (i32, i32) {
        self.size
    }

    /// Put him somewhere, in screen pixels — the same path a drag takes, so a
    /// walk and a drag cannot disagree about where he is.
    pub fn move_to(&self, x: i32, y: i32) {
        window::move_to(self.hwnd, &self.shared, x, y);
    }

    /// The ledges on the desktop right now, front-most first. His own window
    /// is excluded; he cannot stand on himself.
    pub fn ledges(&self) -> Vec<ledges::Ledge> {
        window::ledges_excluding(self.hwnd)
    }

    /// The monitor's scale factor — 1.0 at 96 DPI, 1.5 at 150% scaling.
    pub fn dpi_factor(&self) -> f64 {
        self.shared.dpi.load(Ordering::Relaxed) as f64 / 96.0
    }

    /// Re-assert always-on-top. A no-op on the desktop layer, where being on
    /// top is precisely what he is not.
    pub fn raise(&self) {
        if !self.on_desktop {
            window::raise(self.hwnd);
        }
    }

    pub fn close(&self) {
        window::close(self.hwnd);
    }
}

#[cfg(not(windows))]
pub struct Companion;

#[cfg(not(windows))]
impl Companion {
    pub fn spawn(
        _pkg: &CharacterPackage,
        _config: Config,
        _host: std::sync::Arc<dyn Host>,
    ) -> Result<Self, String> {
        Err("the companion renderer is Windows-only".into())
    }
}
