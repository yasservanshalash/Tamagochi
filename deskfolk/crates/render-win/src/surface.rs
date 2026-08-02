//! The drawing surface: a 32bpp top-down DIB section, presented with
//! `UpdateLayeredWindow`.
//!
//! This is the whole reason the crate exists. A layered window painted this
//! way has *genuine* per-pixel alpha and no host chrome of any kind — there is
//! no HTML document, no WebView2 host window, and therefore nothing left that
//! can decide to draw a caption or a pair of system buttons over the
//! character. It is also its own hit test: Windows routes clicks on alpha-0
//! pixels straight to whatever is underneath.

use std::ffi::c_void;

use windows_sys::Win32::Foundation::{HWND, POINT, SIZE};
use windows_sys::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GdiFlush, GetDC, ReleaseDC,
    SelectObject, AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION, BI_RGB,
    DIB_RGB_COLORS, HBITMAP, HDC, HGDIOBJ,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{UpdateLayeredWindow, ULW_ALPHA};

use crate::canvas::{Canvas, Px};

pub struct Surface {
    pub width: i32,
    pub height: i32,
    dc: HDC,
    bitmap: HBITMAP,
    old: HGDIOBJ,
    bits: *mut Px,
}

impl Surface {
    /// Allocate a top-down 32bpp surface. `None` if GDI refuses, which the
    /// caller should treat as "no companion this run" rather than a panic.
    pub fn new(width: i32, height: i32) -> Option<Self> {
        if width <= 0 || height <= 0 {
            return None;
        }
        unsafe {
            let screen = GetDC(std::ptr::null_mut());
            let dc = CreateCompatibleDC(screen);
            ReleaseDC(std::ptr::null_mut(), screen);
            if dc.is_null() {
                return None;
            }

            let mut info: BITMAPINFO = std::mem::zeroed();
            info.bmiHeader = BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                // Negative height means top-down rows, so row 0 is the top of
                // the window and the buffer indexes the way every other part
                // of this codebase thinks about y.
                biHeight: -height,
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
                dc,
                &info,
                DIB_RGB_COLORS,
                &mut bits,
                std::ptr::null_mut(),
                0,
            );
            if bitmap.is_null() || bits.is_null() {
                DeleteDC(dc);
                return None;
            }
            let old = SelectObject(dc, bitmap as HGDIOBJ);
            Some(Self {
                width,
                height,
                dc,
                bitmap,
                old,
                bits: bits as *mut Px,
            })
        }
    }

    fn pixels_mut(&mut self) -> &mut [Px] {
        // Safety: `bits` is a DIB section of exactly width*height 32bpp
        // pixels, owned by this struct and freed only in `Drop`.
        unsafe {
            std::slice::from_raw_parts_mut(self.bits, (self.width * self.height) as usize)
        }
    }

    pub fn canvas(&mut self) -> Canvas<'_> {
        let (w, h) = (self.width, self.height);
        Canvas::new(self.pixels_mut(), w, h)
    }

    /// Push the surface to the screen and place the window in one call.
    ///
    /// `UpdateLayeredWindow` moves *and* repaints atomically, which is why
    /// dragging is smooth: there is no window that has moved but not yet
    /// redrawn, so the character never tears away from the cursor.
    pub fn present(&self, hwnd: HWND, x: i32, y: i32) -> bool {
        unsafe {
            // Ordering barrier: we wrote the bits directly, GDI reads them.
            GdiFlush();
            let screen = GetDC(std::ptr::null_mut());
            let mut dst = POINT { x, y };
            let mut src = POINT { x: 0, y: 0 };
            let mut size = SIZE {
                cx: self.width,
                cy: self.height,
            };
            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                // Without this the alpha channel is ignored and the window
                // shows up as an opaque black square.
                AlphaFormat: AC_SRC_ALPHA as u8,
            };
            let ok = UpdateLayeredWindow(
                hwnd,
                screen,
                &mut dst,
                &mut size,
                self.dc,
                &mut src,
                0,
                &blend,
                ULW_ALPHA,
            ) != 0;
            ReleaseDC(std::ptr::null_mut(), screen);
            ok
        }
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.old);
            DeleteObject(self.bitmap as HGDIOBJ);
            DeleteDC(self.dc);
        }
    }
}
