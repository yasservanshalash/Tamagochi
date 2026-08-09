//! The modular character render path — additive, dev-gated, and independent of
//! the classic single-sprite [`paint`](crate::paint) pipeline.
//!
//! It loads a [`CharacterDefinition`] + [`SpriteManifest`] + the sheet PNGs the
//! manifest references, then for a given [`CharacterState`] asks the engine's
//! [`LayerResolver`] for a back-to-front draw list and blits each part's *region*
//! straight out of its sheet with [`Canvas::blit_scaled_src`] — one global scale,
//! integer positions, nearest-neighbour, no per-part stretch.
//!
//! Because the shipped manifest is empty (the generated sheets don't conform),
//! this normally draws nothing and logs every missing slot. That is the point:
//! it renders exactly what genuinely exists and is honest about the rest. As the
//! inspector binds conforming parts, they light up here with no code change.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use deskfolk_engine::modular::{
    CharacterDefinition, CharacterState, DrawPart, LayerResolver, SpriteManifest,
};

use crate::canvas::{rgba, Canvas, Px};
use crate::sprites::{premultiply, Sprite};

/// Which debug annotations to overlay on the assembled character.
#[derive(Debug, Clone, Copy, Default)]
pub struct Overlays {
    pub bounds: bool,
    pub pivots: bool,
    pub anchors: bool,
}

const BOUNDS_COLOR: Px = rgba(240, 160, 48, 200); // warm accent
const PIVOT_COLOR: Px = rgba(87, 196, 100, 255); // green cross
const ANCHOR_COLOR: Px = rgba(224, 86, 78, 255); // red tick

/// A loaded modular character: the spec, the part bindings, and the decoded
/// sheets those bindings point at.
pub struct ModularScene {
    pub definition: CharacterDefinition,
    pub manifest: SpriteManifest,
    /// source path (as written in the manifest) -> decoded sheet.
    sheets: HashMap<String, Sprite>,
    /// Directory the manifest + relative sheet paths resolve against.
    base_dir: PathBuf,
}

impl ModularScene {
    /// Load from a `characters/<id>/modular/` directory containing
    /// `definition.json` + `manifest.json` (+ `sheets/`).
    pub fn load(dir: impl AsRef<Path>) -> Result<Self, String> {
        let dir = dir.as_ref().to_path_buf();
        let definition = CharacterDefinition::load(dir.join("definition.json"))
            .map_err(|e| format!("definition: {e}"))?;
        let manifest = SpriteManifest::load(dir.join("manifest.json"))
            .map_err(|e| format!("manifest: {e}"))?;

        // Decode each unique sheet the manifest references. A sheet that fails
        // to decode is dropped (its parts become "missing"), never faked.
        let mut sheets = HashMap::new();
        for inst in &manifest.instances {
            if sheets.contains_key(&inst.source) {
                continue;
            }
            let path = dir.join(&inst.source);
            match decode_sheet(&path) {
                Ok(s) => {
                    sheets.insert(inst.source.clone(), s);
                }
                Err(e) => tracing::warn!(sheet = %inst.source, "modular sheet decode failed: {e}"),
            }
        }

        Ok(Self { definition, manifest, sheets, base_dir: dir })
    }

    pub fn base_dir(&self) -> &Path {
        &self.base_dir
    }

    /// Paint `state` onto `canvas`. `unit` scales 320-authoring-space to device
    /// pixels (the whole character shares this one factor). Returns the number
    /// of parts actually drawn; missing slots are logged.
    pub fn paint(
        &self,
        canvas: &mut Canvas<'_>,
        state: &CharacterState,
        unit: f64,
        overlays: Overlays,
        offset_y: i32,
        flip_x: bool,
    ) -> usize {
        let (parts, missing) = LayerResolver::resolve(&self.definition, state, &self.manifest);
        if !missing.is_empty() {
            let slots: Vec<&str> = missing.iter().map(|m| m.slot.as_str()).collect();
            tracing::debug!(count = missing.len(), ?slots, "modular: parts with no bound sprite");
        }

        let mut drawn = 0;
        for part in &parts {
            if self.draw_part(canvas, part, unit, overlays, offset_y, flip_x) {
                drawn += 1;
            }
        }
        drawn
    }

    fn draw_part(
        &self,
        canvas: &mut Canvas<'_>,
        part: &DrawPart,
        unit: f64,
        overlays: Overlays,
        offset_y: i32,
        flip_x: bool,
    ) -> bool {
        let Some(sheet) = self.sheets.get(&part.source) else {
            return false;
        };
        let (sx, sy, sw, sh) = part.region;
        let d = part.dest;
        // When flipped (facing the other way), mirror the dest around the canvas
        // centre so the whole assembly turns, and mirror each part's pixels.
        let canvas_w = self.definition.canvas().width;
        let dest_x = if flip_x { canvas_w - d.x - d.width } else { d.x };
        canvas.blit_scaled_src(
            &sheet.px,
            sheet.w,
            sheet.h,
            sx,
            sy,
            sw,
            sh,
            dev(dest_x, unit),
            dev(d.y + offset_y, unit),
            dev(d.width, unit).max(1),
            dev(d.height, unit).max(1),
            part.alpha,
            flip_x,
        );

        if overlays.bounds {
            canvas.stroke_round_rect(
                d.x as f32 * unit as f32,
                d.y as f32 * unit as f32,
                d.width as f32 * unit as f32,
                d.height as f32 * unit as f32,
                0.0,
                1.0,
                BOUNDS_COLOR,
            );
        }
        if overlays.pivots {
            draw_cross(canvas, dev(part.pivot.x(), unit), dev(part.pivot.y(), unit), 3, PIVOT_COLOR);
        }
        true
    }

    /// Overlay the connection anchors from the definition's slots (independent of
    /// whether a part is bound), so the rig is visible even on an empty manifest.
    pub fn paint_skeleton(&self, canvas: &mut Canvas<'_>, unit: f64) {
        for pt in self.definition.skeleton_front.values() {
            draw_cross(canvas, dev(pt.x(), unit), dev(pt.y(), unit), 2, ANCHOR_COLOR);
        }
    }
}

/// Scale a 320-space length to device pixels.
#[inline]
fn dev(v: i32, unit: f64) -> i32 {
    (v as f64 * unit).round() as i32
}

fn draw_cross(canvas: &mut Canvas<'_>, cx: i32, cy: i32, r: i32, color: Px) {
    let (cx, cy) = (cx as f32, cy as f32);
    let r = r as f32;
    canvas.stroke_line(cx - r, cy, cx + r, cy, 1.0, color);
    canvas.stroke_line(cx, cy - r, cx, cy + r, 1.0, color);
}

/// Decode a PNG sheet into a premultiplied BGRA buffer — the same format
/// [`crate::sprites::Sprites`] produces, reusing [`premultiply`].
fn decode_sheet(path: &Path) -> Result<Sprite, String> {
    let img = image::open(path).map_err(|e| e.to_string())?.to_rgba8();
    let (w, h) = img.dimensions();
    let px: Vec<Px> = img
        .pixels()
        .map(|p| premultiply(p.0[0], p.0[1], p.0[2], p.0[3]))
        .collect();
    Ok(Sprite { w, h, px })
}
