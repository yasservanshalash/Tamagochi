//! End-to-end proof of the modular render path: a definition + a manifest that
//! binds one part to a region of a real PNG sheet, assembled onto a Canvas.
//! This exercises `ModularScene::load` → `LayerResolver` → `blit_scaled_src`
//! with genuine image decoding, so a regression anywhere in that chain fails
//! here. Windows-only, matching the crate's `#[cfg(windows)]` surface.

#![cfg(windows)]

use deskfolk_engine::modular::{CharacterDefinition, CharacterState};
use deskfolk_render_win::canvas::Canvas;
use deskfolk_render_win::modular_paint::{ModularScene, Overlays};
use std::fs;
use std::path::PathBuf;

/// Minimal definition with a single torso slot at a known canonical location.
const DEF: &str = r#"{
    "schema": "deskfolk.modular-character.v1",
    "id": "test", "name": "T", "version": 1,
    "coordinate_system": { "canvas": {"width":320,"height":320} },
    "layer_order_front": ["torso"],
    "slots": {
        "torso": { "default_bounds": {"x":100,"y":100,"width":20,"height":20},
                   "pivot": [110,110], "variants": ["idle_front"] }
    }
}"#;

fn write_sheet(path: &PathBuf) {
    // A 40x20 sheet: left half transparent, right half (x>=20) solid blue.
    // The manifest points the torso at the blue cell (20,0,20,20).
    let mut img = image::RgbaImage::new(40, 20);
    for y in 0..20 {
        for x in 0..40 {
            let px = if x >= 20 {
                image::Rgba([0, 0, 255, 255])
            } else {
                image::Rgba([0, 0, 0, 0])
            };
            img.put_pixel(x, y, px);
        }
    }
    img.save(path).unwrap();
}

#[test]
fn assembles_a_bound_part_onto_the_canvas() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("modular_scene");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("sheets")).unwrap();

    fs::write(dir.join("definition.json"), DEF).unwrap();
    write_sheet(&dir.join("sheets/torso.png"));

    let manifest = r#"{
        "definition_id": "test",
        "instances": [
            { "id": "torso/idle_front", "slot": "torso", "variant": "idle_front",
              "view": "front", "source": "sheets/torso.png",
              "region": { "x": 20, "y": 0, "w": 20, "h": 20 },
              "canonical_bounds": { "x": 100, "y": 100, "width": 20, "height": 20 },
              "pivot": [110, 110] }
        ]
    }"#;
    fs::write(dir.join("manifest.json"), manifest).unwrap();

    let scene = ModularScene::load(&dir).expect("scene loads");
    let def = CharacterDefinition::from_json(DEF).unwrap();
    let state = CharacterState::canonical_yasser(&def);

    // Canvas big enough to hold 320-space at unit 1.0.
    let mut buf = vec![0u32; 320 * 320];
    let mut canvas = Canvas::new(&mut buf, 320, 320);
    let drawn = scene.paint(&mut canvas, &state, 1.0, Overlays::default(), 0, false);

    assert_eq!(drawn, 1, "the torso part should draw");
    // Centre of the destination (110,110) must be opaque blue-ish.
    let center = buf[110 * 320 + 110];
    assert_eq!(center >> 24, 255, "torso centre should be opaque");
    assert!(center & 0xff > 128, "torso centre should carry blue");
    // Outside the destination stays clear.
    assert_eq!(buf[10 * 320 + 10], 0, "far corner untouched");
}

#[test]
fn empty_manifest_draws_nothing_but_loads() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("modular_empty");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("definition.json"), DEF).unwrap();
    fs::write(
        dir.join("manifest.json"),
        r#"{ "definition_id": "test", "instances": [] }"#,
    )
    .unwrap();

    let scene = ModularScene::load(&dir).unwrap();
    let def = CharacterDefinition::from_json(DEF).unwrap();
    let state = CharacterState::canonical_yasser(&def);
    let mut buf = vec![0u32; 320 * 320];
    let mut canvas = Canvas::new(&mut buf, 320, 320);
    let drawn = scene.paint(&mut canvas, &state, 1.0, Overlays::default(), 0, false);
    assert_eq!(drawn, 0);
    assert!(buf.iter().all(|p| *p == 0), "nothing should be drawn");
}
