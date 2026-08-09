//! [`CharacterState`] — the mutable, per-frame answer to "what is Yasser wearing
//! and doing right now?"
//!
//! It is deliberately thin: for each slot, which *variant* is showing, plus a
//! global *view* (front/side/…) and which layers are hidden. Animation presets
//! and the compatibility resolver mutate this; the layer resolver reads it.
//! Nothing here touches pixels — that binding is the manifest's job.

use super::definition::CharacterDefinition;
use std::collections::BTreeMap;
use std::collections::BTreeSet;

/// Which way the character is facing. Drives view-specific art selection and
/// front/rear limb ordering. Only `Front` is fully wired in this pass; the rest
/// are scaffolded so the manifest and resolver can grow into them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Front,
    ThreeQuarterLeft,
    ThreeQuarterRight,
    SideLeft,
    SideRight,
    Back,
}

impl View {
    /// The manifest `view` string this facing prefers.
    pub fn key(self) -> &'static str {
        match self {
            View::Front => "front",
            View::ThreeQuarterLeft => "3q_left",
            View::ThreeQuarterRight => "3q_right",
            View::SideLeft => "side_left",
            View::SideRight => "side_right",
            View::Back => "back",
        }
    }
}

/// The live assembly state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CharacterState {
    /// slot name -> chosen variant name.
    pub variants: BTreeMap<String, String>,
    /// Global facing.
    pub view: View,
    /// Slots explicitly hidden (e.g. the inspector's layer toggles, or
    /// `without_beanie` handled as a hide rather than a variant).
    pub hidden: BTreeSet<String>,
}

impl CharacterState {
    /// The canonical Yasser: every layered slot set to its spec-default variant
    /// (the first in each slot's `variants` list), facing front, nothing hidden.
    /// This is the "Reset to canonical Yasser" target.
    pub fn canonical_yasser(def: &CharacterDefinition) -> Self {
        let mut variants = BTreeMap::new();
        for (name, slot) in &def.slots {
            // Only slots that are layered parts (have a pivot) and offer variants.
            if slot.pivot.is_some() {
                if let Some(v) = slot.variants.first() {
                    variants.insert(name.clone(), v.clone());
                }
            }
        }
        Self { variants, view: View::Front, hidden: BTreeSet::new() }
    }

    /// Choose a variant for a slot, validating it against the definition.
    /// Returns `Err` with the offending variant if the slot doesn't allow it,
    /// leaving state unchanged — callers surface this rather than silently
    /// accepting an impossible combination.
    pub fn set_variant(
        &mut self,
        def: &CharacterDefinition,
        slot: &str,
        variant: &str,
    ) -> Result<(), String> {
        match def.slot(slot) {
            Some(s) if s.variants.is_empty() || s.allows(variant) => {
                self.variants.insert(slot.to_string(), variant.to_string());
                Ok(())
            }
            Some(_) => Err(format!("variant {variant:?} not allowed for slot {slot:?}")),
            None => Err(format!("unknown slot {slot:?}")),
        }
    }

    /// The variant currently selected for a slot, if any.
    pub fn variant(&self, slot: &str) -> Option<&str> {
        self.variants.get(slot).map(String::as_str)
    }

    pub fn hide(&mut self, slot: &str) {
        self.hidden.insert(slot.to_string());
    }
    pub fn show(&mut self, slot: &str) {
        self.hidden.remove(slot);
    }
    pub fn is_hidden(&self, slot: &str) -> bool {
        self.hidden.contains(slot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modular::definition::CharacterDefinition;

    fn def() -> CharacterDefinition {
        CharacterDefinition::from_json(
            r#"{
            "schema": "deskfolk.modular-character.v1",
            "id": "yasser", "name": "Y", "version": 1,
            "coordinate_system": { "canvas": {"width":320,"height":320} },
            "layer_order_front": ["torso","head_base"],
            "slots": {
                "torso": { "pivot": [160,143], "variants": ["idle_front","idle_side"] },
                "eyes": { "pivot": [160,77], "variants": ["neutral","blink","happy"] },
                "props": { "variants": ["coffee_cup","phone"] }
            }
        }"#,
        )
        .unwrap()
    }

    #[test]
    fn canonical_picks_first_variant_of_each_part_slot() {
        let s = CharacterState::canonical_yasser(&def());
        assert_eq!(s.variant("torso"), Some("idle_front"));
        assert_eq!(s.variant("eyes"), Some("neutral"));
        // props has no pivot -> not a layered part -> not seeded.
        assert_eq!(s.variant("props"), None);
        assert_eq!(s.view, View::Front);
    }

    #[test]
    fn set_variant_validates() {
        let mut s = CharacterState::canonical_yasser(&def());
        assert!(s.set_variant(&def(), "eyes", "blink").is_ok());
        assert_eq!(s.variant("eyes"), Some("blink"));
        // rejected, state unchanged
        assert!(s.set_variant(&def(), "eyes", "laser").is_err());
        assert_eq!(s.variant("eyes"), Some("blink"));
        assert!(s.set_variant(&def(), "nope", "x").is_err());
    }

    #[test]
    fn hide_show_roundtrip() {
        let mut s = CharacterState::canonical_yasser(&def());
        assert!(!s.is_hidden("torso"));
        s.hide("torso");
        assert!(s.is_hidden("torso"));
        s.show("torso");
        assert!(!s.is_hidden("torso"));
    }
}
