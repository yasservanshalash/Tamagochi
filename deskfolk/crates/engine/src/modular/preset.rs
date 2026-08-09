//! [`AnimationPreset`] — applies a named `gesture_preset` from the definition to
//! a [`CharacterState`], touching *only* the slots the gesture names.
//!
//! This is the whole point of a modular rig: a wave changes `right_upper_arm`,
//! `right_forearm` and `right_hand` and leaves the face, torso and legs exactly
//! as they were. Presets read straight from the spec's `gesture_presets`, so
//! the artist's JSON — not code — decides what a gesture is.
//!
//! Two value shapes appear in the spec: a plain `"slot": "variant"` mapping, and
//! a `"slot_sequence": ["v1","v2",...]` list for parts that cycle (a waving
//! hand). Sequences are sampled by frame index; a static apply uses frame 0.

use super::definition::CharacterDefinition;
use super::state::CharacterState;

/// The outcome of applying a preset: which slots changed, and any variant that
/// the preset asked for but the definition rejected (surfaced, never silently
/// dropped).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Applied {
    pub touched: Vec<String>,
    pub rejected: Vec<String>,
}

pub struct AnimationPreset;

impl AnimationPreset {
    /// Names of every gesture the definition offers.
    pub fn names(def: &CharacterDefinition) -> Vec<String> {
        def.gesture_presets.keys().cloned().collect()
    }

    /// The slots a preset would touch, without mutating anything.
    pub fn slots_of(def: &CharacterDefinition, preset: &str) -> Option<Vec<String>> {
        let entries = def.gesture_presets.get(preset)?;
        Some(entries.keys().map(|k| base_slot(k).to_string()).collect())
    }

    /// Apply `preset` at animation `frame` (0 for a static pose). Mutates only
    /// the named slots. Returns which slots changed and which variant requests
    /// were rejected by the definition.
    pub fn apply_frame(
        def: &CharacterDefinition,
        state: &mut CharacterState,
        preset: &str,
        frame: usize,
    ) -> Result<Applied, String> {
        let entries = def
            .gesture_presets
            .get(preset)
            .ok_or_else(|| format!("unknown gesture preset {preset:?}"))?
            .clone();

        let mut applied = Applied::default();
        for (key, val) in &entries {
            let slot = base_slot(key);
            let variant = match val {
                // "slot": "variant"
                serde_json::Value::String(s) => s.clone(),
                // "slot_sequence": ["v1","v2",...]
                serde_json::Value::Array(items) if !items.is_empty() => {
                    let idx = frame % items.len();
                    match items[idx].as_str() {
                        Some(s) => s.to_string(),
                        None => {
                            applied.rejected.push(format!("{key}[{idx}] is not a string"));
                            continue;
                        }
                    }
                }
                _ => {
                    applied.rejected.push(format!("{key} has an unsupported value shape"));
                    continue;
                }
            };

            match state.set_variant(def, slot, &variant) {
                Ok(()) => applied.touched.push(slot.to_string()),
                Err(e) => applied.rejected.push(e),
            }
        }
        Ok(applied)
    }

    /// Apply a preset as a static pose (frame 0).
    pub fn apply(
        def: &CharacterDefinition,
        state: &mut CharacterState,
        preset: &str,
    ) -> Result<Applied, String> {
        Self::apply_frame(def, state, preset, 0)
    }
}

/// `"right_hand_sequence"` → `"right_hand"`; everything else is already a slot.
fn base_slot(key: &str) -> &str {
    key.strip_suffix("_sequence").unwrap_or(key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modular::definition::CharacterDefinition;
    use crate::modular::state::CharacterState;

    fn def() -> CharacterDefinition {
        CharacterDefinition::from_json(
            r#"{
            "schema": "deskfolk.modular-character.v1",
            "id": "yasser", "name": "Y", "version": 1,
            "coordinate_system": { "canvas": {"width":320,"height":320} },
            "layer_order_front": ["torso"],
            "slots": {
                "right_upper_arm": {"pivot":[196,132],"variants":["down","up_90"]},
                "right_forearm": {"pivot":[207,172],"variants":["straight_down","wave_position"]},
                "right_hand": {"pivot":[209,207],"variants":["relaxed","wave_1","wave_2"]},
                "eyes": {"pivot":[160,77],"variants":["neutral","look_up"]},
                "eyebrows": {"pivot":[160,66],"variants":["neutral","raised_left"]},
                "mouth": {"pivot":[160,98],"variants":["closed","neutral_open"]},
                "left_hand": {"pivot":[111,207],"variants":["relaxed"]}
            },
            "gesture_presets": {
                "wave_right": {
                    "right_upper_arm": "up_90",
                    "right_forearm": "wave_position",
                    "right_hand_sequence": ["wave_1","wave_2","wave_1","wave_2"]
                },
                "thinking": {
                    "eyes": "look_up",
                    "eyebrows": "raised_left",
                    "mouth": "closed",
                    "right_hand": "not_a_real_variant"
                }
            }
        }"#,
        )
        .unwrap()
    }

    #[test]
    fn wave_touches_only_the_right_arm_chain() {
        let d = def();
        let mut s = CharacterState::canonical_yasser(&d);
        // baseline snapshot of untouched slots
        let eyes_before = s.variant("eyes").map(String::from);
        let left_before = s.variant("left_hand").map(String::from);

        let applied = AnimationPreset::apply(&d, &mut s, "wave_right").unwrap();
        let mut touched = applied.touched.clone();
        touched.sort();
        assert_eq!(touched, vec!["right_forearm", "right_hand", "right_upper_arm"]);
        assert!(applied.rejected.is_empty());

        assert_eq!(s.variant("right_upper_arm"), Some("up_90"));
        assert_eq!(s.variant("right_forearm"), Some("wave_position"));
        assert_eq!(s.variant("right_hand"), Some("wave_1")); // frame 0
        // untouched slots unchanged
        assert_eq!(s.variant("eyes").map(String::from), eyes_before);
        assert_eq!(s.variant("left_hand").map(String::from), left_before);
    }

    #[test]
    fn sequence_samples_by_frame() {
        let d = def();
        let mut s = CharacterState::canonical_yasser(&d);
        AnimationPreset::apply_frame(&d, &mut s, "wave_right", 1).unwrap();
        assert_eq!(s.variant("right_hand"), Some("wave_2"));
        AnimationPreset::apply_frame(&d, &mut s, "wave_right", 2).unwrap();
        assert_eq!(s.variant("right_hand"), Some("wave_1"));
    }

    #[test]
    fn rejected_variant_is_reported_not_applied() {
        let d = def();
        let mut s = CharacterState::canonical_yasser(&d);
        let applied = AnimationPreset::apply(&d, &mut s, "thinking").unwrap();
        assert!(applied.touched.contains(&"eyes".to_string()));
        assert_eq!(s.variant("eyes"), Some("look_up"));
        // the bogus right_hand variant was rejected, not set
        assert!(!applied.rejected.is_empty());
        assert_ne!(s.variant("right_hand"), Some("not_a_real_variant"));
    }

    #[test]
    fn slots_of_lists_base_slots() {
        let d = def();
        let mut slots = AnimationPreset::slots_of(&d, "wave_right").unwrap();
        slots.sort();
        assert_eq!(slots, vec!["right_forearm", "right_hand", "right_upper_arm"]);
        assert!(AnimationPreset::slots_of(&d, "nope").is_none());
    }
}
