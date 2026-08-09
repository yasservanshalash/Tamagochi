//! The sprite *manifest* — the bridge from messy generated sheets to the clean
//! canonical coordinate system the [`CharacterDefinition`] describes.
//!
//! A [`SpriteInstance`] says: "the `right_hand`/`peace` part lives in cell
//! rect `region` of sheet `source`; when drawn, its content maps onto
//! `canonical_bounds` with its pivot at `pivot`." That indirection is what lets
//! one 1248×832 sheet serve many parts (regions, not duplicated files) while the
//! renderer still works in tidy 320-space.
//!
//! The manifest is authored by the web inspector. It starts effectively empty
//! because none of the currently generated sheets conform (see
//! `ASSET_AUDIT.md`); [`SpriteManifest::validate`] is what makes that gap
//! *visible and specific* rather than silently invented over.

use super::definition::{Bounds, CharacterDefinition, Point};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

/// A pixel rectangle inside a source sheet: `(x, y, w, h)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Region {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// One concrete, placeable sprite: a (slot, variant, view) bound to pixels.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpriteInstance {
    /// Stable id, conventionally `"{slot}/{variant}[/{view}]"`.
    pub id: String,
    pub slot: String,
    pub variant: String,
    /// Which facing this part is drawn for. `"front"` unless the part is
    /// view-specific (side/back/3q). Defaults to `"front"`.
    #[serde(default = "default_view")]
    pub view: String,
    /// Sheet path, relative to the manifest file's directory.
    pub source: String,
    /// The cell rectangle within `source`.
    pub region: Region,
    /// Where the region's content lands in 320-authoring-space.
    pub canonical_bounds: Bounds,
    /// The canonical pivot in 320-space (must equal the slot's spec pivot for a
    /// conforming part; `validate` flags mismatches).
    pub pivot: Point,
    /// Any connection anchors this part carries, in 320-space.
    #[serde(default)]
    pub anchors: BTreeMap<String, Point>,
    /// Rotation in degrees applied about the pivot (0 for canonical art).
    #[serde(default)]
    pub angle: f32,
    /// Explicit layer name; defaults to the slot's position in `layer_order_front`.
    #[serde(default)]
    pub layer: Option<String>,
    /// Marked true only for art the artist certified as left/right symmetrical,
    /// which alone may be mirror-reused. Never auto-set.
    #[serde(default)]
    pub mirror_ok: bool,
}

fn default_view() -> String {
    "front".to_string()
}

impl SpriteInstance {
    /// The layer this part draws on: its explicit `layer`, else its `slot`.
    pub fn layer_name(&self) -> &str {
        self.layer.as_deref().unwrap_or(&self.slot)
    }
}

/// A whole manifest: every bound part, plus the id of the definition it targets.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SpriteManifest {
    /// The `CharacterDefinition.id` this manifest is authored against.
    #[serde(default)]
    pub definition_id: String,
    #[serde(default)]
    pub instances: Vec<SpriteInstance>,
}

/// A single conformance problem, phrased so a human can act on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Warning {
    pub slot: String,
    pub kind: WarningKind,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WarningKind {
    /// A canonical slot has no instance at all.
    MissingSlot,
    /// Two instances claim the same (slot, variant, view).
    DuplicateInstance,
    /// An instance names a variant the slot does not list.
    UnknownVariant,
    /// The instance's pivot disagrees with the slot's canonical pivot.
    PivotMismatch,
    /// The instance's canonical_bounds differ from the slot's default bounds
    /// beyond tolerance — a strong hint the part was not authored on-canvas.
    BoundsMismatch,
    /// The instance targets a slot the definition does not define.
    UnknownSlot,
}

impl std::fmt::Display for Warning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {:?}: {}", self.slot, self.kind, self.detail)
    }
}

impl SpriteManifest {
    pub fn from_json(raw: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(raw)
    }

    pub fn load(path: impl AsRef<Path>) -> std::io::Result<Self> {
        let raw = std::fs::read_to_string(path)?;
        Ok(serde_json::from_str(&raw)?)
    }

    /// All instances for a slot, in manifest order.
    pub fn for_slot<'a>(&'a self, slot: &'a str) -> impl Iterator<Item = &'a SpriteInstance> {
        self.instances.iter().filter(move |i| i.slot == slot)
    }

    /// Find the instance matching a (slot, variant, view), falling back to any
    /// view of that variant, then the first instance of the variant.
    pub fn resolve(&self, slot: &str, variant: &str, view: &str) -> Option<&SpriteInstance> {
        self.instances
            .iter()
            .find(|i| i.slot == slot && i.variant == variant && i.view == view)
            .or_else(|| {
                self.instances
                    .iter()
                    .find(|i| i.slot == slot && i.variant == variant)
            })
    }

    /// Cross-check the manifest against the definition. Returns every problem
    /// found; an empty vec means fully conforming. This is intentionally
    /// non-fatal — the point is to *report*, per the "never invent a missing
    /// component" rule.
    pub fn validate(&self, def: &CharacterDefinition) -> Vec<Warning> {
        // Slots that legitimately have no numeric geometry / are not layered
        // parts — we do not warn when these are absent.
        const NON_PART_SLOTS: &[&str] = &["props"];
        // Tolerance in pixels for pivot / bounds agreement.
        const TOL: i32 = 2;

        let mut warnings = Vec::new();

        // 1) Unknown slots / variants, duplicates, pivot & bounds agreement.
        let mut seen: BTreeMap<(String, String, String), usize> = BTreeMap::new();
        for inst in &self.instances {
            let Some(slot_def) = def.slot(&inst.slot) else {
                warnings.push(Warning {
                    slot: inst.slot.clone(),
                    kind: WarningKind::UnknownSlot,
                    detail: format!("instance {:?} targets a slot not in the definition", inst.id),
                });
                continue;
            };

            let key = (inst.slot.clone(), inst.variant.clone(), inst.view.clone());
            *seen.entry(key).or_insert(0) += 1;

            if !slot_def.variants.is_empty() && !slot_def.allows(&inst.variant) {
                warnings.push(Warning {
                    slot: inst.slot.clone(),
                    kind: WarningKind::UnknownVariant,
                    detail: format!("variant {:?} is not listed for this slot", inst.variant),
                });
            }

            if let Some(pv) = slot_def.pivot {
                if (pv.x() - inst.pivot.x()).abs() > TOL || (pv.y() - inst.pivot.y()).abs() > TOL {
                    warnings.push(Warning {
                        slot: inst.slot.clone(),
                        kind: WarningKind::PivotMismatch,
                        detail: format!(
                            "pivot {:?} != canonical [{},{}]",
                            inst.pivot,
                            pv.x(),
                            pv.y()
                        ),
                    });
                }
            }

            if let Some(db) = slot_def.default_bounds {
                let b = inst.canonical_bounds;
                if (db.x - b.x).abs() > TOL
                    || (db.y - b.y).abs() > TOL
                    || (db.width - b.width).abs() > TOL
                    || (db.height - b.height).abs() > TOL
                {
                    warnings.push(Warning {
                        slot: inst.slot.clone(),
                        kind: WarningKind::BoundsMismatch,
                        detail: format!(
                            "bounds {}x{}@{},{} differ from canonical {}x{}@{},{}",
                            b.width, b.height, b.x, b.y, db.width, db.height, db.x, db.y
                        ),
                    });
                }
            }
        }
        for ((slot, variant, view), n) in seen {
            if n > 1 {
                warnings.push(Warning {
                    slot,
                    kind: WarningKind::DuplicateInstance,
                    detail: format!("{n} instances of variant {variant:?} view {view:?}"),
                });
            }
        }

        // 2) Slots the definition demands but the manifest never fills.
        for (name, slot_def) in &def.slots {
            if NON_PART_SLOTS.contains(&name.as_str()) {
                continue;
            }
            // Only warn about slots that are actual layered parts (have a pivot).
            if slot_def.pivot.is_none() {
                continue;
            }
            let has = self.instances.iter().any(|i| &i.slot == name);
            if !has {
                warnings.push(Warning {
                    slot: name.clone(),
                    kind: WarningKind::MissingSlot,
                    detail: "no conforming sprite bound for this slot".to_string(),
                });
            }
        }

        warnings
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
                "torso": { "default_bounds": {"x":103,"y":111,"width":114,"height":104},
                           "pivot": [160,143], "variants": ["idle_front"] },
                "head_base": { "default_bounds": {"x":124,"y":42,"width":72,"height":76},
                           "pivot": [160,105], "variants": ["front"] }
            }
        }"#,
        )
        .unwrap()
    }

    fn conforming_torso() -> SpriteInstance {
        SpriteInstance {
            id: "torso/idle_front".into(),
            slot: "torso".into(),
            variant: "idle_front".into(),
            view: "front".into(),
            source: "sheets/torso.png".into(),
            region: Region { x: 0, y: 0, w: 114, h: 104 },
            canonical_bounds: Bounds { x: 103, y: 111, width: 114, height: 104 },
            pivot: Point(160, 143),
            anchors: Default::default(),
            angle: 0.0,
            layer: None,
            mirror_ok: false,
        }
    }

    #[test]
    fn empty_manifest_reports_every_part_slot_missing() {
        let m = SpriteManifest::default();
        let w = m.validate(&def());
        // torso and head_base both missing.
        assert_eq!(w.iter().filter(|w| w.kind == WarningKind::MissingSlot).count(), 2);
    }

    #[test]
    fn conforming_instance_produces_no_warning_for_its_slot() {
        let m = SpriteManifest {
            definition_id: "yasser".into(),
            instances: vec![conforming_torso()],
        };
        let w = m.validate(&def());
        assert!(!w.iter().any(|w| w.slot == "torso"), "torso should be clean: {w:?}");
        // head_base still missing.
        assert!(w.iter().any(|w| w.slot == "head_base" && w.kind == WarningKind::MissingSlot));
    }

    #[test]
    fn flags_pivot_and_bounds_and_variant_and_unknown_slot() {
        let mut bad = conforming_torso();
        bad.pivot = Point(0, 0);
        bad.canonical_bounds = Bounds { x: 0, y: 0, width: 10, height: 10 };
        bad.variant = "not_a_variant".into();
        let unknown = SpriteInstance { slot: "wings".into(), ..conforming_torso() };
        let m = SpriteManifest {
            definition_id: "yasser".into(),
            instances: vec![bad, unknown],
        };
        let w = m.validate(&def());
        assert!(w.iter().any(|w| w.kind == WarningKind::PivotMismatch));
        assert!(w.iter().any(|w| w.kind == WarningKind::BoundsMismatch));
        assert!(w.iter().any(|w| w.kind == WarningKind::UnknownVariant));
        assert!(w.iter().any(|w| w.kind == WarningKind::UnknownSlot));
    }

    #[test]
    fn flags_duplicates() {
        let m = SpriteManifest {
            definition_id: "yasser".into(),
            instances: vec![conforming_torso(), conforming_torso()],
        };
        let w = m.validate(&def());
        assert!(w.iter().any(|w| w.kind == WarningKind::DuplicateInstance));
    }

    #[test]
    fn resolve_prefers_exact_view_then_falls_back() {
        let mut side = conforming_torso();
        side.view = "side_left".into();
        side.id = "torso/idle_front/side_left".into();
        let m = SpriteManifest {
            definition_id: "yasser".into(),
            instances: vec![conforming_torso(), side],
        };
        assert_eq!(m.resolve("torso", "idle_front", "front").unwrap().view, "front");
        assert_eq!(m.resolve("torso", "idle_front", "side_left").unwrap().view, "side_left");
        // unknown view falls back to first of variant
        assert!(m.resolve("torso", "idle_front", "back").is_some());
    }
}
