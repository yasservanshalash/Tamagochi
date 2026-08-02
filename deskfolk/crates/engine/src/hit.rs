//! Where the window should catch clicks, and where they should fall through
//! to the desktop underneath.
//!
//! The portal shape decides the rule. In a framed portal the whole frame is
//! solid and the test is simple geometry. **Untethered** — the character
//! standing directly on the desktop with no frame at all — is the interesting
//! case and the one the design leads with: only his actual pixels may catch a
//! click, or he becomes an invisible rectangle sitting on top of your icons.

use deskfolk_package::{SpriteMasks, Stage};
use serde::{Deserialize, Serialize};

use crate::layout::{Composition, Rect};

/// How the companion is framed on the desktop. Straight from the design doc's
/// portal options.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Portal {
    /// No frame: he simply stands on the desktop. Clicks land only on him.
    Untethered,
    /// The alpha's round window.
    Circle,
    /// A soft-cornered rectangle.
    Rounded { radius: u32 },
}

impl Default for Portal {
    fn default() -> Self {
        Portal::Untethered
    }
}

/// Should the window swallow a click at this stage-space point?
///
/// `ui_rects` are interactive overlays the engine doesn't own pixel data for
/// — the speech bubble, close button and so on. They always catch clicks.
pub fn hit_test(
    portal: Portal,
    stage: &Stage,
    comp: &Composition,
    masks: &SpriteMasks,
    ui_rects: &[Rect],
    x: i32,
    y: i32,
) -> bool {
    if ui_rects.iter().any(|r| r.contains(x, y)) {
        return true;
    }

    match portal {
        Portal::Circle => {
            let (cx, cy) = (stage.width as f64 / 2.0, stage.height as f64 / 2.0);
            let r = (stage.width.min(stage.height) as f64) / 2.0;
            let (dx, dy) = (x as f64 - cx, y as f64 - cy);
            dx * dx + dy * dy <= r * r
        }
        Portal::Rounded { radius } => rounded_rect_contains(stage, radius, x, y),
        Portal::Untethered => sprite_hit(comp, masks, x, y),
    }
}

/// True only over a solid pixel of something actually drawn.
fn sprite_hit(comp: &Composition, masks: &SpriteMasks, x: i32, y: i32) -> bool {
    for placed in [comp.character.as_ref(), comp.fx.as_ref()].into_iter().flatten() {
        if !placed.rect.contains(x, y) {
            continue;
        }
        let Some(mask) = masks.get(&placed.sprite) else {
            // No mask for a sprite that is nonetheless on screen: treat its
            // bounding box as solid rather than making him unclickable.
            return true;
        };
        if mask.opaque_at(x - placed.rect.x, y - placed.rect.y) {
            return true;
        }
    }
    false
}

fn rounded_rect_contains(stage: &Stage, radius: u32, x: i32, y: i32) -> bool {
    let (w, h) = (stage.width as i32, stage.height as i32);
    if x < 0 || y < 0 || x >= w || y >= h {
        return false;
    }
    let r = (radius as i32).min(w / 2).min(h / 2);
    if r <= 0 {
        return true;
    }
    // Only the four corner boxes need a distance check.
    let cx = if x < r {
        r
    } else if x >= w - r {
        w - r - 1
    } else {
        return true;
    };
    let cy = if y < r {
        r
    } else if y >= h - r {
        h - r - 1
    } else {
        return true;
    };
    let (dx, dy) = ((x - cx) as f64, (y - cy) as f64);
    dx * dx + dy * dy <= (r as f64) * (r as f64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::Placed;

    fn stage() -> Stage {
        Stage {
            width: 400,
            height: 400,
            anchor_x: 200,
            anchor_y: 380,
            fx_dx: 44,
            fx_dy: 34,
            glitch_center: None,
        }
    }

    fn empty_comp() -> Composition {
        Composition { character: None, fx: None, glitch: None }
    }

    fn comp_with(sprite: &str, rect: Rect) -> Composition {
        Composition {
            character: Some(Placed { sprite: sprite.into(), rect }),
            fx: None,
            glitch: None,
        }
    }

    #[test]
    fn circle_portal_catches_the_middle_and_frees_the_corners() {
        let s = stage();
        let c = empty_comp();
        let m = SpriteMasks::default();
        assert!(hit_test(Portal::Circle, &s, &c, &m, &[], 200, 200));
        assert!(
            !hit_test(Portal::Circle, &s, &c, &m, &[], 5, 5),
            "corners outside the circle must fall through to the desktop"
        );
        assert!(!hit_test(Portal::Circle, &s, &c, &m, &[], 395, 395));
    }

    #[test]
    fn rounded_portal_frees_only_the_corners() {
        let s = stage();
        let c = empty_comp();
        let m = SpriteMasks::default();
        let p = Portal::Rounded { radius: 40 };
        assert!(hit_test(p, &s, &c, &m, &[], 200, 200));
        assert!(hit_test(p, &s, &c, &m, &[], 200, 2), "top edge is still solid");
        assert!(hit_test(p, &s, &c, &m, &[], 2, 200), "left edge is still solid");
        assert!(!hit_test(p, &s, &c, &m, &[], 1, 1), "rounded corner falls through");
    }

    #[test]
    fn untethered_with_nothing_drawn_catches_nothing() {
        // The whole point: an empty desktop companion must not be an
        // invisible rectangle sitting on the user's icons.
        let s = stage();
        let c = empty_comp();
        let m = SpriteMasks::default();
        for (x, y) in [(0, 0), (200, 200), (399, 399), (50, 380)] {
            assert!(
                !hit_test(Portal::Untethered, &s, &c, &m, &[], x, y),
                "({x},{y}) should have fallen through"
            );
        }
    }

    #[test]
    fn untethered_falls_back_to_the_bounding_box_when_a_mask_is_missing() {
        // Better a slightly greedy hitbox than a character you cannot click.
        let s = stage();
        let c = comp_with("unmasked", Rect { x: 100, y: 100, w: 50, h: 50 });
        let m = SpriteMasks::default();
        assert!(hit_test(Portal::Untethered, &s, &c, &m, &[], 120, 120));
        assert!(!hit_test(Portal::Untethered, &s, &c, &m, &[], 20, 20));
    }

    #[test]
    fn ui_overlays_always_catch_clicks() {
        let s = stage();
        let c = empty_comp();
        let m = SpriteMasks::default();
        let bubble = [Rect { x: 40, y: 300, w: 200, h: 60 }];
        assert!(hit_test(Portal::Untethered, &s, &c, &m, &bubble, 100, 320));
        assert!(!hit_test(Portal::Untethered, &s, &c, &m, &bubble, 100, 100));
    }

    #[test]
    fn hit_testing_far_outside_the_stage_is_safe() {
        // The cursor poll runs constantly, including when the pointer is
        // nowhere near the window.
        let s = stage();
        let c = comp_with("x", Rect { x: 0, y: 0, w: 10, h: 10 });
        let m = SpriteMasks::default();
        for (x, y) in [(-5000, -5000), (i32::MAX, 0), (0, i32::MIN)] {
            let _ = hit_test(Portal::Untethered, &s, &c, &m, &[], x, y);
            let _ = hit_test(Portal::Circle, &s, &c, &m, &[], x, y);
            let _ = hit_test(Portal::Rounded { radius: 20 }, &s, &c, &m, &[], x, y);
        }
    }
}
