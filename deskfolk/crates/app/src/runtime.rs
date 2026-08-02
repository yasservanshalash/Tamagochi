//! The companion runtime: one engine, one window, one loop.
//!
//! The loop advances the engine and hands the window a frame when something
//! visible changed. It is deliberately a single thread holding a single lock
//! for a few microseconds at a time; multiple threads racing over window state
//! was not worth the complexity at this scale.
//!
//! What used to be here and is now gone: the per-frame click-through decision.
//! A webview window swallows every click in its rectangle or none of them, so
//! the host polled the cursor at 60Hz, tested it against the alpha mask of the
//! frame on screen, and toggled `set_ignore_cursor_events`. The layered window
//! hit-tests against its own alpha, per pixel, in the compositor — so that
//! entire mechanism, and the desync bug it kept growing, is simply deleted.

use deskfolk_engine::{compose, Engine, Inputs, State};
use deskfolk_package::{CharacterPackage, SpriteMasks, Stage};
use deskfolk_render_win::Frame;

/// Loop period. 60Hz keeps dragging and the character's reactions feeling
/// immediate; the work per iteration is small enough that this is still well
/// under a percent of a core when idle.
pub const LOOP_MS: u64 = 16;

/// Clicks landing within this many stage units of the silhouette still count.
/// Hitting a two-pixel-wide arm exactly is not fun.
pub const GRAB_PADDING: u32 = 2;

pub struct Runtime {
    pub engine: Engine,
    /// Sprite dimensions for `compose`. The renderer decodes its own pixels;
    /// these masks are what tells the layout how big each sprite is.
    pub masks: SpriteMasks,
    /// Stage units to logical pixels.
    pub scale: f64,
    pub inputs: Inputs,
    pub last_sent: Option<Frame>,
}

impl Runtime {
    pub fn new(pkg: CharacterPackage, masks: SpriteMasks, scale: f64) -> Self {
        Self {
            engine: Engine::new(pkg),
            masks,
            scale,
            inputs: Inputs::default(),
            last_sent: None,
        }
    }

    pub fn stage(&self) -> Stage {
        self.engine.package().manifest.stage
    }

    /// Build the frame to draw right now.
    pub fn current_frame(&self) -> Frame {
        let frame = self.engine.frame(&self.inputs);
        let comp = compose(&frame, &self.stage(), &self.masks);
        Frame {
            comp,
            subtitle: frame.subtitle,
            speaking: frame.state == State::Speaking,
            thinking: frame.state == State::Thinking,
            listening: frame.listening,
            level: self.inputs.voice_level,
        }
    }

    /// Convert a cursor position in physical desktop pixels into stage units.
    ///
    /// `origin` is the window's top-left in physical pixels and `dpi` its
    /// scale factor. Returns coordinates even when they fall outside the
    /// stage — the caller decides what "too far away to notice" means, and
    /// clamping here would put a phantom cursor on his shoulder.
    pub fn cursor_to_stage(
        &self,
        cursor: (f64, f64),
        origin: (f64, f64),
        dpi: f64,
    ) -> (i32, i32) {
        let per_unit = self.scale * dpi;
        if per_unit <= 0.0 {
            return (i32::MIN, i32::MIN);
        }
        let x = (cursor.0 - origin.0) / per_unit;
        let y = (cursor.1 - origin.1) / per_unit;
        (saturate(x), saturate(y))
    }
}

/// Cast to i32 without wrapping. A cursor on a second monitor can be far
/// outside the window, and `as i32` on a large f64 is a silent trap.
fn saturate(v: f64) -> i32 {
    // NaN must be handled before clamp: `f64::NAN.clamp(..) as i32` is 0,
    // which would read as a cursor sitting on the stage origin — a false hit.
    // Infinities clamp correctly and keep their sign.
    if v.is_nan() {
        return i32::MIN;
    }
    v.clamp(i32::MIN as f64, i32::MAX as f64) as i32
}

#[cfg(test)]
mod tests {
    use super::*;
    use deskfolk_engine::layout::{Composition, Placed};
    use deskfolk_engine::Rect;

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

    /// Minimal stand-in for a loaded runtime, so the coordinate maths can be
    /// tested without touching disk or a window.
    struct Coords {
        scale: f64,
    }

    impl Coords {
        fn to_stage(&self, cursor: (f64, f64), origin: (f64, f64), dpi: f64) -> (i32, i32) {
            let per_unit = self.scale * dpi;
            (
                saturate((cursor.0 - origin.0) / per_unit),
                saturate((cursor.1 - origin.1) / per_unit),
            )
        }
    }

    #[test]
    fn cursor_maps_to_stage_units_at_1x() {
        let c = Coords { scale: 1.0 };
        assert_eq!(c.to_stage((150.0, 250.0), (100.0, 200.0), 1.0), (50, 50));
    }

    #[test]
    fn cursor_accounts_for_companion_scale() {
        // A 1.3x companion: 65 physical pixels in is 50 stage units in.
        let c = Coords { scale: 1.3 };
        assert_eq!(c.to_stage((165.0, 165.0), (100.0, 100.0), 1.0), (50, 50));
    }

    #[test]
    fn cursor_accounts_for_display_dpi() {
        // 150% Windows scaling: physical pixels are 1.5x logical ones.
        let c = Coords { scale: 1.0 };
        assert_eq!(c.to_stage((175.0, 175.0), (100.0, 100.0), 1.5), (50, 50));
    }

    #[test]
    fn cursor_far_off_screen_does_not_wrap() {
        // Regression guard: `as i32` on a huge f64 used to wrap to a value
        // that landed back inside the stage and made him react to a cursor
        // on another monitor.
        let c = Coords { scale: 1.0 };
        let (x, y) = c.to_stage((-1.0e18, 1.0e18), (0.0, 0.0), 1.0);
        assert!(x < -1000, "got {x}");
        assert!(y > 1000, "got {y}");
    }

    #[test]
    fn saturate_handles_nonfinite() {
        assert_eq!(saturate(f64::NAN), i32::MIN);
        assert_eq!(saturate(f64::INFINITY), i32::MAX);
        assert_eq!(saturate(f64::NEG_INFINITY), i32::MIN);
    }

    #[test]
    fn frames_compare_by_value_so_unchanged_ones_are_never_repainted() {
        // The loop only presents when the frame differs; if equality were
        // identity-based the window would repaint 60 times a second forever.
        let mk = || Frame {
            comp: Composition {
                character: Some(Placed {
                    sprite: "a".into(),
                    rect: Rect { x: 1, y: 2, w: 3, h: 4 },
                }),
                fx: None,
                glitch: None,
            },
            subtitle: Some("hi".into()),
            speaking: false,
            thinking: false,
            listening: false,
            level: 0,
        };
        assert_eq!(mk(), mk());
        let mut other = mk();
        other.level = 5;
        assert_ne!(mk(), other);
    }

    #[test]
    fn a_speaking_frame_is_distinct_from_a_thinking_one() {
        // The renderer draws a different state ring for each; folding them
        // together would make him look like he is still thinking while he
        // talks.
        let base = Frame {
            comp: Composition { character: None, fx: None, glitch: None },
            subtitle: None,
            speaking: false,
            thinking: false,
            listening: false,
            level: 0,
        };
        let speaking = Frame { speaking: true, ..base.clone() };
        let thinking = Frame { thinking: true, ..base.clone() };
        assert_ne!(speaking, thinking);
    }

    #[test]
    fn window_size_scales_the_stage() {
        let s = stage();
        let scale = 1.3;
        assert_eq!(
            (s.width as f64 * scale, s.height as f64 * scale),
            (520.0, 520.0)
        );
    }
}
