//! [`audit`] — turns a manifest's conformance against the definition into a
//! human-readable report.
//!
//! This is the "report exactly what's missing instead of inventing it" deliverable
//! in code form. It aggregates [`SpriteManifest::validate`] by slot, classifies
//! each part slot, and can render Markdown for `ASSET_AUDIT.md` or a dev log. It
//! reasons purely over the definition + manifest (no image decoding); the web
//! inspector supplies the pixel-level sheet analysis.

use super::definition::CharacterDefinition;
use super::manifest::{SpriteManifest, Warning, WarningKind};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlotStatus {
    /// At least one instance, and none of its warnings are problems.
    Conforming,
    /// Has instances but they carry warnings (pivot/bounds/variant/dupes).
    Issues,
    /// No instance bound at all.
    Missing,
}

#[derive(Debug, Clone)]
pub struct SlotReport {
    pub slot: String,
    pub status: SlotStatus,
    pub instances: usize,
    pub warnings: Vec<Warning>,
}

#[derive(Debug, Clone)]
pub struct ConformanceReport {
    pub definition_id: String,
    pub slots: Vec<SlotReport>,
}

impl ConformanceReport {
    pub fn part_slot_total(&self) -> usize {
        self.slots.len()
    }
    pub fn conforming(&self) -> usize {
        self.slots.iter().filter(|s| s.status == SlotStatus::Conforming).count()
    }
    pub fn missing(&self) -> usize {
        self.slots.iter().filter(|s| s.status == SlotStatus::Missing).count()
    }
    pub fn with_issues(&self) -> usize {
        self.slots.iter().filter(|s| s.status == SlotStatus::Issues).count()
    }
    /// True when every part slot has a conforming sprite — the green light for a
    /// fully assembled canonical Yasser.
    pub fn is_complete(&self) -> bool {
        self.missing() == 0 && self.with_issues() == 0
    }

    pub fn to_markdown(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!("# Modular asset conformance — `{}`\n\n", self.definition_id));
        s.push_str(&format!(
            "**{} / {} part slots conforming** · {} with issues · {} missing.\n\n",
            self.conforming(),
            self.part_slot_total(),
            self.with_issues(),
            self.missing()
        ));
        s.push_str("| slot | status | instances | notes |\n");
        s.push_str("| --- | --- | --: | --- |\n");
        for r in &self.slots {
            let status = match r.status {
                SlotStatus::Conforming => "✅ conforming",
                SlotStatus::Issues => "⚠️ issues",
                SlotStatus::Missing => "❌ missing",
            };
            let notes = if r.warnings.is_empty() {
                String::new()
            } else {
                r.warnings.iter().map(|w| format!("{:?}", w.kind)).collect::<Vec<_>>().join(", ")
            };
            s.push_str(&format!(
                "| `{}` | {} | {} | {} |\n",
                r.slot, status, r.instances, notes
            ));
        }
        s
    }
}

/// Build a conformance report for `manifest` against `def`.
pub fn audit(def: &CharacterDefinition, manifest: &SpriteManifest) -> ConformanceReport {
    let all = manifest.validate(def);

    // Group warnings by slot.
    let mut by_slot: BTreeMap<String, Vec<Warning>> = BTreeMap::new();
    for w in all {
        by_slot.entry(w.slot.clone()).or_default().push(w);
    }

    // Report over the definition's *part* slots (those with a pivot), which is
    // the same set `validate` treats as required.
    let mut slots = Vec::new();
    for (name, slot_def) in &def.slots {
        if slot_def.pivot.is_none() {
            continue;
        }
        let count = manifest.for_slot(name).count();
        let warnings = by_slot.get(name).cloned().unwrap_or_default();
        let status = if warnings.iter().any(|w| w.kind == WarningKind::MissingSlot) {
            SlotStatus::Missing
        } else if warnings.is_empty() {
            SlotStatus::Conforming
        } else {
            SlotStatus::Issues
        };
        slots.push(SlotReport { slot: name.clone(), status, instances: count, warnings });
    }

    ConformanceReport { definition_id: def.id.clone(), slots }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modular::definition::{Bounds, CharacterDefinition, Point};
    use crate::modular::manifest::{Region, SpriteInstance, SpriteManifest};

    fn def() -> CharacterDefinition {
        CharacterDefinition::from_json(
            r#"{
            "schema": "deskfolk.modular-character.v1",
            "id": "yasser", "name": "Y", "version": 1,
            "coordinate_system": { "canvas": {"width":320,"height":320} },
            "layer_order_front": ["torso","head_base"],
            "slots": {
                "torso": { "default_bounds": {"x":103,"y":111,"width":114,"height":104},
                           "pivot": [160,143], "variants": ["idle_front"] },
                "head_base": { "pivot": [160,105], "variants": ["front"] }
            }
        }"#,
        )
        .unwrap()
    }

    #[test]
    fn empty_manifest_is_all_missing() {
        let r = audit(&def(), &SpriteManifest::default());
        assert_eq!(r.part_slot_total(), 2);
        assert_eq!(r.missing(), 2);
        assert!(!r.is_complete());
        let md = r.to_markdown();
        assert!(md.contains("❌ missing"));
        assert!(md.contains("0 / 2 part slots conforming"));
    }

    #[test]
    fn one_good_one_missing() {
        let m = SpriteManifest {
            definition_id: "yasser".into(),
            instances: vec![SpriteInstance {
                id: "torso/idle_front".into(),
                slot: "torso".into(),
                variant: "idle_front".into(),
                view: "front".into(),
                source: "s.png".into(),
                region: Region { x: 0, y: 0, w: 114, h: 104 },
                canonical_bounds: Bounds { x: 103, y: 111, width: 114, height: 104 },
                pivot: Point(160, 143),
                anchors: Default::default(),
                angle: 0.0,
                layer: None,
                mirror_ok: false,
            }],
        };
        let r = audit(&def(), &m);
        assert_eq!(r.conforming(), 1);
        assert_eq!(r.missing(), 1);
    }
}
