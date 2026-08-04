//! The desktop as a set of things he can stand on.
//!
//! This is the nearest thing Windows has to a DOM: every top-level window is a
//! rectangle with a position, a title and a stacking order, and the top edge of
//! one is a shelf a small person could sit on. Reading that turns "he floats at
//! a fixed spot" into "he is somewhere, in relation to what you are doing".
//!
//! What counts as a ledge is deliberately narrow. Enumerating top-level windows
//! returns a great deal that is not a window in the sense a user means: hidden
//! helpers, zero-size message sinks, tool windows, and — the one that catches
//! everybody on Windows 10 and later — *cloaked* windows, which are the store
//! apps sitting suspended on another virtual desktop. They report themselves
//! visible with a perfectly good rectangle, so filtering on `IsWindowVisible`
//! alone puts him on a shelf that is not on screen.

#[cfg(windows)]
use windows_sys::Win32::Foundation::{HWND, LPARAM, RECT};

/// A surface he can perch on: the top edge of a real window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ledge {
    /// Left and right ends of the usable edge, in screen pixels.
    pub left: i32,
    pub right: i32,
    /// The height of the edge itself — where his feet go.
    pub top: i32,
    /// Whose edge it is, for logs and for him to react to.
    pub title: String,
}

impl Ledge {
    pub fn width(&self) -> i32 {
        self.right - self.left
    }

    /// Somewhere along it he could stand without hanging off either end.
    pub fn clamp_x(&self, x: i32, half_width: i32) -> i32 {
        let lo = self.left + half_width;
        let hi = self.right - half_width;
        if lo >= hi {
            // Narrower than he is: centre him and let him overhang evenly.
            (self.left + self.right) / 2
        } else {
            x.clamp(lo, hi)
        }
    }
}

/// A window narrower or shorter than this is furniture, not a place to sit.
const MIN_LEDGE_WIDTH: i32 = 220;
const MIN_LEDGE_HEIGHT: i32 = 120;

/// Turn raw window rectangles into ledges.
///
/// Split out from the enumeration so the rules can be tested without a desktop:
/// everything interesting here is the filtering, not the FFI.
pub fn ledges_from(
    windows: impl IntoIterator<Item = (RectLike, String)>,
    work: RectLike,
) -> Vec<Ledge> {
    let mut out: Vec<Ledge> = windows
        .into_iter()
        .filter_map(|(r, title)| {
            if r.width() < MIN_LEDGE_WIDTH || r.height() < MIN_LEDGE_HEIGHT {
                return None;
            }
            // A maximised or off-screen window's top edge is not a shelf: the
            // first is flush with the top of the screen and the second is not
            // on it. Requiring the edge to sit inside the work area covers
            // both, and keeps him off the sliver above a full-screen video.
            if r.top <= work.top || r.top >= work.bottom {
                return None;
            }
            // Clip to the visible desktop so he cannot walk off the side of a
            // window that is half off-screen.
            let left = r.left.max(work.left);
            let right = r.right.min(work.right);
            if right - left < MIN_LEDGE_WIDTH {
                return None;
            }
            Some(Ledge { left, right, top: r.top, title })
        })
        .collect();

    // The desktop floor is always available, so he is never stranded when
    // every window closes. Last, so a real window is preferred to it.
    out.push(Ledge {
        left: work.left,
        right: work.right,
        top: work.bottom,
        title: "the desktop".into(),
    });
    out
}

/// A rectangle, independent of Win32 so the rules above can be unit-tested.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RectLike {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl RectLike {
    pub fn width(&self) -> i32 {
        self.right - self.left
    }
    pub fn height(&self) -> i32 {
        self.bottom - self.top
    }
}

// ---------------------------------------------------------------------------
// Reading the real desktop
// ---------------------------------------------------------------------------

#[cfg(windows)]
mod win {
    use super::*;
    use windows_sys::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowLongPtrW, GetWindowRect, GetWindowTextLengthW, GetWindowTextW,
        IsIconic, IsWindowVisible, GWL_EXSTYLE, WS_EX_TOOLWINDOW,
    };

    struct Scan {
        skip: HWND,
        found: Vec<(RectLike, String)>,
    }

    /// Is this window one a person would say is on their screen?
    unsafe fn is_real_window(hwnd: HWND, skip: HWND) -> bool {
        if hwnd == skip || IsWindowVisible(hwnd) == 0 || IsIconic(hwnd) != 0 {
            return false;
        }
        // Tool windows are palettes and helpers; they are not places.
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        if ex & WS_EX_TOOLWINDOW != 0 {
            return false;
        }
        // Cloaked: visible by every classic test, but actually suspended on
        // another virtual desktop. Without this he perches on nothing.
        let mut cloaked: u32 = 0;
        let ok = DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED as u32,
            (&mut cloaked as *mut u32).cast(),
            std::mem::size_of::<u32>() as u32,
        );
        if ok == 0 && cloaked != 0 {
            return false;
        }
        GetWindowTextLengthW(hwnd) > 0
    }

    unsafe fn title_of(hwnd: HWND) -> String {
        let len = GetWindowTextLengthW(hwnd);
        if len <= 0 {
            return String::new();
        }
        let mut buf = vec![0u16; len as usize + 1];
        let n = GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
        String::from_utf16_lossy(&buf[..n.max(0) as usize])
    }

    unsafe extern "system" fn each(hwnd: HWND, lparam: LPARAM) -> i32 {
        let scan = &mut *(lparam as *mut Scan);
        if !is_real_window(hwnd, scan.skip) {
            return 1;
        }
        let mut r: RECT = std::mem::zeroed();
        if GetWindowRect(hwnd, &mut r) == 0 {
            return 1;
        }
        scan.found.push((
            RectLike { left: r.left, top: r.top, right: r.right, bottom: r.bottom },
            title_of(hwnd),
        ));
        1
    }

    /// Every ledge on the desktop right now, nearest the front first.
    ///
    /// `skip` is the companion's own window — he cannot stand on himself.
    pub fn scan(skip: isize, work: RectLike) -> Vec<Ledge> {
        let mut scan = Scan { skip: skip as HWND, found: Vec::new() };
        unsafe {
            EnumWindows(Some(each), (&mut scan as *mut Scan) as LPARAM);
        }
        // EnumWindows walks front to back, which is the order he should prefer:
        // the window you are actually using is the interesting one to sit on.
        ledges_from(scan.found, work)
    }
}

#[cfg(windows)]
pub use win::scan;

#[cfg(test)]
mod tests {
    use super::*;

    fn work() -> RectLike {
        RectLike { left: 0, top: 0, right: 1920, bottom: 1040 }
    }

    fn r(left: i32, top: i32, right: i32, bottom: i32) -> RectLike {
        RectLike { left, top, right, bottom }
    }

    #[test]
    fn a_normal_window_offers_its_top_edge() {
        let l = ledges_from(vec![(r(300, 200, 900, 700), "Editor".into())], work());
        assert_eq!(l[0], Ledge { left: 300, right: 900, top: 200, title: "Editor".into() });
    }

    #[test]
    fn the_desktop_floor_is_always_there_but_always_last() {
        // Otherwise closing every window strands him mid-air.
        let l = ledges_from(vec![(r(300, 200, 900, 700), "Editor".into())], work());
        assert_eq!(l.len(), 2);
        assert_eq!(l.last().unwrap().title, "the desktop");
        assert_eq!(l.last().unwrap().top, 1040, "stands on the work area's floor");

        let bare = ledges_from(vec![], work());
        assert_eq!(bare.len(), 1, "never stranded: {bare:?}");
    }

    #[test]
    fn slivers_and_furniture_are_not_places_to_sit() {
        let l = ledges_from(
            vec![
                (r(0, 300, 100, 700), "narrow".into()),
                (r(300, 300, 900, 340), "short".into()),
            ],
            work(),
        );
        assert_eq!(l.len(), 1, "only the desktop survives: {l:?}");
    }

    #[test]
    fn a_maximised_window_has_no_edge_to_sit_on() {
        // Its top is flush with the screen, so there is no shelf — and putting
        // him there would pin him to the very top of the display.
        let l = ledges_from(vec![(r(0, 0, 1920, 1040), "Maximised".into())], work());
        assert_eq!(l.len(), 1, "{l:?}");
    }

    #[test]
    fn an_edge_below_the_work_area_is_off_screen() {
        let l = ledges_from(vec![(r(300, 1200, 900, 1500), "Below".into())], work());
        assert_eq!(l.len(), 1, "{l:?}");
    }

    #[test]
    fn a_half_off_screen_window_is_clipped_to_what_is_visible() {
        let l = ledges_from(vec![(r(-400, 200, 700, 700), "Hanging off".into())], work());
        assert_eq!(l[0].left, 0, "cannot walk off the left of the screen");
        assert_eq!(l[0].right, 700);
    }

    #[test]
    fn clipping_can_leave_too_little_to_stand_on() {
        let l = ledges_from(vec![(r(-900, 200, 100, 700), "Mostly gone".into())], work());
        assert_eq!(l.len(), 1, "{l:?}");
    }

    #[test]
    fn he_stands_within_the_ledge_not_hanging_off_it() {
        let l = Ledge { left: 300, right: 900, top: 200, title: "e".into() };
        assert_eq!(l.clamp_x(100, 50), 350, "pushed in from the left end");
        assert_eq!(l.clamp_x(5000, 50), 850, "pushed in from the right end");
        assert_eq!(l.clamp_x(600, 50), 600, "already comfortable");
    }

    #[test]
    fn a_ledge_narrower_than_he_is_centres_him() {
        let l = Ledge { left: 300, right: 400, top: 200, title: "e".into() };
        assert_eq!(l.clamp_x(0, 200), 350, "overhangs evenly rather than snapping");
    }
}
