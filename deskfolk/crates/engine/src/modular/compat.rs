//! [`CompatibilityResolver`] — turns the spec's prose `compatibility_rules` into
//! machine checks over a [`CharacterState`].
//!
//! It **reports**, it does not silently repair: an impossible combination
//! (a front-facing head wearing a side-profile beanie, a coffee grip with no
//! cup) surfaces as a [`CompatWarning`] the inspector shows and the dev log
//! prints. That honours the project rule to flag rather than invent.
//!
//! Two rules are actionable from state alone and implemented here:
//! * **"Head angle determines the compatible … set."** Every view-encoded head
//!   slot must agree with the character's facing.
//! * **"Hand pose determines the compatible forearm ending and prop grip."**
//!   A `*_grip` hand implies a specific forearm variant and a held prop.
//!
//! Runtime scaling and nearest-neighbour rules are enforced structurally by the
//! renderer (there is simply no per-part scale path); mirroring is gated by the
//! manifest's `mirror_ok` flag. Those are noted, not re-checked here.

use super::definition::CharacterDefinition;
use super::state::{CharacterState, View};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompatWarning {
    pub rule: &'static str,
    pub detail: String,
}

impl std::fmt::Display for CompatWarning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.rule, self.detail)
    }
}

/// What a `*_grip` hand variant requires elsewhere.
struct GripReq {
    hand: &'static str,
    /// The forearm variant that mechanically matches this grip, if any.
    forearm: Option<&'static str>,
    /// The prop that must be held for this grip to make sense, if any.
    prop: Option<&'static str>,
}

/// The grip → (forearm, prop) table, read off the spec's variant vocabularies
/// and `gesture_presets`. Hands whose grip implies nothing extra are omitted.
const GRIPS: &[GripReq] = &[
    GripReq { hand: "coffee_grip", forearm: Some("coffee_position"), prop: Some("coffee_cup") },
    GripReq { hand: "phone_grip", forearm: None, prop: Some("phone") },
    GripReq { hand: "controller_grip", forearm: None, prop: Some("game_controller") },
    GripReq { hand: "keyboard_pose", forearm: None, prop: Some("keyboard") },
    GripReq { hand: "mouse_pose", forearm: None, prop: Some("mouse") },
    GripReq { hand: "joint_grip", forearm: None, prop: Some("joint") },
];

/// Head-region slots whose *variant name encodes a view* and must therefore
/// agree with the character's facing.
const VIEW_ENCODED_HEAD_SLOTS: &[&str] =
    &["head_base", "beanie", "headphones", "beard", "nose"];

pub struct CompatibilityResolver;

impl CompatibilityResolver {
    /// Check a state for contradictions. Empty result = coherent.
    pub fn check(def: &CharacterDefinition, state: &CharacterState) -> Vec<CompatWarning> {
        let mut out = Vec::new();
        Self::check_head_view(def, state, &mut out);
        Self::check_hand_grips(def, state, &mut out);
        out
    }

    /// "Head angle determines the compatible beanie, headphones, beard, nose,
    /// eyes and mouth set." A variant naming a *different* facing than the
    /// character is a mismatch.
    fn check_head_view(def: &CharacterDefinition, state: &CharacterState, out: &mut Vec<CompatWarning>) {
        let want = state.view.key(); // "front", "side_left", ...
        for slot in VIEW_ENCODED_HEAD_SLOTS {
            if state.is_hidden(slot) {
                continue;
            }
            let Some(variant) = state.variant(slot) else { continue };
            if !def.slot(slot).map(|s| s.allows(variant)).unwrap_or(true) {
                continue; // unknown-variant is manifest's problem, not ours
            }
            let encodes = encoded_view(variant);
            if let Some(v) = encodes {
                if v != want {
                    out.push(CompatWarning {
                        rule: "head_angle_set",
                        detail: format!(
                            "{slot} variant {variant:?} is a {v} part but the head faces {want}"
                        ),
                    });
                }
            } else if state.view != View::Front {
                // Variant carries no view token (i.e. it's the front form) while
                // the head is turned.
                out.push(CompatWarning {
                    rule: "head_angle_set",
                    detail: format!(
                        "{slot} variant {variant:?} has no {want} form selected for a turned head"
                    ),
                });
            }
        }
    }

    /// "Hand pose determines the compatible forearm ending and prop grip
    /// anchor." A grip hand needs its matching forearm and a held prop.
    fn check_hand_grips(_def: &CharacterDefinition, state: &CharacterState, out: &mut Vec<CompatWarning>) {
        for (hand_slot, forearm_slot) in
            [("left_hand", "left_forearm"), ("right_hand", "right_forearm")]
        {
            let Some(hand) = state.variant(hand_slot) else { continue };
            let Some(req) = GRIPS.iter().find(|g| g.hand == hand) else { continue };

            if let Some(want_forearm) = req.forearm {
                let fore = state.variant(forearm_slot);
                if fore != Some(want_forearm) {
                    out.push(CompatWarning {
                        rule: "hand_pose_forearm_prop",
                        detail: format!(
                            "{hand_slot}={hand:?} expects {forearm_slot}={want_forearm:?}, found {fore:?}"
                        ),
                    });
                }
            }
            if let Some(want_prop) = req.prop {
                let prop = state.variant("props");
                if prop != Some(want_prop) {
                    out.push(CompatWarning {
                        rule: "hand_pose_forearm_prop",
                        detail: format!(
                            "{hand_slot}={hand:?} should hold prop {want_prop:?}, found {prop:?}"
                        ),
                    });
                }
            }
        }
    }
}

/// Extract the facing a variant name encodes, if any. Matches the spec's tokens
/// (`front`, `3q_left`, `3q_right`, `side_left`, `side_right`, `back`), longest
/// first so `side_left` wins over a bare `left`.
fn encoded_view(variant: &str) -> Option<&'static str> {
    const KEYS: &[&str] = &["3q_left", "3q_right", "side_left", "side_right", "back", "front"];
    for k in KEYS {
        if variant.contains(k) {
            return Some(k);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modular::definition::CharacterDefinition;
    use crate::modular::state::{CharacterState, View};

    fn def() -> CharacterDefinition {
        CharacterDefinition::from_json(
            r#"{
            "schema": "deskfolk.modular-character.v1",
            "id": "yasser", "name": "Y", "version": 1,
            "coordinate_system": { "canvas": {"width":320,"height":320} },
            "layer_order_front": ["torso"],
            "slots": {
                "head_base": { "pivot": [160,105], "variants": ["front","side_left","3q_left","back"] },
                "beanie": { "pivot": [160,66], "variants": ["default_front","default_side_left"] },
                "nose": { "pivot": [160,84], "variants": ["front","side_left"] },
                "right_forearm": { "pivot": [207,172], "variants": ["straight_down","coffee_position"] },
                "right_hand": { "pivot": [209,207], "variants": ["relaxed","coffee_grip","phone_grip"] },
                "props": { "variants": ["coffee_cup","phone"] }
            }
        }"#,
        )
        .unwrap()
    }

    #[test]
    fn canonical_front_is_coherent() {
        let mut s = CharacterState::canonical_yasser(&def());
        s.set_variant(&def(), "beanie", "default_front").unwrap();
        s.set_variant(&def(), "nose", "front").unwrap();
        let w = CompatibilityResolver::check(&def(), &s);
        assert!(w.is_empty(), "expected coherent, got {w:?}");
    }

    #[test]
    fn side_beanie_on_front_head_warns() {
        let mut s = CharacterState::canonical_yasser(&def());
        s.set_variant(&def(), "beanie", "default_side_left").unwrap();
        let w = CompatibilityResolver::check(&def(), &s);
        assert!(w.iter().any(|w| w.rule == "head_angle_set"), "{w:?}");
    }

    #[test]
    fn turned_head_wants_turned_parts() {
        let mut s = CharacterState::canonical_yasser(&def());
        s.view = View::SideLeft;
        s.set_variant(&def(), "head_base", "side_left").unwrap();
        s.set_variant(&def(), "beanie", "default_side_left").unwrap();
        // nose still front -> should warn
        s.set_variant(&def(), "nose", "front").unwrap();
        let w = CompatibilityResolver::check(&def(), &s);
        assert!(w.iter().any(|w| w.detail.contains("nose")), "{w:?}");
    }

    #[test]
    fn coffee_grip_needs_forearm_and_cup() {
        let mut s = CharacterState::canonical_yasser(&def());
        s.set_variant(&def(), "right_hand", "coffee_grip").unwrap();
        // no matching forearm, no prop
        let w = CompatibilityResolver::check(&def(), &s);
        assert!(w.iter().filter(|w| w.rule == "hand_pose_forearm_prop").count() >= 2, "{w:?}");

        s.set_variant(&def(), "right_forearm", "coffee_position").unwrap();
        s.set_variant(&def(), "props", "coffee_cup").unwrap();
        let w2 = CompatibilityResolver::check(&def(), &s);
        assert!(!w2.iter().any(|w| w.rule == "hand_pose_forearm_prop"), "{w2:?}");
    }
}
