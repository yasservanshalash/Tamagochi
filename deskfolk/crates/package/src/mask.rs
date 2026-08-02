//! Alpha hit-masks.
//!
//! Tauri exposes no per-region hit-testing: a window either swallows every
//! click inside its rectangle or passes all of them through. For a companion
//! standing untethered on the desktop that is the difference between a
//! character and a giant invisible box that eats your icons.
//!
//! So we do it ourselves. Every sprite's alpha channel is baked into a bitset
//! once at load, and the host polls the cursor and toggles
//! `set_ignore_cursor_events` based on whether it is over a solid pixel of the
//! *currently drawn* frame. Bitsets keep the whole character's mask set well
//! under a megabyte, so the lookup is a couple of shifts and an AND.

use std::collections::HashMap;

use crate::CharacterPackage;

/// Alpha at or above this counts as solid. Pixel art is normally 0 or 255, so
/// this only matters for soft-edged packages, where we prefer *not* to grab
/// clicks on a nearly-invisible edge.
pub const DEFAULT_ALPHA_THRESHOLD: u8 = 32;

/// A 1-bit-per-pixel opacity mask.
#[derive(Debug, Clone)]
pub struct AlphaMask {
    pub width: u32,
    pub height: u32,
    words: Vec<u64>,
}

impl AlphaMask {
    fn new(width: u32, height: u32) -> Self {
        let bits = width as usize * height as usize;
        Self { width, height, words: vec![0; bits.div_ceil(64)] }
    }

    #[inline]
    fn index(&self, x: u32, y: u32) -> usize {
        y as usize * self.width as usize + x as usize
    }

    #[inline]
    fn set(&mut self, x: u32, y: u32) {
        let i = self.index(x, y);
        self.words[i / 64] |= 1u64 << (i % 64);
    }

    /// Is this pixel solid? Out-of-bounds is never solid.
    #[inline]
    pub fn opaque_at(&self, x: i32, y: i32) -> bool {
        if x < 0 || y < 0 || x as u32 >= self.width || y as u32 >= self.height {
            return false;
        }
        let i = self.index(x as u32, y as u32);
        self.words[i / 64] & (1u64 << (i % 64)) != 0
    }

    pub fn is_empty(&self) -> bool {
        self.words.iter().all(|w| *w == 0)
    }

    /// Grow the mask outward by `radius` pixels.
    ///
    /// Purely a feel fix: hitting a 2px-wide arm exactly is fiddly, and a
    /// companion you keep failing to click is a companion you stop clicking.
    /// Uses a chebyshev (square) kernel — cheap, and the difference from a
    /// circular one is invisible at these radii.
    pub fn dilated(&self, radius: u32) -> Self {
        if radius == 0 {
            return self.clone();
        }
        let r = radius as i32;
        let mut out = Self::new(self.width, self.height);
        for y in 0..self.height as i32 {
            for x in 0..self.width as i32 {
                if !self.opaque_at(x, y) {
                    continue;
                }
                for dy in -r..=r {
                    for dx in -r..=r {
                        let (nx, ny) = (x + dx, y + dy);
                        if nx >= 0 && ny >= 0 && (nx as u32) < self.width && (ny as u32) < self.height
                        {
                            out.set(nx as u32, ny as u32);
                        }
                    }
                }
            }
        }
        out
    }
}

#[derive(Debug, thiserror::Error)]
pub enum MaskError {
    #[error("could not read sprite {path}: {source}")]
    Read { path: String, source: image::ImageError },
}

/// Every sprite in a package, as hit-masks.
#[derive(Debug, Clone, Default)]
pub struct SpriteMasks {
    masks: HashMap<String, AlphaMask>,
}

impl SpriteMasks {
    /// Bake masks for every sprite the manifest references.
    ///
    /// `grab_padding` dilates each mask so clicks just off the silhouette
    /// still land.
    pub fn load(
        pkg: &CharacterPackage,
        threshold: u8,
        grab_padding: u32,
    ) -> Result<Self, MaskError> {
        let mut masks = HashMap::new();
        for name in pkg.referenced_sprites() {
            let path = pkg.sprite_path(&name);
            let img = image::open(&path).map_err(|source| MaskError::Read {
                path: path.display().to_string(),
                source,
            })?;
            let rgba = img.to_rgba8();
            let (w, h) = rgba.dimensions();
            let mut m = AlphaMask::new(w, h);
            for (x, y, px) in rgba.enumerate_pixels() {
                if px.0[3] >= threshold {
                    m.set(x, y);
                }
            }
            let m = if grab_padding > 0 { m.dilated(grab_padding) } else { m };
            masks.insert(name, m);
        }
        Ok(Self { masks })
    }

    pub fn get(&self, name: &str) -> Option<&AlphaMask> {
        self.masks.get(name)
    }

    pub fn len(&self) -> usize {
        self.masks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.masks.is_empty()
    }

    /// Total mask memory, for the performance panel.
    pub fn bytes(&self) -> usize {
        self.masks.values().map(|m| m.words.len() * 8).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mask_from(rows: &[&str]) -> AlphaMask {
        let h = rows.len() as u32;
        let w = rows[0].len() as u32;
        let mut m = AlphaMask::new(w, h);
        for (y, row) in rows.iter().enumerate() {
            for (x, c) in row.chars().enumerate() {
                if c == '#' {
                    m.set(x as u32, y as u32);
                }
            }
        }
        m
    }

    #[test]
    fn reports_solid_and_empty_pixels() {
        let m = mask_from(&[".#.", "###", ".#."]);
        assert!(m.opaque_at(1, 0));
        assert!(m.opaque_at(0, 1));
        assert!(!m.opaque_at(0, 0), "corner is transparent");
        assert!(!m.opaque_at(2, 2));
    }

    #[test]
    fn out_of_bounds_is_never_solid() {
        // The cursor is polled against whatever frame is drawn, so it is
        // routinely outside a small sprite's bounds. That must not panic.
        let m = mask_from(&["##", "##"]);
        assert!(!m.opaque_at(-1, 0));
        assert!(!m.opaque_at(0, -1));
        assert!(!m.opaque_at(2, 0));
        assert!(!m.opaque_at(0, 99));
        assert!(!m.opaque_at(i32::MIN, i32::MAX));
    }

    #[test]
    fn dilation_grows_the_grabbable_area() {
        let m = mask_from(&[".....", ".....", "..#..", ".....", "....."]);
        let d = m.dilated(1);
        assert!(d.opaque_at(2, 2), "original pixel stays solid");
        assert!(d.opaque_at(1, 1), "diagonal neighbour becomes grabbable");
        assert!(d.opaque_at(3, 2));
        assert!(!d.opaque_at(0, 0), "should not grow beyond the radius");
    }

    #[test]
    fn dilation_clamps_at_the_edges() {
        let m = mask_from(&["#."]);
        let d = m.dilated(3);
        assert!(d.opaque_at(0, 0));
        assert!(d.opaque_at(1, 0));
        assert!(!d.opaque_at(2, 0), "must not read outside the sprite");
    }

    #[test]
    fn zero_radius_is_a_noop() {
        let m = mask_from(&[".#", "#."]);
        let d = m.dilated(0);
        assert!(d.opaque_at(1, 0) && d.opaque_at(0, 1));
        assert!(!d.opaque_at(0, 0) && !d.opaque_at(1, 1));
    }

    #[test]
    fn a_fully_transparent_sprite_grabs_nothing() {
        let m = mask_from(&["..", ".."]);
        assert!(m.is_empty());
        assert!(!m.dilated(2).opaque_at(0, 0), "dilating nothing yields nothing");
    }
}
