//! Every sprite a package references, decoded once into premultiplied BGRA.
//!
//! The webview renderer handed 54 PNGs across the IPC boundary as base64 data
//! URLs and let the browser decode them. Drawing natively, decoding happens
//! here instead — once, at boot, straight into the exact pixel format
//! `UpdateLayeredWindow` wants, so a frame is a memcpy-and-blend rather than a
//! format conversion.

use std::collections::HashMap;

use deskfolk_package::CharacterPackage;

use crate::canvas::Px;

pub struct Sprite {
    pub w: u32,
    pub h: u32,
    pub px: Vec<Px>,
}

#[derive(Default)]
pub struct Sprites {
    map: HashMap<String, Sprite>,
    bytes: usize,
}

impl Sprites {
    /// Decode everything the manifest references.
    ///
    /// A sprite that fails to decode costs that sprite, not the character —
    /// the same rule the webview renderer used. `compose` already skips
    /// anything it cannot measure, so a missing entry renders as absence.
    pub fn load(pkg: &CharacterPackage) -> Self {
        let mut map = HashMap::new();
        let mut bytes = 0;
        for name in pkg.referenced_sprites() {
            let path = pkg.sprite_path(&name);
            match image::open(&path) {
                Ok(img) => {
                    let rgba = img.to_rgba8();
                    let (w, h) = rgba.dimensions();
                    let mut px = Vec::with_capacity((w * h) as usize);
                    for p in rgba.pixels() {
                        px.push(premultiply(p.0[0], p.0[1], p.0[2], p.0[3]));
                    }
                    bytes += px.len() * 4;
                    map.insert(name, Sprite { w, h, px });
                }
                Err(e) => tracing::warn!("sprite {} failed to decode: {e}", path.display()),
            }
        }
        Self { map, bytes }
    }

    pub fn get(&self, name: &str) -> Option<&Sprite> {
        self.map.get(name)
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn bytes(&self) -> usize {
        self.bytes
    }
}

/// Straight RGBA (what PNG stores) to premultiplied BGRA (what the layered
/// window requires).
#[inline]
pub fn premultiply(r: u8, g: u8, b: u8, a: u8) -> Px {
    if a == 0 {
        return 0;
    }
    if a == 255 {
        return (0xff << 24) | ((r as u32) << 16) | ((g as u32) << 8) | b as u32;
    }
    let m = |c: u8| ((c as u32 * a as u32 + 127) / 255) & 0xff;
    ((a as u32) << 24) | (m(r) << 16) | (m(g) << 8) | m(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opaque_pixels_keep_their_colour() {
        let p = premultiply(10, 20, 30, 255);
        assert_eq!(p, 0xFF0A141E);
    }

    #[test]
    fn fully_transparent_pixels_collapse_to_zero() {
        // Pixel art PNGs routinely carry stale colour under alpha 0. Left
        // unmultiplied it would show as a coloured fringe on the desktop.
        assert_eq!(premultiply(255, 0, 0, 0), 0);
    }

    #[test]
    fn half_alpha_halves_the_channels() {
        let p = premultiply(200, 100, 50, 128);
        assert_eq!(p >> 24, 128);
        assert_eq!((p >> 16) & 0xff, 100);
        assert_eq!((p >> 8) & 0xff, 50);
        assert_eq!(p & 0xff, 25);
    }

    #[test]
    fn channel_order_is_bgra_in_memory() {
        // A 32bpp BI_RGB DIB reads bytes as B, G, R, A. Getting this backwards
        // swaps red and blue on the whole character.
        let p = premultiply(0xAA, 0xBB, 0xCC, 0xFF);
        let bytes = p.to_le_bytes();
        assert_eq!(bytes, [0xCC, 0xBB, 0xAA, 0xFF]);
    }
}
