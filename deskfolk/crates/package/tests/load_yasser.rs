//! Loads the real shipped Yasser package.
//!
//! This is the format's end-to-end contract test: if the generator, the
//! manifest schema and the validator ever drift apart, this fails.

use std::path::PathBuf;

use deskfolk_package::{CharacterPackage, Role};

fn yasser() -> CharacterPackage {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../characters/yasser");
    CharacterPackage::load_dir(&root)
        .unwrap_or_else(|e| panic!("failed to load {}: {e}", root.display()))
}

#[test]
fn loads_without_fatal_problems() {
    let p = yasser();
    assert_eq!(p.manifest.id, "yasser");
    assert_eq!(p.manifest.stage.width, 412);
    // Feet sit 26px up from the bottom edge, as in the firmware.
    assert_eq!(p.manifest.stage.anchor_y, 386);
}

#[test]
fn ships_clean_with_no_warnings() {
    // The package we ship is the reference every third-party author copies.
    // It should not model bad practice.
    let p = yasser();
    assert!(p.warnings.is_empty(), "shipped package has warnings: {:?}", p.warnings);
}

#[test]
fn carries_the_whole_alpha_behavior_table() {
    // `hd_*` are preview frames from a newer sprite sheet, reachable only from
    // the animation tester. Counting them here would mean this guard fired
    // every time one was added, which is the opposite of what it is for: the
    // behaviour table is what must not drift.
    let p = yasser();
    let behaviour = |names: Vec<&String>| -> usize {
        names.iter().filter(|n| !n.starts_with("hd_")).count()
    };
    // 20 + 19 at the alpha; `walk`, `walk_right` and `stand` joined the
    // behaviour table when he stopped hopping and started walking for real.
    assert_eq!(
        behaviour(p.manifest.clips.keys().collect()),
        23,
        "clip count drifted from the alpha"
    );
    assert_eq!(
        behaviour(p.manifest.emotions.keys().collect()),
        22,
        "emotion count drifted from the alpha"
    );
    assert_eq!(p.manifest.visemes.len(), 4);
}

#[test]
fn stopping_has_a_pose_of_its_own() {
    // Without this he freezes on whatever stride he happened to end on, which
    // reads as a dropped frame rather than as a person standing still.
    let p = yasser();
    let stand = p.clip("stand").expect("no standing pose");
    assert_eq!(stand.frames.len(), 1, "standing is one pose, not a cycle");
    let walking: Vec<&str> = p
        .clip("walk")
        .expect("walk")
        .frames
        .iter()
        .map(|f| f.img.as_str())
        .collect();
    assert!(
        !walking.contains(&stand.frames[0].img.as_str()),
        "the standing pose is inside the walk cycle, which is what made him \
         turn to face the viewer once per stride"
    );
}

#[test]
fn walking_has_a_mirrored_twin_that_is_actually_mirrored() {
    // The renderer cannot flip a sprite at draw time, so facing right is a
    // second set of pre-flipped frames. Both names pointing at the same images
    // is the failure that has him moonwalking in one direction.
    let p = yasser();
    let left = p.clip("walk").expect("walk drives Gait::of; without it he hops");
    let right = p.clip("walk_right").expect("no mirrored cycle");
    assert_eq!(left.frames.len(), right.frames.len(), "cycles differ in length");
    for (a, b) in left.frames.iter().zip(right.frames.iter()) {
        assert_ne!(a.img, b.img, "'{}' is used for both directions", a.img);
    }
}

#[test]
fn every_preview_animation_is_playable() {
    // The tester lists emotions and plays them by name, so a preview clip with
    // no emotion pointing at it is invisible from the menu.
    let p = yasser();
    let previews: Vec<&String> =
        p.manifest.clips.keys().filter(|n| n.starts_with("hd_")).collect();
    assert!(!previews.is_empty(), "the preview sheet should be imported");
    for clip in previews {
        let emo = p.manifest.emotions.get(clip);
        assert!(emo.is_some(), "clip '{clip}' has no emotion, so nothing can play it");
        assert_eq!(&emo.unwrap().clip, clip, "emotion '{clip}' points elsewhere");
    }
}

#[test]
fn every_role_resolves_to_a_real_clip() {
    let p = yasser();
    for role in [
        Role::Idle,
        Role::Blink,
        Role::Talk,
        Role::Think,
        Role::Sleep,
        Role::Startle,
        Role::Glitch,
    ] {
        let name = p.role_clip(role);
        assert!(p.clip(name).is_some(), "role {role:?} -> missing clip '{name}'");
    }
}

#[test]
fn every_referenced_sprite_exists_on_disk() {
    let p = yasser();
    let sprites = p.referenced_sprites();
    assert!(sprites.len() > 30, "only {} sprites referenced", sprites.len());
    for s in &sprites {
        let path = p.sprite_path(s);
        assert!(path.exists(), "missing sprite file: {}", path.display());
    }
}

#[test]
fn idle_clip_loops_so_he_is_never_frozen() {
    let p = yasser();
    let idle = p.clip(p.role_clip(Role::Idle)).unwrap();
    assert!(idle.looping, "the idle clip must loop or he freezes");
    assert!(idle.frames.len() > 1, "idle needs motion, not a single frame");
}

#[test]
fn dance_cycles_its_music_note_overlay() {
    // Guards the fx cycling path, which is easy to break and very visible.
    let p = yasser();
    let dance = p.clip("dance").expect("dance clip");
    assert!(dance.fx.len() > 1);
    assert!(dance.fx_ms > 0);
}
