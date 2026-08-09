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
fn resolves_a_portrait_from_the_talking_face() {
    // No explicit `portrait` role, so it falls back to the talk clip's first
    // frame — the talking face, which is what the UI shows as his avatar.
    let p = yasser();
    let sprite = p.portrait_sprite().expect("a portrait resolves");
    assert!(sprite.starts_with("img_y_talk"), "portrait should be the talk face: {sprite}");
    let path = p.portrait_path().expect("a portrait path");
    assert!(path.exists(), "portrait PNG must exist on disk: {}", path.display());
}

#[test]
fn scan_discovers_yasser_in_the_characters_dir() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../characters");
    let found = deskfolk_package::scan(&dir);
    let yasser = found.iter().find(|s| s.id == "yasser").expect("yasser is discoverable");
    assert_eq!(yasser.dir, "yasser", "loads by its folder name");
    assert!(yasser.tagline.is_some(), "the picker shows a tagline");
    assert!(yasser.portrait.as_ref().is_some_and(|p| p.exists()), "with a portrait");
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
    // behaviour table when he stopped hopping and started walking for real,
    // `stand_left` / `stand_right` when standing gained a direction to face, and
    // the eight `teleport_{out,in}_{a,b}[_l]` clips for instant transmission
    // both ways.
    assert_eq!(
        behaviour(p.manifest.clips.keys().collect()),
        33,
        "clip count drifted from the alpha"
    );
    assert_eq!(
        behaviour(p.manifest.emotions.keys().collect()),
        32,
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
    // One image, held. It appears twice only for the one-pixel breath this
    // package uses elsewhere, so stopping is not a frozen frame.
    let images: std::collections::BTreeSet<&str> =
        stand.frames.iter().map(|f| f.img.as_str()).collect();
    assert_eq!(images.len(), 1, "standing is one pose, not a cycle: {images:?}");
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

#[test]
fn the_instant_transmission_clips_are_all_present_and_play_once() {
    // The app plays these four by name and times them by frame count, so a
    // rename or a stray `loop: true` would break the teleport silently.
    let p = yasser();
    for name in [
        "teleport_out_a",
        "teleport_in_a",
        "teleport_out_b",
        "teleport_in_b",
        "teleport_out_a_l",
        "teleport_in_a_l",
        "teleport_out_b_l",
        "teleport_in_b_l",
    ] {
        let emo = p.manifest.emotions.get(name).unwrap_or_else(|| panic!("no emotion {name}"));
        let clip = p.clip(&emo.clip).unwrap_or_else(|| panic!("{name} points at a missing clip"));
        assert!(!clip.looping, "{name} must play once and park, not loop");
    }
    // Sheet a is six frames a half, b is eight — the app's timing depends on it.
    assert_eq!(p.clip("teleport_out_a").unwrap().frames.len(), 6);
    assert_eq!(p.clip("teleport_in_a").unwrap().frames.len(), 6);
    assert_eq!(p.clip("teleport_out_b").unwrap().frames.len(), 8);
    assert_eq!(p.clip("teleport_in_b").unwrap().frames.len(), 8);
}

#[test]
fn each_walk_clip_uses_the_frames_named_for_its_direction() {
    // The art faces one way; the mirror faces the other. Wiring them the wrong
    // way round is invisible in code and unmistakable on screen — he slides
    // along facing backwards. Naming is the only thing that pins it down, so
    // the naming is what gets checked.
    let p = yasser();
    for (clip, want) in [("walk", "img_y_walkl"), ("walk_right", "img_y_walkr")] {
        for f in &p.clip(clip).expect(clip).frames {
            assert!(
                f.img.starts_with(want),
                "clip '{clip}' uses '{}', which is the other direction's art",
                f.img
            );
        }
    }
}
