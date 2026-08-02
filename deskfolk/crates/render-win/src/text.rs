//! Text, by way of a coverage map.
//!
//! GDI is the only text engine guaranteed to be present, and it has one
//! disqualifying habit: it writes glyphs with the alpha byte left at zero. Draw
//! straight into the layered window's DIB and the text is perfectly rendered
//! and completely invisible.
//!
//! So glyphs go onto a scratch bitmap as white-on-black, and their brightness
//! is read back as an 8-bit coverage map. The compositor then paints whatever
//! colour we want through it, at whatever opacity — which also means the
//! speech bubble's text can sit on a translucent panel without GDI ever
//! knowing alpha exists.

use std::ffi::c_void;

use windows_sys::Win32::Foundation::RECT;
use windows_sys::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, CreateFontW, DeleteDC, DeleteObject, DrawTextW, GetDC,
    ReleaseDC, SelectObject, SetBkMode, SetTextColor, ANTIALIASED_QUALITY, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, DEFAULT_CHARSET, DIB_RGB_COLORS, DT_CALCRECT, DT_NOPREFIX,
    DT_WORDBREAK, FW_SEMIBOLD, HBITMAP, HDC, HFONT, HGDIOBJ, TRANSPARENT,
};

/// UTF-16, NUL-terminated — what the W-suffixed entry points expect.
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub struct TextRenderer {
    dc: HDC,
    bitmap: HBITMAP,
    old_bitmap: HGDIOBJ,
    bits: *mut u32,
    w: i32,
    h: i32,
    font: HFONT,
    old_font: HGDIOBJ,
}

impl TextRenderer {
    /// A UI font at `px` device pixels. `None` if GDI will not give us a DC,
    /// in which case the caller simply draws no text — a bubble without a
    /// caption still beats no companion.
    pub fn new(px: i32) -> Option<Self> {
        unsafe {
            let screen = GetDC(std::ptr::null_mut());
            let dc = CreateCompatibleDC(screen);
            ReleaseDC(std::ptr::null_mut(), screen);
            if dc.is_null() {
                return None;
            }
            // Negative height asks for a font of that *character* height,
            // which is what "12px text" means everywhere else in this project.
            let face = wide("Segoe UI");
            let font = CreateFontW(
                -px.max(1),
                0,
                0,
                0,
                FW_SEMIBOLD as i32,
                0,
                0,
                0,
                DEFAULT_CHARSET as u32,
                0,
                0,
                // Greyscale AA, not ClearType: subpixel rendering would put
                // coloured fringes into a channel we read as pure coverage.
                ANTIALIASED_QUALITY as u32,
                0,
                face.as_ptr(),
            );
            if font.is_null() {
                DeleteDC(dc);
                return None;
            }
            let old_font = SelectObject(dc, font as HGDIOBJ);
            SetBkMode(dc, TRANSPARENT as i32);
            SetTextColor(dc, 0x00FF_FFFF);
            Some(Self {
                dc,
                bitmap: std::ptr::null_mut(),
                old_bitmap: std::ptr::null_mut(),
                bits: std::ptr::null_mut(),
                w: 0,
                h: 0,
                font,
                old_font,
            })
        }
    }

    /// Wrapped size of `text` within `max_w`, in device pixels.
    pub fn measure(&self, text: &str, max_w: i32) -> (i32, i32) {
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: max_w.max(1),
            bottom: 0,
        };
        let s = wide(text);
        unsafe {
            DrawTextW(
                self.dc,
                s.as_ptr(),
                -1,
                &mut rect,
                DT_CALCRECT | DT_WORDBREAK | DT_NOPREFIX,
            );
        }
        ((rect.right - rect.left).max(0), (rect.bottom - rect.top).max(0))
    }

    fn ensure(&mut self, w: i32, h: i32) -> bool {
        if w <= 0 || h <= 0 {
            return false;
        }
        if !self.bitmap.is_null() && self.w >= w && self.h >= h {
            return true;
        }
        // Grow only — a companion that talks for an hour should not churn GDI
        // objects on every line of dialogue.
        let (nw, nh) = (self.w.max(w), self.h.max(h));
        unsafe {
            let mut info: BITMAPINFO = std::mem::zeroed();
            info.bmiHeader = BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: nw,
                biHeight: -nh,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                biSizeImage: 0,
                biXPelsPerMeter: 0,
                biYPelsPerMeter: 0,
                biClrUsed: 0,
                biClrImportant: 0,
            };
            let mut bits: *mut c_void = std::ptr::null_mut();
            let bitmap = CreateDIBSection(
                self.dc,
                &info,
                DIB_RGB_COLORS,
                &mut bits,
                std::ptr::null_mut(),
                0,
            );
            if bitmap.is_null() || bits.is_null() {
                return false;
            }
            let old = SelectObject(self.dc, bitmap as HGDIOBJ);
            if !self.bitmap.is_null() {
                DeleteObject(self.bitmap as HGDIOBJ);
            } else {
                self.old_bitmap = old;
            }
            self.bitmap = bitmap;
            self.bits = bits as *mut u32;
            self.w = nw;
            self.h = nh;
        }
        true
    }

    /// Render `text` wrapped into a `w` x `h` box and return its coverage.
    ///
    /// `flags` are raw `DrawText` flags so the caller can centre or left-align
    /// without this module growing a layout opinion.
    pub fn coverage(&mut self, text: &str, w: i32, h: i32, flags: u32) -> Option<Vec<u8>> {
        if !self.ensure(w, h) {
            return None;
        }
        let stride = self.w as usize;
        // Safety: `bits` covers exactly self.w * self.h pixels and the DIB is
        // selected into `dc`, so GDI writes into this same memory.
        let px = unsafe {
            std::slice::from_raw_parts_mut(self.bits, stride * self.h as usize)
        };
        // Black background: brightness *is* coverage.
        for y in 0..h as usize {
            px[y * stride..y * stride + w as usize].fill(0);
        }

        let mut rect = RECT {
            left: 0,
            top: 0,
            right: w,
            bottom: h,
        };
        let s = wide(text);
        unsafe {
            DrawTextW(self.dc, s.as_ptr(), -1, &mut rect, flags);
            // GDI batches; the bits are not guaranteed written until it flushes.
            windows_sys::Win32::Graphics::Gdi::GdiFlush();
        }

        let mut cov = vec![0u8; (w * h) as usize];
        for y in 0..h as usize {
            for x in 0..w as usize {
                let p = px[y * stride + x];
                let (r, g, b) = ((p >> 16) & 0xff, (p >> 8) & 0xff, p & 0xff);
                cov[y * w as usize + x] = r.max(g).max(b) as u8;
            }
        }
        Some(cov)
    }
}

impl Drop for TextRenderer {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.old_font);
            DeleteObject(self.font as HGDIOBJ);
            if !self.bitmap.is_null() {
                SelectObject(self.dc, self.old_bitmap);
                DeleteObject(self.bitmap as HGDIOBJ);
            }
            DeleteDC(self.dc);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wide_strings_are_nul_terminated() {
        let s = wide("hi");
        assert_eq!(s, vec![b'h' as u16, b'i' as u16, 0]);
    }

    #[test]
    fn wide_handles_non_ascii() {
        // He swears in ellipses and em-dashes; a truncating conversion would
        // corrupt the bubble.
        let s = wide("café…");
        assert_eq!(*s.last().unwrap(), 0);
        assert!(s.len() > 5);
    }
}
