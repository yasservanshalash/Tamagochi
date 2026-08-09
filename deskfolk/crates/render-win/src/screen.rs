//! Reading how busy a patch of the desktop is, so he can stand somewhere calm.
//!
//! He would rather not plant himself over a wall of text or a cluster of icons.
//! There is no API for "where is the clutter" — the only honest answer is to
//! look at the pixels. So a candidate spot is captured off the screen and scored
//! for how much fine detail is in it: a blank stretch of wallpaper scores near
//! zero, a paragraph of text or a grid of icons scores high.
//!
//! The scoring ([`busyness`]) is a pure function of a pixel buffer, kept apart
//! from the Win32 capture so it can be tested with synthetic images. The capture
//! ([`capture_region`]) is the only part that touches the screen, and it can
//! read the desktop now precisely because the companion is an ordinary
//! composited layered window rather than the old WebView2 surface.

/// How busy a captured region is, on a 0 (blank) .. 100 (dense detail) scale.
///
/// Measured as the average absolute luminance difference between neighbouring
/// pixels — an edge-density estimate. Flat colour has no edges; text, icons and
/// UI chrome are almost all edges. Pure, so it is testable without a screen.
pub fn busyness(pixels: &[u32], w: usize, h: usize) -> u32 {
    if w < 2 || h < 2 || pixels.len() < w * h {
        return 0;
    }
    // Rec. 601-ish luma from a 0x00RRGGBB pixel, in fixed point.
    let lum = |p: u32| -> i32 {
        let r = ((p >> 16) & 0xff) as i32;
        let g = ((p >> 8) & 0xff) as i32;
        let b = (p & 0xff) as i32;
        (r * 54 + g * 183 + b * 19) >> 8
    };
    let mut edges: u64 = 0;
    let mut count: u64 = 0;
    for y in 0..h {
        for x in 0..w {
            let c = lum(pixels[y * w + x]);
            if x + 1 < w {
                edges += (lum(pixels[y * w + x + 1]) - c).unsigned_abs() as u64;
                count += 1;
            }
            if y + 1 < h {
                edges += (lum(pixels[(y + 1) * w + x]) - c).unsigned_abs() as u64;
                count += 1;
            }
        }
    }
    if count == 0 {
        return 0;
    }
    let avg = edges / count; // 0..255 average neighbour delta
    // An average neighbour delta at or above this reads as "full of detail".
    // Text against a page sits well above it; smooth wallpaper sits near zero.
    const BUSY_FULL: u64 = 24;
    ((avg * 100 / BUSY_FULL).min(100)) as u32
}

/// Grab a rectangle of the screen as top-down 0x00RRGGBB pixels.
///
/// `None` if the region is empty or GDI refuses — the caller treats that as
/// "can't tell", not a panic.
#[cfg(windows)]
pub fn capture_region(x: i32, y: i32, w: i32, h: i32) -> Option<Vec<u32>> {
    use std::ffi::c_void;
    use windows_sys::Win32::Graphics::Gdi::{
        BitBlt, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC,
        SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HGDIOBJ, SRCCOPY,
    };

    if w <= 0 || h <= 0 {
        return None;
    }
    unsafe {
        let screen = GetDC(std::ptr::null_mut());
        if screen.is_null() {
            return None;
        }
        let mem = CreateCompatibleDC(screen);
        if mem.is_null() {
            ReleaseDC(std::ptr::null_mut(), screen);
            return None;
        }

        let mut info: BITMAPINFO = std::mem::zeroed();
        info.bmiHeader = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            biHeight: -h, // top-down, matching the rest of the crate
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
        let bmp = CreateDIBSection(mem, &info, DIB_RGB_COLORS, &mut bits, std::ptr::null_mut(), 0);
        if bmp.is_null() || bits.is_null() {
            DeleteDC(mem);
            ReleaseDC(std::ptr::null_mut(), screen);
            return None;
        }
        let old = SelectObject(mem, bmp as HGDIOBJ);
        let ok = BitBlt(mem, 0, 0, w, h, screen, x, y, SRCCOPY) != 0;
        let out = if ok {
            let slice = std::slice::from_raw_parts(bits as *const u32, (w * h) as usize);
            Some(slice.to_vec())
        } else {
            None
        };
        SelectObject(mem, old);
        DeleteObject(bmp as HGDIOBJ);
        DeleteDC(mem);
        ReleaseDC(std::ptr::null_mut(), screen);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_flat_region_is_not_busy() {
        let px = vec![0x00303030u32; 40 * 40];
        assert_eq!(busyness(&px, 40, 40), 0, "flat colour has no detail");
    }

    #[test]
    fn a_checkerboard_is_maximally_busy() {
        // Every neighbour flips black<->white: as dense as detail gets.
        let (w, h) = (40usize, 40usize);
        let mut px = vec![0u32; w * h];
        for y in 0..h {
            for x in 0..w {
                px[y * w + x] = if (x + y) & 1 == 0 { 0x00000000 } else { 0x00ffffff };
            }
        }
        assert_eq!(busyness(&px, w, h), 100, "hard checker should saturate");
    }

    #[test]
    fn text_like_detail_beats_a_calm_gradient() {
        let (w, h) = (60usize, 60usize);
        // A smooth horizontal gradient: some edges, but gentle.
        let mut gradient = vec![0u32; w * h];
        for y in 0..h {
            for x in 0..w {
                let v = (x * 255 / w) as u32;
                gradient[y * w + x] = (v << 16) | (v << 8) | v;
            }
        }
        // Sparse dark "glyphs" on a light page: occasional hard edges.
        let mut page = vec![0x00f0f0f0u32; w * h];
        for y in 0..h {
            for x in 0..w {
                if x % 5 == 0 && y % 3 == 0 {
                    page[y * w + x] = 0x00101010;
                }
            }
        }
        let calm = busyness(&gradient, w, h);
        let busy = busyness(&page, w, h);
        assert!(busy > calm, "text-like {busy} should beat gradient {calm}");
    }

    #[test]
    fn a_degenerate_region_scores_zero_rather_than_panicking() {
        assert_eq!(busyness(&[], 0, 0), 0);
        assert_eq!(busyness(&[1, 2, 3], 1, 1), 0);
        assert_eq!(busyness(&[1, 2], 5, 5), 0, "buffer smaller than claimed size");
    }
}
