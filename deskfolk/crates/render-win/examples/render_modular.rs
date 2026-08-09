//! Headless "run the modular renderer" — loads a `characters/<id>/modular/`
//! directory (definition + manifest + sheets) with the *real* runtime code
//! (`ModularScene`), assembles the canonical Yasser, and writes the result to a
//! PNG. This is the program's own render path, just aimed at a file instead of
//! a layered window, so you can see exactly what the app would draw.
//!
//! Usage:
//!   cargo run -p deskfolk-render-win --example render_modular -- \
//!       characters/yasser/modular out.png [scale]
//!
//! With the shipped (empty) manifest it draws nothing and prints the missing
//! slots; bind parts (inspector or autoslice_manifest.py) and it fills in.

#![cfg(windows)]

use deskfolk_engine::modular::{CharacterDefinition, CharacterState};
use deskfolk_render_win::canvas::Canvas;
use deskfolk_render_win::modular_paint::{ModularScene, Overlays};

fn main() {
    let mut args = std::env::args().skip(1);
    let dir = args.next().unwrap_or_else(|| "characters/yasser/modular".into());
    let out = args.next().unwrap_or_else(|| "modular_render.png".into());
    let scale: u32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(2);

    let def = CharacterDefinition::load(std::path::Path::new(&dir).join("definition.json"))
        .expect("load definition.json");
    let canvas_units = def.canvas().width.max(def.canvas().height) as u32;
    let size = canvas_units * scale;

    let scene = ModularScene::load(&dir).expect("load modular scene");
    let mut state = CharacterState::canonical_yasser(&def);
    // Verification hooks: swap individual slot variants to preview poses.
    for (env, slot) in [
        ("DESKFOLK_HEAD", "head_base"),
        ("DESKFOLK_TORSO", "torso"),
        ("DESKFOLK_LEG", "left_leg"),
    ] {
        if let Ok(v) = std::env::var(env) {
            state.variants.insert(slot.to_string(), v);
        }
    }

    let mut buf = vec![0u32; (size * size) as usize];
    let mut canvas = Canvas::new(&mut buf, size as i32, size as i32);
    let overlays = Overlays {
        bounds: std::env::var("OVERLAY_BOUNDS").is_ok(),
        pivots: std::env::var("OVERLAY_PIVOTS").is_ok(),
        anchors: false,
    };
    let flip = std::env::var("DESKFOLK_FLIP").is_ok();
    let drawn = scene.paint(&mut canvas, &state, scale as f64, overlays, 0, flip);

    // Premultiplied BGRA u32 -> straight RGBA8 for PNG.
    let mut img = image::RgbaImage::new(size, size);
    for (i, px) in buf.iter().enumerate() {
        let a = (px >> 24) as u8;
        let (mut r, mut g, mut b) = (((px >> 16) & 0xff) as u8, ((px >> 8) & 0xff) as u8, (px & 0xff) as u8);
        if a > 0 && a < 255 {
            // un-premultiply so viewers show the true colour
            r = ((r as u32 * 255) / a as u32).min(255) as u8;
            g = ((g as u32 * 255) / a as u32).min(255) as u8;
            b = ((b as u32 * 255) / a as u32).min(255) as u8;
        }
        let x = (i as u32) % size;
        let y = (i as u32) / size;
        img.put_pixel(x, y, image::Rgba([r, g, b, a]));
    }
    img.save(&out).expect("write png");
    println!("rendered {drawn} parts -> {out} ({size}x{size})");
}
