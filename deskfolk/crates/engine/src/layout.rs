//! Turning a logical frame into actual rectangles.
//!
//! This is deliberately the *only* place that knows how a sprite maps onto the
//! stage. Both the renderer and the click-through hit-test consume the result,
//! so they cannot drift apart — if the character is drawn two pixels left of
//! where clicks land, he becomes subtly unclickable, and that class of bug is
//! miserable to chase from a screenshot.
//!
//! Positions come out in **stage units**. Scaling to physical pixels happens
//! once, at the edge, so a package authored for a 412×412 stage works at any
//! display scale.

use deskfolk_package::Stage;
use serde::{Deserialize, Serialize};

use crate::FrameState;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

impl Rect {
    #[inline]
    pub fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x
            && py >= self.y
            && px < self.x + self.w as i32
            && py < self.y + self.h as i32
    }
}

/// One sprite, placed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Placed {
    pub sprite: String,
    pub rect: Rect,
}

/// Everything to draw this frame, already positioned.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Composition {
    pub character: Option<Placed>,
    pub fx: Option<Placed>,
    pub glitch: Option<Placed>,
}

/// Anything that can report a sprite's pixel dimensions.
pub trait SpriteDims {
    fn dims(&self, name: &str) -> Option<(u32, u32)>;
}

impl SpriteDims for deskfolk_package::SpriteMasks {
    fn dims(&self, name: &str) -> Option<(u32, u32)> {
        self.get(name).map(|m| (m.width, m.height))
    }
}

impl SpriteDims for std::collections::HashMap<String, (u32, u32)> {
    fn dims(&self, name: &str) -> Option<(u32, u32)> {
        self.get(name).copied()
    }
}

/// Place a sprite bottom-center anchored at the stage anchor, nudged by the
/// frame's `dx`/`dy`. Sprites of different heights therefore share a floor,
/// which is what stops a character bobbing when clips swap.
pub fn character_rect(stage: &Stage, dx: i32, dy: i32, w: u32, h: u32) -> Rect {
    Rect {
        x: stage.anchor_x + dx - (w as i32) / 2,
        y: stage.anchor_y - dy - h as i32,
        w,
        h,
    }
}

/// Compose a full frame. Missing sprites are skipped rather than faked, so a
/// broken package renders partially instead of panicking.
pub fn compose(frame: &FrameState, stage: &Stage, dims: &impl SpriteDims) -> Composition {
    let character = dims.dims(&frame.sprite).map(|(w, h)| Placed {
        sprite: frame.sprite.clone(),
        rect: character_rect(stage, frame.dx, frame.dy, w, h),
    });

    // FX hangs beside the head, positioned relative to the character's own
    // top edge so it tracks tall and short sprites alike.
    let fx = frame.fx.as_ref().and_then(|name| {
        let (fw, fh) = dims.dims(name)?;
        let ch = character.as_ref()?;
        Some(Placed {
            sprite: name.clone(),
            rect: Rect {
                x: ch.rect.x + ch.rect.w as i32 / 2 + stage.fx_dx - fw as i32 / 2,
                y: ch.rect.y + stage.fx_dy - fh as i32,
                w: fw,
                h: fh,
            },
        })
    });

    let glitch = frame.glitch_fx.as_ref().and_then(|name| {
        let (gw, gh) = dims.dims(name)?;
        let [cx, cy] = stage
            .glitch_center
            .unwrap_or([stage.width as i32 / 2, stage.height as i32 / 2]);
        Some(Placed {
            sprite: name.clone(),
            rect: Rect {
                x: cx - gw as i32 / 2,
                y: cy - gh as i32 / 2,
                w: gw,
                h: gh,
            },
        })
    });

    Composition { character, fx, glitch }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn stage() -> Stage {
        Stage {
            width: 412,
            height: 412,
            anchor_x: 206,
            anchor_y: 386,
            fx_dx: 44,
            fx_dy: 34,
            glitch_center: Some([206, 190]),
        }
    }

    fn dims() -> HashMap<String, (u32, u32)> {
        HashMap::from([
            ("body".to_string(), (100, 200)),
            ("tall".to_string(), (100, 260)),
            ("heart".to_string(), (20, 20)),
            ("static".to_string(), (300, 300)),
        ])
    }

    fn frame(sprite: &str) -> FrameState {
        FrameState {
            sprite: sprite.into(),
            dx: 0,
            dy: 0,
            fx: None,
            glitch_fx: None,
            state: crate::State::Idle,
            subtitle: None,
            listening: false,
        }
    }

    #[test]
    fn character_sits_bottom_center_on_the_anchor() {
        let r = character_rect(&stage(), 0, 0, 100, 200);
        assert_eq!(r.x, 156, "centered on anchor_x 206");
        assert_eq!(r.y, 186, "bottom edge lands on anchor_y 386");
        assert_eq!(r.y + r.h as i32, 386);
    }

    #[test]
    fn sprites_of_different_heights_share_a_floor() {
        // If this breaks, the character visibly bobs whenever a clip changes.
        let short = character_rect(&stage(), 0, 0, 100, 200);
        let tall = character_rect(&stage(), 0, 0, 100, 260);
        assert_eq!(short.y + short.h as i32, tall.y + tall.h as i32);
    }

    #[test]
    fn frame_offsets_nudge_the_character() {
        let base = character_rect(&stage(), 0, 0, 100, 200);
        let moved = character_rect(&stage(), 5, 3, 100, 200);
        assert_eq!(moved.x, base.x + 5);
        // Positive dy lifts him, matching the firmware's convention.
        assert_eq!(moved.y, base.y - 3);
    }

    #[test]
    fn fx_hangs_beside_the_head() {
        let mut f = frame("body");
        f.fx = Some("heart".into());
        let c = compose(&f, &stage(), &dims());
        let ch = c.character.unwrap().rect;
        let fx = c.fx.expect("fx should be placed").rect;
        assert_eq!(fx.x, ch.x + 50 + 44 - 10);
        assert_eq!(fx.y, ch.y + 34 - 20);
    }

    #[test]
    fn fx_follows_a_taller_sprite_upward() {
        let mut f = frame("tall");
        f.fx = Some("heart".into());
        let c = compose(&f, &stage(), &dims());
        let ch = c.character.unwrap().rect;
        let fx = c.fx.unwrap().rect;
        assert_eq!(fx.y, ch.y + 34 - 20, "fx is relative to the head, not the stage");
    }

    #[test]
    fn glitch_overlay_is_centered_on_its_configured_point() {
        let mut f = frame("body");
        f.glitch_fx = Some("static".into());
        let c = compose(&f, &stage(), &dims());
        let g = c.glitch.unwrap().rect;
        assert_eq!(g.x + g.w as i32 / 2, 206);
        assert_eq!(g.y + g.h as i32 / 2, 190);
    }

    #[test]
    fn missing_sprites_are_skipped_not_faked() {
        let mut f = frame("does_not_exist");
        f.fx = Some("also_missing".into());
        let c = compose(&f, &stage(), &dims());
        assert!(c.character.is_none());
        assert!(c.fx.is_none(), "fx without a character has nothing to anchor to");
    }

    #[test]
    fn rect_contains_is_half_open() {
        let r = Rect { x: 10, y: 20, w: 5, h: 5 };
        assert!(r.contains(10, 20));
        assert!(r.contains(14, 24));
        assert!(!r.contains(15, 24), "right edge is exclusive");
        assert!(!r.contains(14, 25), "bottom edge is exclusive");
        assert!(!r.contains(9, 20));
    }
}
