//! [`LayerResolver`] — flattens a [`CharacterState`] into a back-to-front draw
//! list the renderer can blit in order.
//!
//! The spec's `layer_order_front` is written in *rig* terms (`rear_arm`,
//! `face_features`, `handheld_prop`) rather than concrete slots. This module
//! owns the mapping from each rig layer to the concrete slots that fill it for a
//! given [`View`], resolves each to a manifest [`SpriteInstance`], and reports
//! any slot that has no bound sprite (so the renderer logs the gap instead of
//! drawing nothing and pretending it's fine).

use super::definition::{Bounds, CharacterDefinition, Point};
use super::manifest::{SpriteInstance, SpriteManifest};
use super::state::{CharacterState, View};

/// One resolved, placeable part in final paint order.
#[derive(Debug, Clone)]
pub struct DrawPart {
    pub layer: String,
    pub slot: String,
    pub variant: String,
    pub view: String,
    /// Sheet path (relative to manifest dir).
    pub source: String,
    /// Source rectangle within the sheet: (x, y, w, h).
    pub region: (u32, u32, u32, u32),
    /// Destination rectangle in 320 authoring-space.
    pub dest: Bounds,
    pub pivot: Point,
    pub angle: f32,
    pub alpha: u8,
}

/// A slot that was selected but has no sprite bound in the manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingPart {
    pub layer: String,
    pub slot: String,
    pub variant: String,
    pub view: String,
}

/// A pixel-free ordering entry, used for reasoning and tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanEntry {
    pub layer: String,
    pub slot: String,
    pub variant: String,
    pub view: String,
}

pub struct LayerResolver;

impl LayerResolver {
    /// The concrete slots that fill a rig layer, in draw order, for `view`.
    ///
    /// Arms and legs split into a rear side (drawn before the torso) and a
    /// front side (after). In `Front` view the split is screen-left = rear,
    /// screen-right = front — a stable, symmetric default; other views can
    /// refine this as their art lands.
    fn slots_for_layer(layer: &str, _view: View) -> &'static [&'static str] {
        match layer {
            // Nothing in the current slot vocabulary fills these; hood/back
            // hair are baked into torso variants for now.
            "back_accessory" | "back_hair_or_hood" | "neck" | "ears" | "front_accessory" => &[],

            "rear_arm" => &["left_upper_arm", "left_forearm", "left_hand"],
            "rear_leg" => &["left_leg", "left_boot"],
            // Pelvis is the hip cap between legs and jacket; draw it just under
            // the torso within the torso layer.
            "torso" => &["pelvis", "torso"],
            "front_leg" => &["right_leg", "right_boot"],
            "front_arm" => &["right_upper_arm", "right_forearm", "right_hand"],

            "head_base" => &["head_base"],
            // Spec face order: eyes, eyebrows, nose, mouth.
            "face_features" => &["eyes", "eyebrows", "nose", "mouth"],
            "beard" => &["beard"],
            "beanie" => &["beanie"],
            "headphones" => &["headphones"],
            "handheld_prop" => &["props"],
            "effects" => &["effects"],

            _ => &[],
        }
    }

    /// Ordered (layer, slot, variant, view) plan — no pixels. Deterministic and
    /// exactly follows `layer_order_front`; the basis for the render list.
    pub fn plan(def: &CharacterDefinition, state: &CharacterState) -> Vec<PlanEntry> {
        let mut out = Vec::new();
        for layer in &def.layer_order_front {
            for slot in Self::slots_for_layer(layer, state.view) {
                if state.is_hidden(slot) {
                    continue;
                }
                let Some(variant) = state.variant(slot) else { continue };
                // A slot with a `without_*` variant is a deliberate omission.
                if variant.starts_with("without_") {
                    continue;
                }
                out.push(PlanEntry {
                    layer: layer.clone(),
                    slot: (*slot).to_string(),
                    variant: variant.to_string(),
                    view: state.view.key().to_string(),
                });
            }
        }
        out
    }

    /// Resolve the plan against a manifest into a paint-ordered draw list, plus
    /// the list of slots that had no bound sprite.
    pub fn resolve(
        def: &CharacterDefinition,
        state: &CharacterState,
        manifest: &SpriteManifest,
    ) -> (Vec<DrawPart>, Vec<MissingPart>) {
        let mut parts = Vec::new();
        let mut missing = Vec::new();
        for e in Self::plan(def, state) {
            match manifest.resolve(&e.slot, &e.variant, &e.view) {
                Some(inst) => parts.push(to_draw(&e.layer, inst)),
                None => missing.push(MissingPart {
                    layer: e.layer,
                    slot: e.slot,
                    variant: e.variant,
                    view: e.view,
                }),
            }
        }
        (parts, missing)
    }
}

fn to_draw(layer: &str, inst: &SpriteInstance) -> DrawPart {
    DrawPart {
        layer: layer.to_string(),
        slot: inst.slot.clone(),
        variant: inst.variant.clone(),
        view: inst.view.clone(),
        source: inst.source.clone(),
        region: (inst.region.x, inst.region.y, inst.region.w, inst.region.h),
        dest: inst.canonical_bounds,
        pivot: inst.pivot,
        angle: inst.angle,
        alpha: 255,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modular::definition::CharacterDefinition;
    use crate::modular::manifest::{Region, SpriteInstance, SpriteManifest};
    use crate::modular::state::CharacterState;

    fn def() -> CharacterDefinition {
        // Full front layer order with a representative subset of slots.
        CharacterDefinition::from_json(
            r#"{
            "schema": "deskfolk.modular-character.v1",
            "id": "yasser", "name": "Y", "version": 1,
            "coordinate_system": { "canvas": {"width":320,"height":320} },
            "layer_order_front": ["rear_arm","rear_leg","torso","front_leg","front_arm",
                                  "head_base","face_features","beard","beanie","headphones"],
            "slots": {
                "left_upper_arm": {"pivot":[124,132],"variants":["down"]},
                "left_forearm": {"pivot":[113,172],"variants":["straight_down"]},
                "left_hand": {"pivot":[111,207],"variants":["relaxed"]},
                "left_leg": {"pivot":[145,207],"variants":["stand"]},
                "left_boot": {"pivot":[142,281],"variants":["flat"]},
                "pelvis": {"pivot":[160,206],"variants":["standing"]},
                "torso": {"pivot":[160,143],"variants":["idle_front"]},
                "right_leg": {"pivot":[175,207],"variants":["stand"]},
                "right_boot": {"pivot":[178,281],"variants":["flat"]},
                "right_upper_arm": {"pivot":[196,132],"variants":["down"]},
                "right_forearm": {"pivot":[207,172],"variants":["straight_down"]},
                "right_hand": {"pivot":[209,207],"variants":["relaxed"]},
                "head_base": {"pivot":[160,105],"variants":["front"]},
                "eyes": {"pivot":[160,77],"variants":["neutral"]},
                "eyebrows": {"pivot":[160,66],"variants":["neutral"]},
                "nose": {"pivot":[160,84],"variants":["front"]},
                "mouth": {"pivot":[160,98],"variants":["closed"]},
                "beard": {"pivot":[160,103],"variants":["neutral_front"]},
                "beanie": {"pivot":[160,66],"variants":["default_front"]},
                "headphones": {"pivot":[160,74],"variants":["on_ears_front"]}
            }
        }"#,
        )
        .unwrap()
    }

    #[test]
    fn plan_follows_layer_order_and_expands_groups() {
        let s = CharacterState::canonical_yasser(&def());
        let plan = LayerResolver::plan(&def(), &s);
        let slots: Vec<&str> = plan.iter().map(|e| e.slot.as_str()).collect();
        assert_eq!(
            slots,
            vec![
                // rear_arm
                "left_upper_arm", "left_forearm", "left_hand",
                // rear_leg
                "left_leg", "left_boot",
                // torso (pelvis under torso)
                "pelvis", "torso",
                // front_leg
                "right_leg", "right_boot",
                // front_arm
                "right_upper_arm", "right_forearm", "right_hand",
                // head
                "head_base",
                // face_features
                "eyes", "eyebrows", "nose", "mouth",
                "beard", "beanie", "headphones",
            ]
        );
        // rear arm strictly before torso before front arm
        let pos = |name: &str| slots.iter().position(|s| *s == name).unwrap();
        assert!(pos("left_hand") < pos("torso"));
        assert!(pos("torso") < pos("right_hand"));
        assert!(pos("head_base") < pos("beanie"));
    }

    #[test]
    fn hidden_and_without_variants_drop_out() {
        let mut s = CharacterState::canonical_yasser(&def());
        s.hide("headphones");
        s.set_variant(&def(), "beanie", "default_front").unwrap();
        // simulate a without_* selection by inserting directly
        s.variants.insert("beard".into(), "without_beard".into());
        let plan = LayerResolver::plan(&def(), &s);
        assert!(!plan.iter().any(|e| e.slot == "headphones"));
        assert!(!plan.iter().any(|e| e.slot == "beard"));
    }

    #[test]
    fn resolve_reports_missing_when_manifest_empty() {
        let s = CharacterState::canonical_yasser(&def());
        let (parts, missing) = LayerResolver::resolve(&def(), &s, &SpriteManifest::default());
        assert!(parts.is_empty());
        assert!(!missing.is_empty());
        assert!(missing.iter().any(|m| m.slot == "torso"));
    }

    #[test]
    fn resolve_binds_present_instances_in_order() {
        let s = CharacterState::canonical_yasser(&def());
        let mk = |slot: &str, variant: &str| SpriteInstance {
            id: format!("{slot}/{variant}"),
            slot: slot.into(),
            variant: variant.into(),
            view: "front".into(),
            source: "sheets/x.png".into(),
            region: Region { x: 0, y: 0, w: 10, h: 10 },
            canonical_bounds: Bounds { x: 0, y: 0, width: 10, height: 10 },
            pivot: Point(0, 0),
            anchors: Default::default(),
            angle: 0.0,
            layer: None,
            mirror_ok: false,
        };
        let manifest = SpriteManifest {
            definition_id: "yasser".into(),
            instances: vec![mk("torso", "idle_front"), mk("head_base", "front")],
        };
        let (parts, missing) = LayerResolver::resolve(&def(), &s, &manifest);
        assert_eq!(parts.len(), 2);
        // torso drawn before head_base
        assert_eq!(parts[0].slot, "torso");
        assert_eq!(parts[1].slot, "head_base");
        assert!(missing.iter().any(|m| m.slot == "eyes"));
    }
}
