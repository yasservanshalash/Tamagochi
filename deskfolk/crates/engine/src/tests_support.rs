//! A minimal synthetic package for engine tests.
//!
//! Deliberately *not* Yasser: the engine must be provably character-agnostic,
//! so its tests run against a character that has nothing to do with the one
//! we ship. Frame durations are round numbers to keep assertions readable.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use deskfolk_package::CharacterPackage;

/// A valid 1x1 transparent PNG. The validator only checks that sprite files
/// exist, but writing real PNGs keeps the fixture honest if that ever changes.
const PNG_1X1: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // signature
    0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, // IHDR
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15,
    0xC4, 0x89, //
    0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, // IDAT
    0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, //
    0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82, // IEND
];

const SPRITES: &[&str] = &[
    "a", "b", // idle
    "bl", // blink
    "t", "th", "z", "st", "w", "d", "p", // states
    "m0", "m1", "m2", "m3", // visemes
    "n1", "n2", // fx
    "s1", "s2", // glitch static
];

fn manifest_json() -> serde_json::Value {
    serde_json::json!({
        "format": 1,
        "id": "testchar",
        "name": "Test Character",
        "version": "0.0.1",
        "stage": { "width": 100, "height": 100, "anchor_x": 50, "anchor_y": 90 },
        "sprites": { "dir": "sprites", "filter": "nearest" },
        "clips": {
            "idle":      { "frames": [{ "img": "a", "ms": 100 }, { "img": "b", "ms": 100 }], "loop": true },
            "blink":     { "frames": [{ "img": "bl", "ms": 80 }] },
            "talk":      { "frames": [{ "img": "t", "ms": 100 }], "loop": true },
            "think":     { "frames": [{ "img": "th", "ms": 400 }], "loop": true },
            "sleep":     { "frames": [{ "img": "z", "ms": 1000 }] },
            "startle":   { "frames": [{ "img": "st", "ms": 200 }] },
            "wave":      { "frames": [{ "img": "w", "ms": 100 }] },
            "possessed": { "frames": [{ "img": "p", "ms": 100 }], "loop": true },
            "suspicious": { "frames": [{ "img": "w", "ms": 300 }] },
            "dance":     { "frames": [{ "img": "d", "ms": 240 }], "loop": true,
                           "fx": ["n1", "n2"], "fx_ms": 200 }
        },
        "emotions": {
            "idle":      { "clip": "idle", "base": true },
            "talk":      { "clip": "talk", "base": true, "hold_ms": 2600 },
            "think":     { "clip": "think", "base": true, "hold_ms": 4000 },
            "happy":     { "clip": "wave" },
            "confused":  { "clip": "wave" },
            "dance":     { "clip": "dance", "base": true, "hold_ms": 5200 }
        },
        "visemes": ["m0", "m1", "m2", "m3"],
        "roles": {
            "idle": "idle", "blink": "blink", "talk": "talk", "think": "think",
            "sleep": "sleep", "startle": "startle", "glitch": "possessed",
            "glitch_fx": ["s1", "s2"]
        },
        "life": {
            "blink_every": { "min": 3000, "max": 8000 },
            "nap_after_ms": 1200000,
            "night_nap_after_ms": 90000,
            "night_hours": [22, 8],
            "engaged_ms": 600000,
            "self_talk_every": { "min": 60000, "max": 150000 },
            "self_talk": true,
            "fidget_every": { "min": 4000, "max": 6000 },
            "fidgets": ["wave", "dance"],
            "gaze_px": 7,
            "notice_radius": 190,
            "notice_clip": "suspicious",
            "nudge_after_ms": 10800000,
            "nudge_cooldown_ms": 2700000
        },
        "personality": { "prompt": "a test character" }
    })
}

fn build_once() -> &'static PathBuf {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| {
        let root = std::env::temp_dir().join("deskfolk-engine-test-pkg");
        let sprites: &Path = &root.join("sprites");
        std::fs::create_dir_all(sprites).expect("create fixture sprite dir");
        for s in SPRITES {
            let p = sprites.join(format!("{s}.png"));
            if !p.exists() {
                std::fs::write(&p, PNG_1X1).expect("write fixture sprite");
            }
        }
        std::fs::write(
            root.join("character.json"),
            serde_json::to_string_pretty(&manifest_json()).unwrap(),
        )
        .expect("write fixture manifest");
        root
    })
}

pub(crate) fn test_package() -> CharacterPackage {
    let root = build_once();
    CharacterPackage::load_dir(root).expect("fixture package must be valid")
}

#[cfg(test)]
mod meta {
    use super::*;

    /// If the fixture itself stops validating, every other engine test is
    /// meaningless — so check it explicitly.
    #[test]
    fn fixture_package_is_valid() {
        let p = test_package();
        assert_eq!(p.manifest.id, "testchar");
        assert!(p.warnings.is_empty(), "fixture has warnings: {:?}", p.warnings);
    }
}
