//! Integration test: the *real* packaged spec must load, and the shipped
//! (empty) manifest must audit as "everything missing" — the honest state until
//! conforming parts are curated. This guards against silent drift between the
//! JSON the artist authored and the structs the engine reads.

use deskfolk_engine::modular::{
    audit, CharacterDefinition, CharacterState, LayerResolver, SpriteManifest,
};
use std::path::PathBuf;

fn pkg_dir() -> PathBuf {
    // crates/engine -> ../../characters/yasser/modular
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../characters/yasser/modular")
}

#[test]
fn real_definition_loads_and_matches_spec_shape() {
    let def = CharacterDefinition::load(pkg_dir().join("definition.json"))
        .expect("packaged definition.json must load");

    assert_eq!(def.id, "deskfolk_yasser_modular_v1");
    assert_eq!(def.canvas().width, 320);
    assert_eq!(def.canvas().height, 320);
    // 17-entry front layer order, per the spec.
    assert_eq!(def.layer_order_front.len(), 17);
    assert_eq!(def.layer_order_front.first().map(String::as_str), Some("back_accessory"));
    assert_eq!(def.layer_order_front.last().map(String::as_str), Some("effects"));
    // 22 slots.
    assert_eq!(def.slots.len(), 22);
    // A few canonical geometry facts.
    assert_eq!(def.default_variant("torso"), Some("idle_front"));
    assert_eq!(def.slot("head_base").unwrap().pivot.unwrap().y(), 105);
    assert_eq!(def.slot("right_hand").unwrap().variants.len(), 18);
    // Unmodelled spec sections survive.
    assert!(def.extra.contains_key("style_lock"));
    assert!(def.extra.contains_key("character_lock"));
    assert!(def.extra.contains_key("generation_contract"));
}

#[test]
fn canonical_state_seeds_every_part_slot() {
    let def = CharacterDefinition::load(pkg_dir().join("definition.json")).unwrap();
    let state = CharacterState::canonical_yasser(&def);
    // Every slot with a pivot (a real layered part) gets a default variant.
    let part_slots = def.slots.values().filter(|s| s.pivot.is_some()).count();
    assert_eq!(state.variants.len(), part_slots);
    assert_eq!(state.variant("beanie"), Some("default_front"));
    assert_eq!(state.variant("eyes"), Some("neutral"));
}

#[test]
fn shipped_manifest_is_empty_and_audits_as_all_missing() {
    let def = CharacterDefinition::load(pkg_dir().join("definition.json")).unwrap();
    let manifest = SpriteManifest::load(pkg_dir().join("manifest.json"))
        .expect("packaged manifest.json must load");
    assert!(manifest.instances.is_empty());

    let report = audit(&def, &manifest);
    assert!(report.part_slot_total() >= 20);
    assert_eq!(report.conforming(), 0);
    assert_eq!(report.missing(), report.part_slot_total());
    assert!(!report.is_complete());

    // And the layer resolver yields no parts, but reports the gaps.
    let (parts, missing) = LayerResolver::resolve(&def, &CharacterState::canonical_yasser(&def), &manifest);
    assert!(parts.is_empty());
    assert!(!missing.is_empty());
}
