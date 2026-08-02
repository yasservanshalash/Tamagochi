//! Turning one composed frame into pixels.
//!
//! A direct port of what the canvas renderer drew, with the same rules and the
//! same reasons: stage units in, device pixels out, nothing decided here that
//! the engine already decided. The engine still owns *where* everything goes —
//! `deskfolk_engine::compose` produced these rectangles, and the hit-test the
//! window relies on is the very same alpha these blits write.

use deskfolk_engine::layout::{Composition, Placed};
use deskfolk_engine::Portal;
use deskfolk_package::Stage;

use crate::canvas::{rgba, Canvas, Px};
use crate::sprites::Sprites;
use crate::text::TextRenderer;
use crate::Frame;

/// The glitch overlay is a full-frame static sheet; at full strength it hides
/// the character completely.
const GLITCH_ALPHA: u8 = 191; // 0.75

const BUBBLE_FILL: Px = rgba(14, 18, 22, 219); // 0.86
const BUBBLE_EDGE: Px = rgba(240, 160, 48, 89); // 0.35
const BUBBLE_TEXT: Px = rgba(230, 233, 226, 255);
const PORTAL_FILL: Px = rgba(20, 16, 12, 235); // 0.92

/// `DrawText` flags for a centred, wrapped caption.
const DT_CENTER: u32 = 0x1;
const DT_WORDBREAK: u32 = 0x10;
const DT_NOPREFIX: u32 = 0x800;

/// Scale a stage-unit length to device pixels.
#[inline]
fn dev(v: i32, unit: f64) -> i32 {
    (v as f64 * unit).round() as i32
}

pub fn paint(
    canvas: &mut Canvas<'_>,
    sprites: &Sprites,
    text: Option<&mut TextRenderer>,
    frame: &Frame,
    stage: &Stage,
    portal: Portal,
    unit: f64,
) {
    canvas.clear();

    draw_portal(canvas, stage, portal, unit);
    draw_placed(canvas, sprites, frame.comp.character.as_ref(), unit, 255);
    draw_placed(canvas, sprites, frame.comp.fx.as_ref(), unit, 255);
    draw_placed(canvas, sprites, frame.comp.glitch.as_ref(), unit, GLITCH_ALPHA);

    if let (Some(s), Some(t)) = (&frame.subtitle, text) {
        if !s.trim().is_empty() {
            draw_bubble(canvas, t, s, &frame.comp, stage, unit);
        }
    }

    draw_state_ring(canvas, frame, stage, portal, unit);
}

fn draw_placed(
    canvas: &mut Canvas<'_>,
    sprites: &Sprites,
    placed: Option<&Placed>,
    unit: f64,
    alpha: u8,
) {
    let Some(p) = placed else { return };
    let Some(sprite) = sprites.get(&p.sprite) else { return };
    canvas.blit_scaled(
        &sprite.px,
        sprite.w,
        sprite.h,
        dev(p.rect.x, unit),
        dev(p.rect.y, unit),
        dev(p.rect.w as i32, unit).max(1),
        dev(p.rect.h as i32, unit).max(1),
        alpha,
    );
}

fn draw_portal(canvas: &mut Canvas<'_>, stage: &Stage, portal: Portal, unit: f64) {
    let w = stage.width as f64 * unit;
    let h = stage.height as f64 * unit;
    match portal {
        Portal::Untethered => {}
        Portal::Circle => {
            // A circle is a rounded rect whose radius is half its shorter side.
            canvas.fill_round_rect(0.0, 0.0, w as f32, h as f32, (w.min(h) / 2.0) as f32, PORTAL_FILL);
        }
        Portal::Rounded { radius } => {
            canvas.fill_round_rect(
                0.0,
                0.0,
                w as f32,
                h as f32,
                (radius as f64 * unit) as f32,
                PORTAL_FILL,
            );
        }
    }
}

/// Speech bubble above the head, nudged to stay inside the stage.
///
/// Sized to the text so a three-word reply doesn't get a slab behind it, and
/// flipped below him when there is no room above — a bubble clipped by the top
/// of the window reads as a broken app, not as a character talking.
fn draw_bubble(
    canvas: &mut Canvas<'_>,
    text: &mut TextRenderer,
    body: &str,
    comp: &Composition,
    stage: &Stage,
    unit: f64,
) -> Option<(i32, i32, i32, i32)> {
    let pad_x = (8.0 * unit) as i32;
    let pad_y = (6.0 * unit) as i32;
    let max_w = ((stage.width as f64 - 24.0).min(300.0) * unit) as i32;
    let inner_max = (max_w - pad_x * 2).max(16);

    let (tw, th) = text.measure(body, inner_max);
    if tw <= 0 || th <= 0 {
        return None;
    }
    let box_w = (tw + pad_x * 2).min(max_w);
    let box_h = th + pad_y * 2;

    let stage_w = stage.width as f64 * unit;
    let head = comp.character.as_ref().map(|c| c.rect);
    let center_x = match head {
        Some(r) => (r.x as f64 + r.w as f64 / 2.0) * unit,
        None => stage_w / 2.0,
    };

    let mut x = (center_x - box_w as f64 / 2.0).round() as i32;
    let mut y = match head {
        Some(r) => dev(r.y, unit) - box_h - (8.0 * unit) as i32,
        None => (8.0 * unit) as i32,
    };
    let margin = (4.0 * unit) as i32;
    x = x.clamp(margin, (stage_w as i32 - box_w - margin).max(margin));
    if y < margin {
        y = match head {
            Some(r) => dev(r.y + r.h as i32, unit) + (8.0 * unit) as i32,
            None => margin,
        };
    }

    let radius = (10.0 * unit) as f32;
    canvas.fill_round_rect(x as f32, y as f32, box_w as f32, box_h as f32, radius, BUBBLE_FILL);
    canvas.stroke_round_rect(
        x as f32,
        y as f32,
        box_w as f32,
        box_h as f32,
        radius,
        unit.max(1.0) as f32,
        BUBBLE_EDGE,
    );

    if let Some(cov) = text.coverage(
        body,
        box_w - pad_x * 2,
        box_h - pad_y * 2,
        DT_CENTER | DT_WORDBREAK | DT_NOPREFIX,
    ) {
        canvas.blit_coverage(
            &cov,
            box_w - pad_x * 2,
            box_h - pad_y * 2,
            x + pad_x,
            y + pad_y,
            BUBBLE_TEXT,
        );
    }

    Some((x, y, box_w, box_h))
}

/// State feedback, and deliberately none when untethered.
///
/// A ring at his feet on the open desktop is app chrome announcing "this is a
/// widget"; inside a portal there is already a frame to trace, so it reads as
/// part of the object instead.
fn draw_state_ring(
    canvas: &mut Canvas<'_>,
    frame: &Frame,
    stage: &Stage,
    portal: Portal,
    unit: f64,
) {
    if matches!(portal, Portal::Untethered) {
        return;
    }
    let color = if frame.listening {
        let pulse = (0.45 + (frame.level as f32 / 200.0).min(0.45)) * 255.0;
        rgba(235, 80, 70, pulse as u8)
    } else if frame.thinking {
        rgba(240, 180, 60, 166)
    } else if frame.speaking {
        rgba(90, 200, 120, 140)
    } else {
        return;
    };

    let w = stage.width as f64 * unit;
    let h = stage.height as f64 * unit;
    let inset = 3.0 * unit;
    canvas.stroke_round_rect(
        inset as f32,
        inset as f32,
        (w - inset * 2.0) as f32,
        (h - inset * 2.0) as f32,
        ((w.min(h)) / 2.0) as f32,
        (4.0 * unit) as f32,
        color,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use deskfolk_engine::Rect;

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

    fn frame_with(sprite: &str) -> Frame {
        Frame {
            comp: Composition {
                character: Some(Placed {
                    sprite: sprite.into(),
                    rect: Rect { x: 100, y: 100, w: 80, h: 120 },
                }),
                fx: None,
                glitch: None,
            },
            subtitle: None,
            speaking: false,
            thinking: false,
            listening: false,
            level: 0,
        }
    }

    #[test]
    fn stage_units_scale_into_device_pixels() {
        assert_eq!(dev(100, 1.3), 130);
        assert_eq!(dev(0, 2.0), 0);
        assert_eq!(dev(-5, 2.0), -10);
    }

    #[test]
    fn a_missing_sprite_draws_nothing_rather_than_panicking() {
        // A package can reference a sprite that failed to decode; the frame
        // must still render everything else.
        let mut buf = vec![0u32; 64 * 64];
        let mut canvas = Canvas::new(&mut buf, 64, 64);
        let sprites = Sprites::default();
        let f = frame_with("not_loaded");
        paint(&mut canvas, &sprites, None, &f, &stage(), Portal::Untethered, 1.0);
        assert!(buf.iter().all(|p| *p == 0));
    }

    #[test]
    fn an_untethered_portal_paints_no_background() {
        // The whole point of untethered is that nothing but the character is
        // on the desktop. Any fill here would be a visible box.
        let mut buf = vec![0u32; 32 * 32];
        let mut canvas = Canvas::new(&mut buf, 32, 32);
        draw_portal(&mut canvas, &stage(), Portal::Untethered, 0.05);
        assert!(buf.iter().all(|p| *p == 0));
    }

    #[test]
    fn a_rounded_portal_paints_a_backdrop() {
        let mut buf = vec![0u32; 32 * 32];
        let mut canvas = Canvas::new(&mut buf, 32, 32);
        draw_portal(&mut canvas, &stage(), Portal::Rounded { radius: 8 }, 32.0 / 412.0);
        assert!(canvas.get(16, 16) >> 24 > 0, "centre should be filled");
    }

    #[test]
    fn an_untethered_companion_gets_no_state_ring() {
        let mut buf = vec![0u32; 32 * 32];
        let mut canvas = Canvas::new(&mut buf, 32, 32);
        let mut f = frame_with("x");
        f.listening = true;
        draw_state_ring(&mut canvas, &f, &stage(), Portal::Untethered, 32.0 / 412.0);
        assert!(buf.iter().all(|p| *p == 0));
    }

    #[test]
    fn a_portal_companion_shows_a_listening_ring() {
        let mut buf = vec![0u32; 64 * 64];
        let mut canvas = Canvas::new(&mut buf, 64, 64);
        let mut f = frame_with("x");
        f.listening = true;
        draw_state_ring(&mut canvas, &f, &stage(), Portal::Circle, 64.0 / 412.0);
        assert!(buf.iter().any(|p| *p >> 24 > 0), "the ring should mark some pixels");
    }
}
