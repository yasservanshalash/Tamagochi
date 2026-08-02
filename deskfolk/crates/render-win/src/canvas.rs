//! A premultiplied-BGRA pixel buffer and the handful of operations the
//! companion needs to draw itself.
//!
//! Everything here is plain arithmetic on a `&mut [u32]` — no Win32, no GDI —
//! so the compositing rules are unit-testable on any machine. The Windows side
//! only supplies the buffer (a DIB section) and hands the result to
//! `UpdateLayeredWindow`.
//!
//! **Premultiplied** is not a style choice: `UpdateLayeredWindow` with
//! `AC_SRC_ALPHA` requires it. Feeding it straight (unassociated) alpha makes
//! every semi-transparent edge glow — the classic bright halo around a sprite.

/// One pixel: `0xAARRGGBB`, which little-endian byte order lays out as
/// B, G, R, A — exactly what a 32bpp `BI_RGB` DIB expects.
pub type Px = u32;

#[inline]
pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Px {
    // Premultiply at construction so callers cannot forget.
    let a32 = a as u32;
    let rr = (r as u32 * a32 + 127) / 255;
    let gg = (g as u32 * a32 + 127) / 255;
    let bb = (b as u32 * a32 + 127) / 255;
    (a32 << 24) | (rr << 16) | (gg << 8) | bb
}

#[inline]
fn mul255(a: u32, b: u32) -> u32 {
    (a * b + 127) / 255
}

/// Source-over, both sides premultiplied.
#[inline]
pub fn over(dst: Px, src: Px) -> Px {
    let sa = src >> 24;
    if sa == 255 {
        return src;
    }
    if sa == 0 {
        return dst;
    }
    let inv = 255 - sa;
    let ch = |shift: u32| {
        let d = (dst >> shift) & 0xff;
        let s = (src >> shift) & 0xff;
        (s + mul255(d, inv)).min(255) << shift
    };
    ch(24) | ch(16) | ch(8) | ch(0)
}

/// Scale a premultiplied pixel's alpha (and therefore its colour).
#[inline]
pub fn scale_alpha(px: Px, alpha: u8) -> Px {
    if alpha == 255 {
        return px;
    }
    let a = alpha as u32;
    let ch = |shift: u32| mul255((px >> shift) & 0xff, a) << shift;
    ch(24) | ch(16) | ch(8) | ch(0)
}

pub struct Canvas<'a> {
    pub px: &'a mut [Px],
    pub w: i32,
    pub h: i32,
}

impl<'a> Canvas<'a> {
    pub fn new(px: &'a mut [Px], w: i32, h: i32) -> Self {
        debug_assert!(px.len() >= (w.max(0) as usize) * (h.max(0) as usize));
        Self { px, w, h }
    }

    pub fn clear(&mut self) {
        self.px.fill(0);
    }

    #[inline]
    pub fn get(&self, x: i32, y: i32) -> Px {
        if x < 0 || y < 0 || x >= self.w || y >= self.h {
            return 0;
        }
        self.px[(y as usize) * (self.w as usize) + x as usize]
    }

    #[inline]
    pub fn blend(&mut self, x: i32, y: i32, src: Px) {
        if x < 0 || y < 0 || x >= self.w || y >= self.h || src >> 24 == 0 {
            return;
        }
        let i = (y as usize) * (self.w as usize) + x as usize;
        self.px[i] = over(self.px[i], src);
    }

    /// Nearest-neighbour blit of a premultiplied source image into `dst`.
    ///
    /// Nearest-neighbour is mandatory, not lazy: this is pixel art, and any
    /// interpolation turns a crisp 2px outline into mush. It is the same
    /// `imageSmoothingEnabled = false` the canvas renderer relied on.
    pub fn blit_scaled(
        &mut self,
        src: &[Px],
        sw: u32,
        sh: u32,
        dx: i32,
        dy: i32,
        dw: i32,
        dh: i32,
        alpha: u8,
    ) {
        if sw == 0 || sh == 0 || dw <= 0 || dh <= 0 || alpha == 0 {
            return;
        }
        // Clip to the canvas before touching a pixel, so an off-stage sprite
        // costs nothing instead of looping over rows it cannot draw.
        let x0 = dx.max(0);
        let y0 = dy.max(0);
        let x1 = (dx + dw).min(self.w);
        let y1 = (dy + dh).min(self.h);
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        for y in y0..y1 {
            let sy = (((y - dy) as i64 * sh as i64) / dh as i64) as usize;
            let sy = sy.min(sh as usize - 1);
            let row = sy * sw as usize;
            for x in x0..x1 {
                let sx = (((x - dx) as i64 * sw as i64) / dw as i64) as usize;
                let sx = sx.min(sw as usize - 1);
                let s = src[row + sx];
                if s >> 24 == 0 {
                    continue;
                }
                let s = scale_alpha(s, alpha);
                let i = (y as usize) * (self.w as usize) + x as usize;
                self.px[i] = over(self.px[i], s);
            }
        }
    }

    /// Composite a solid colour through an 8-bit coverage map — how text gets
    /// onto the canvas. GDI cannot draw text into an alpha channel, so glyphs
    /// are rendered white-on-black elsewhere and their brightness becomes
    /// coverage here.
    pub fn blit_coverage(
        &mut self,
        cov: &[u8],
        cw: i32,
        ch: i32,
        dx: i32,
        dy: i32,
        color: Px,
    ) {
        let base_a = color >> 24;
        if base_a == 0 {
            return;
        }
        for y in 0..ch {
            let ty = dy + y;
            if ty < 0 || ty >= self.h {
                continue;
            }
            for x in 0..cw {
                let tx = dx + x;
                if tx < 0 || tx >= self.w {
                    continue;
                }
                let c = cov[(y as usize) * (cw as usize) + x as usize];
                if c == 0 {
                    continue;
                }
                let s = scale_alpha(color, c);
                let i = (ty as usize) * (self.w as usize) + tx as usize;
                self.px[i] = over(self.px[i], s);
            }
        }
    }

    /// Anti-aliased rounded rectangle.
    ///
    /// Coverage comes from a signed distance to the rounded box rather than
    /// from GDI: a hard-edged bubble looks like a dialog, and `RoundRect`
    /// would flatten the alpha channel we depend on.
    pub fn fill_round_rect(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        radius: f32,
        color: Px,
    ) {
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        let r = radius.min(w / 2.0).min(h / 2.0).max(0.0);
        let (cx, cy) = (x + w / 2.0, y + h / 2.0);
        let (hx, hy) = (w / 2.0 - r, h / 2.0 - r);

        let x0 = (x.floor() as i32 - 1).max(0);
        let y0 = (y.floor() as i32 - 1).max(0);
        let x1 = ((x + w).ceil() as i32 + 1).min(self.w);
        let y1 = ((y + h).ceil() as i32 + 1).min(self.h);

        for py in y0..y1 {
            for px in x0..x1 {
                // Distance from the pixel centre to the rounded box.
                let dx = ((px as f32 + 0.5) - cx).abs() - hx;
                let dy = ((py as f32 + 0.5) - cy).abs() - hy;
                let d = if dx > 0.0 && dy > 0.0 {
                    (dx * dx + dy * dy).sqrt() - r
                } else {
                    dx.max(dy) - r
                };
                // One pixel of feathering across the boundary.
                let cov = (0.5 - d).clamp(0.0, 1.0);
                if cov <= 0.0 {
                    continue;
                }
                let a = (cov * 255.0).round() as u8;
                self.blend(px, py, scale_alpha(color, a));
            }
        }
    }

    /// A filled parallelogram: a rectangle whose top edge is pushed `skew`
    /// pixels right of its bottom edge.
    ///
    /// The slant is the whole visual identity of the floating menu — square
    /// panels read as a dialog, leaning ones read as a card thrown onto the
    /// screen. Rows are filled individually with the ends anti-aliased, which
    /// is enough at these sizes and avoids a general polygon rasteriser.
    pub fn fill_skewed(&mut self, x: f32, y: f32, w: f32, h: f32, skew: f32, color: Px) {
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        let y0 = (y.floor() as i32).max(0);
        let y1 = ((y + h).ceil() as i32).min(self.h);
        for py in y0..y1 {
            let centre = py as f32 + 0.5;
            // Vertical coverage, so the top and bottom edges are not jagged.
            let vcov = ((centre - y + 0.5).clamp(0.0, 1.0))
                .min((y + h - centre + 0.5).clamp(0.0, 1.0));
            if vcov <= 0.0 {
                continue;
            }
            let frac = ((centre - y) / h).clamp(0.0, 1.0);
            let left = x + skew * (1.0 - frac);
            let right = left + w;
            let px0 = (left.floor() as i32).max(0);
            let px1 = ((right).ceil() as i32).min(self.w);
            for px in px0..px1 {
                let c = px as f32 + 0.5;
                let hcov = ((c - left + 0.5).clamp(0.0, 1.0))
                    .min((right - c + 0.5).clamp(0.0, 1.0));
                let cov = hcov * vcov;
                if cov <= 0.0 {
                    continue;
                }
                self.blend(px, py, scale_alpha(color, (cov * 255.0).round() as u8));
            }
        }
    }

    /// Outline of the same parallelogram.
    pub fn stroke_skewed(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        skew: f32,
        thickness: f32,
        color: Px,
    ) {
        let (tl, tr) = ((x + skew, y), (x + skew + w, y));
        let (bl, br) = ((x, y + h), (x + w, y + h));
        self.stroke_line(tl.0, tl.1, tr.0, tr.1, thickness, color);
        self.stroke_line(bl.0, bl.1, br.0, br.1, thickness, color);
        self.stroke_line(tl.0, tl.1, bl.0, bl.1, thickness, color);
        self.stroke_line(tr.0, tr.1, br.0, br.1, thickness, color);
    }

    /// Composite a coverage map with the same lean as the chip it sits on, so
    /// the label belongs to the card instead of floating on top of it.
    pub fn blit_coverage_skewed(
        &mut self,
        cov: &[u8],
        cw: i32,
        ch: i32,
        dx: f32,
        dy: f32,
        skew: f32,
        color: Px,
    ) {
        if color >> 24 == 0 || ch <= 0 || cw <= 0 {
            return;
        }
        for y in 0..ch {
            let ty = dy + y as f32;
            let row = ty.round() as i32;
            if row < 0 || row >= self.h {
                continue;
            }
            let frac = y as f32 / ch as f32;
            let xo = dx + skew * (1.0 - frac);
            for x in 0..cw {
                let c = cov[(y as usize) * (cw as usize) + x as usize];
                if c == 0 {
                    continue;
                }
                let tx = (xo + x as f32).round() as i32;
                if tx < 0 || tx >= self.w {
                    continue;
                }
                let i = (row as usize) * (self.w as usize) + tx as usize;
                self.px[i] = over(self.px[i], scale_alpha(color, c));
            }
        }
    }

    /// Anti-aliased line segment with round caps.
    ///
    /// Every glyph the menu draws — chevrons, ticks, the little icons — is
    /// made of these, so they scale with DPI instead of being pinned to a
    /// bitmap at one size.
    pub fn stroke_line(
        &mut self,
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        thickness: f32,
        color: Px,
    ) {
        if thickness <= 0.0 {
            return;
        }
        let half = thickness / 2.0;
        let pad = half + 1.0;
        let lo_x = (x0.min(x1) - pad).floor().max(0.0) as i32;
        let hi_x = ((x0.max(x1) + pad).ceil() as i32).min(self.w);
        let lo_y = (y0.min(y1) - pad).floor().max(0.0) as i32;
        let hi_y = ((y0.max(y1) + pad).ceil() as i32).min(self.h);

        let (dx, dy) = (x1 - x0, y1 - y0);
        let len_sq = dx * dx + dy * dy;

        for py in lo_y..hi_y {
            for px in lo_x..hi_x {
                let (fx, fy) = (px as f32 + 0.5, py as f32 + 0.5);
                // Distance from the pixel to the segment.
                let t = if len_sq <= f32::EPSILON {
                    0.0
                } else {
                    (((fx - x0) * dx + (fy - y0) * dy) / len_sq).clamp(0.0, 1.0)
                };
                let (nx, ny) = (x0 + dx * t, y0 + dy * t);
                let d = ((fx - nx).powi(2) + (fy - ny).powi(2)).sqrt();
                let cov = (0.5 - (d - half)).clamp(0.0, 1.0);
                if cov <= 0.0 {
                    continue;
                }
                self.blend(px, py, scale_alpha(color, (cov * 255.0).round() as u8));
            }
        }
    }

    /// Outline of a rounded rectangle, `thickness` device pixels wide.
    pub fn stroke_round_rect(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        radius: f32,
        thickness: f32,
        color: Px,
    ) {
        if w <= 0.0 || h <= 0.0 || thickness <= 0.0 {
            return;
        }
        let r = radius.min(w / 2.0).min(h / 2.0).max(0.0);
        let (cx, cy) = (x + w / 2.0, y + h / 2.0);
        let (hx, hy) = (w / 2.0 - r, h / 2.0 - r);
        let half = thickness / 2.0;

        let x0 = (x.floor() as i32 - 2).max(0);
        let y0 = (y.floor() as i32 - 2).max(0);
        let x1 = ((x + w).ceil() as i32 + 2).min(self.w);
        let y1 = ((y + h).ceil() as i32 + 2).min(self.h);

        for py in y0..y1 {
            for px in x0..x1 {
                let dx = ((px as f32 + 0.5) - cx).abs() - hx;
                let dy = ((py as f32 + 0.5) - cy).abs() - hy;
                let d = if dx > 0.0 && dy > 0.0 {
                    (dx * dx + dy * dy).sqrt() - r
                } else {
                    dx.max(dy) - r
                };
                // Distance to the *edge*, so the stroke straddles it.
                let cov = (0.5 - (d.abs() - half)).clamp(0.0, 1.0);
                if cov <= 0.0 {
                    continue;
                }
                let a = (cov * 255.0).round() as u8;
                self.blend(px, py, scale_alpha(color, a));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn canvas(w: i32, h: i32) -> Vec<Px> {
        vec![0; (w * h) as usize]
    }

    #[test]
    fn rgba_premultiplies() {
        // Half-transparent white must store half-bright colour, or the
        // layered window paints a glowing halo around every soft edge.
        let p = rgba(255, 255, 255, 128);
        assert_eq!(p >> 24, 128);
        assert_eq!(p & 0xff, 128);
        assert_eq!((p >> 8) & 0xff, 128);
        assert_eq!((p >> 16) & 0xff, 128);
    }

    #[test]
    fn opaque_source_replaces_destination() {
        let dst = rgba(255, 0, 0, 255);
        let src = rgba(0, 255, 0, 255);
        assert_eq!(over(dst, src), src);
    }

    #[test]
    fn transparent_source_leaves_destination_alone() {
        let dst = rgba(255, 0, 0, 255);
        assert_eq!(over(dst, 0), dst);
    }

    #[test]
    fn half_alpha_over_opaque_stays_opaque() {
        // Compositing onto a solid pixel must never *reduce* alpha; if it did,
        // the character would go see-through wherever the bubble overlaps him.
        let dst = rgba(0, 0, 0, 255);
        let out = over(dst, rgba(255, 255, 255, 128));
        assert_eq!(out >> 24, 255);
    }

    #[test]
    fn alpha_accumulates_over_transparent_background() {
        let out = over(0, rgba(255, 255, 255, 128));
        assert_eq!(out >> 24, 128);
        let out2 = over(out, rgba(255, 255, 255, 128));
        assert!(out2 >> 24 > 128, "a second layer should be more opaque");
    }

    #[test]
    fn blit_scales_a_sprite_up() {
        let mut buf = canvas(4, 4);
        let mut c = Canvas::new(&mut buf, 4, 4);
        let src = vec![rgba(255, 0, 0, 255)];
        c.blit_scaled(&src, 1, 1, 0, 0, 4, 4, 255);
        for y in 0..4 {
            for x in 0..4 {
                assert_eq!(c.get(x, y) >> 24, 255, "pixel {x},{y} should be covered");
            }
        }
    }

    #[test]
    fn blit_clips_instead_of_panicking() {
        // A sprite drawn partly off-stage is routine — he leans, and fx hangs
        // past the edge. Writing outside the DIB would be memory corruption.
        let mut buf = canvas(4, 4);
        let mut c = Canvas::new(&mut buf, 4, 4);
        let src = vec![rgba(0, 255, 0, 255); 16];
        c.blit_scaled(&src, 4, 4, -2, -2, 4, 4, 255);
        c.blit_scaled(&src, 4, 4, 3, 3, 4, 4, 255);
        assert_eq!(c.get(0, 0) >> 24, 255);
        assert_eq!(c.get(3, 3) >> 24, 255);
    }

    #[test]
    fn fully_off_canvas_blit_draws_nothing() {
        let mut buf = canvas(4, 4);
        let mut c = Canvas::new(&mut buf, 4, 4);
        let src = vec![rgba(0, 255, 0, 255); 16];
        c.blit_scaled(&src, 4, 4, 100, 100, 4, 4, 255);
        assert!(buf.iter().all(|p| *p == 0));
    }

    #[test]
    fn blit_alpha_fades_the_whole_sprite() {
        // The glitch overlay is drawn at 75%; at 0 it must vanish entirely.
        let mut buf = canvas(2, 2);
        let mut c = Canvas::new(&mut buf, 2, 2);
        let src = vec![rgba(255, 255, 255, 255); 4];
        c.blit_scaled(&src, 2, 2, 0, 0, 2, 2, 128);
        assert_eq!(c.get(0, 0) >> 24, 128);
    }

    #[test]
    fn round_rect_fills_the_middle_and_misses_the_corner() {
        let mut buf = canvas(20, 20);
        let mut c = Canvas::new(&mut buf, 20, 20);
        c.fill_round_rect(0.0, 0.0, 20.0, 20.0, 8.0, rgba(255, 255, 255, 255));
        assert_eq!(c.get(10, 10) >> 24, 255, "centre is filled");
        assert_eq!(c.get(0, 0) >> 24, 0, "corner is rounded away");
    }

    #[test]
    fn coverage_map_paints_only_where_it_is_set() {
        let mut buf = canvas(4, 4);
        let mut c = Canvas::new(&mut buf, 4, 4);
        let cov = vec![0u8, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        c.blit_coverage(&cov, 4, 4, 0, 0, rgba(255, 255, 255, 255));
        assert_eq!(c.get(0, 0) >> 24, 0);
        assert_eq!(c.get(1, 0) >> 24, 255);
    }

    #[test]
    fn a_skewed_fill_leans_the_way_it_is_told() {
        // Top edge pushed right of the bottom edge: the lean that makes the
        // menu read as a thrown card rather than a dialog.
        let mut buf = canvas(40, 20);
        let mut c = Canvas::new(&mut buf, 40, 20);
        c.fill_skewed(4.0, 2.0, 16.0, 16.0, 10.0, rgba(255, 255, 255, 255));
        assert!(c.get(16, 3) >> 24 > 200, "top row sits right");
        assert_eq!(c.get(6, 3) >> 24, 0, "and not left");
        assert!(c.get(6, 16) >> 24 > 200, "bottom row sits left");
        assert_eq!(c.get(24, 16) >> 24, 0, "and not right");
    }

    #[test]
    fn a_skewed_fill_with_no_skew_is_just_a_rectangle() {
        let mut buf = canvas(20, 20);
        let mut c = Canvas::new(&mut buf, 20, 20);
        c.fill_skewed(5.0, 5.0, 10.0, 10.0, 0.0, rgba(255, 255, 255, 255));
        assert!(c.get(10, 6) >> 24 > 200);
        assert!(c.get(6, 14) >> 24 > 200);
        assert_eq!(c.get(2, 10) >> 24, 0);
    }

    #[test]
    fn a_skewed_fill_off_canvas_is_clipped_not_fatal() {
        let mut buf = canvas(8, 8);
        let mut c = Canvas::new(&mut buf, 8, 8);
        c.fill_skewed(-40.0, -40.0, 10.0, 10.0, 6.0, rgba(255, 255, 255, 255));
        c.fill_skewed(100.0, 2.0, 10.0, 4.0, 6.0, rgba(255, 255, 255, 255));
        assert!(buf.iter().all(|p| *p == 0));
    }

    #[test]
    fn a_line_marks_its_own_path_and_nothing_else() {
        let mut buf = canvas(16, 16);
        let mut c = Canvas::new(&mut buf, 16, 16);
        c.stroke_line(2.0, 8.0, 14.0, 8.0, 2.0, rgba(255, 255, 255, 255));
        assert!(c.get(8, 8) >> 24 > 200, "the line itself");
        assert_eq!(c.get(8, 2) >> 24, 0, "well clear of it");
    }

    #[test]
    fn a_zero_length_line_is_a_dot_not_a_panic() {
        // Chevrons collapse to a point mid-rotation; that must not divide by
        // zero or paint the whole canvas.
        let mut buf = canvas(8, 8);
        let mut c = Canvas::new(&mut buf, 8, 8);
        c.stroke_line(4.0, 4.0, 4.0, 4.0, 2.0, rgba(255, 255, 255, 255));
        assert!(c.get(4, 4) >> 24 > 0);
        assert_eq!(c.get(0, 0) >> 24, 0);
    }

    #[test]
    fn a_line_off_the_canvas_draws_nothing() {
        let mut buf = canvas(8, 8);
        let mut c = Canvas::new(&mut buf, 8, 8);
        c.stroke_line(-50.0, -50.0, -40.0, -40.0, 2.0, rgba(255, 255, 255, 255));
        assert!(buf.iter().all(|p| *p == 0));
    }

    #[test]
    fn drawing_outside_the_canvas_is_ignored() {
        let mut buf = canvas(2, 2);
        let mut c = Canvas::new(&mut buf, 2, 2);
        c.blend(-1, 0, rgba(255, 255, 255, 255));
        c.blend(0, 99, rgba(255, 255, 255, 255));
        assert_eq!(c.get(0, 0), 0);
    }
}
