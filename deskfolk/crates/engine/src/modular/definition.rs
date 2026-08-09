//! The modular character *definition* — a faithful in-memory mirror of
//! `deskfolk_yasser_modular_character_v1.json`.
//!
//! This is the **source of truth** for geometry: the 320×320 authoring canvas,
//! the z-order of body parts, and for every slot its canonical bounds, pivot,
//! connection anchors and the list of variants it may take. Nothing here is
//! simplified away — fields the engine does not yet consume (palette, style
//! lock, customization categories) are preserved verbatim as [`serde_json::Value`]
//! so a round-trip never loses spec intent.
//!
//! A [`CharacterDefinition`] is immutable once loaded. The mutable, per-frame
//! choice of "which variant is showing" lives in
//! [`CharacterState`](crate::modular::state::CharacterState); the binding of a
//! variant to actual pixels lives in
//! [`SpriteManifest`](crate::modular::manifest::SpriteManifest).

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

/// The schema string every v1 definition must carry.
pub const SCHEMA_ID: &str = "deskfolk.modular-character.v1";

/// A 2-D point in authoring-canvas pixels. Stored as the spec stores them —
/// a bare `[x, y]` array — so JSON stays byte-for-byte round-trippable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Point(pub i32, pub i32);

impl Point {
    pub fn x(self) -> i32 {
        self.0
    }
    pub fn y(self) -> i32 {
        self.1
    }
}

/// An axis-aligned rectangle in authoring-canvas pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bounds {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// The shared authoring canvas and the reference frame every part is placed in.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoordinateSystem {
    pub canvas: Canvas,
    #[serde(default)]
    pub origin: Option<String>,
    #[serde(default)]
    pub character_bounds_front: Option<Bounds>,
    #[serde(default)]
    pub ground_y: Option<i32>,
    #[serde(default)]
    pub center_x: Option<i32>,
    /// Anything else the spec carries here (scale_rule, recommended_export…)
    /// is preserved but not interpreted.
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Canvas {
    pub width: i32,
    pub height: i32,
}

/// One slot's canonical geometry and its permitted variants.
///
/// `pivot` is the fixed point the part rotates/attaches about; every generated
/// sprite for this slot must place its pivot at exactly this coordinate so
/// parts stay interchangeable. `connection_anchors` are where *other* slots
/// join (e.g. a torso's `neck`/`left_shoulder`); the limb-specific
/// `end_anchor`/`knee_anchor`/`ankle_anchor` are captured explicitly because
/// the compatibility resolver reasons about them.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Slot {
    #[serde(default)]
    pub authoring_canvas: Option<[i32; 2]>,
    #[serde(default)]
    pub default_bounds: Option<Bounds>,
    #[serde(default)]
    pub pivot: Option<Point>,
    #[serde(default)]
    pub connection_anchors: BTreeMap<String, Point>,
    #[serde(default)]
    pub end_anchor: Option<Point>,
    #[serde(default)]
    pub knee_anchor: Option<Point>,
    #[serde(default)]
    pub ankle_anchor: Option<Point>,
    #[serde(default)]
    pub variants: Vec<String>,
    /// Slots like `props` carry a free-form `pivot_rule` string instead of a
    /// numeric pivot; keep whatever else appears without dropping it.
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

impl Slot {
    /// Is `variant` a legal choice for this slot?
    pub fn allows(&self, variant: &str) -> bool {
        self.variants.iter().any(|v| v == variant)
    }
}

/// The whole definition. Fields the engine consumes are typed; the remainder
/// (`style_lock`, `character_lock`, `customization_categories`, `generation_contract`)
/// is preserved as raw JSON so nothing the artist authored is lost.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CharacterDefinition {
    pub schema: String,
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub version: u32,
    pub coordinate_system: CoordinateSystem,
    /// Back-to-front paint order. The first entry is drawn first (furthest back).
    pub layer_order_front: Vec<String>,
    pub slots: BTreeMap<String, Slot>,
    #[serde(default)]
    pub skeleton_front: BTreeMap<String, Point>,
    #[serde(default)]
    pub gesture_presets: BTreeMap<String, BTreeMap<String, serde_json::Value>>,
    #[serde(default)]
    pub compatibility_rules: Vec<String>,
    /// Everything else, verbatim.
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

/// Why a definition could not be loaded.
#[derive(Debug)]
pub enum DefinitionError {
    Io(std::io::Error),
    Parse(serde_json::Error),
    /// The `schema` field is not the one this build understands.
    WrongSchema { found: String },
}

impl std::fmt::Display for DefinitionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DefinitionError::Io(e) => write!(f, "reading definition: {e}"),
            DefinitionError::Parse(e) => write!(f, "parsing definition JSON: {e}"),
            DefinitionError::WrongSchema { found } => {
                write!(f, "unsupported schema {found:?}, expected {SCHEMA_ID:?}")
            }
        }
    }
}

impl std::error::Error for DefinitionError {}

impl From<std::io::Error> for DefinitionError {
    fn from(e: std::io::Error) -> Self {
        DefinitionError::Io(e)
    }
}
impl From<serde_json::Error> for DefinitionError {
    fn from(e: serde_json::Error) -> Self {
        DefinitionError::Parse(e)
    }
}

impl CharacterDefinition {
    /// Parse a definition from a JSON string, checking the schema tag.
    pub fn from_json(raw: &str) -> Result<Self, DefinitionError> {
        let def: CharacterDefinition = serde_json::from_str(raw)?;
        if def.schema != SCHEMA_ID {
            return Err(DefinitionError::WrongSchema { found: def.schema });
        }
        Ok(def)
    }

    /// Load a definition from `definition.json` on disk.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, DefinitionError> {
        let raw = std::fs::read_to_string(path)?;
        Self::from_json(&raw)
    }

    /// The authoring canvas size, defaulting to 320×320 if the spec omitted it.
    pub fn canvas(&self) -> Canvas {
        self.coordinate_system.canvas
    }

    /// Look up a slot by name.
    pub fn slot(&self, name: &str) -> Option<&Slot> {
        self.slots.get(name)
    }

    /// The first variant listed for a slot — the spec's implicit default and
    /// the one `canonical_yasser` selects unless overridden.
    pub fn default_variant(&self, slot: &str) -> Option<&str> {
        self.slots.get(slot).and_then(|s| s.variants.first()).map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A trimmed but structurally faithful sample; the real-file round-trip is
    /// exercised by `tests/modular_definition.rs` against the packaged JSON.
    const SAMPLE: &str = r#"{
        "schema": "deskfolk.modular-character.v1",
        "id": "x",
        "name": "X",
        "version": 1,
        "coordinate_system": { "canvas": {"width":320,"height":320}, "ground_y": 290, "center_x": 160,
            "scale_rule": "keep" },
        "layer_order_front": ["torso","head_base"],
        "slots": {
            "torso": { "default_bounds": {"x":103,"y":111,"width":114,"height":104},
                       "pivot": [160,143],
                       "connection_anchors": {"neck":[160,108]},
                       "variants": ["idle_front","idle_side"] },
            "left_forearm": { "pivot": [113,172], "end_anchor": [111,207],
                       "variants": ["straight_down"] }
        },
        "skeleton_front": { "root": [160,290] },
        "gesture_presets": { "wave_right": { "right_hand": "wave_1" } },
        "compatibility_rules": ["Every asset must use the same 320x320 canvas."],
        "style_lock": { "anti_aliasing": false }
    }"#;

    #[test]
    fn parses_and_checks_schema() {
        let def = CharacterDefinition::from_json(SAMPLE).unwrap();
        assert_eq!(def.id, "x");
        assert_eq!(def.canvas().width, 320);
        assert_eq!(def.layer_order_front, vec!["torso", "head_base"]);
        assert_eq!(def.default_variant("torso"), Some("idle_front"));
        assert!(def.slot("torso").unwrap().allows("idle_side"));
        assert!(!def.slot("torso").unwrap().allows("nope"));
        // limb anchors captured
        assert_eq!(def.slot("left_forearm").unwrap().end_anchor, Some(Point(111, 207)));
    }

    #[test]
    fn preserves_unmodelled_fields() {
        let def = CharacterDefinition::from_json(SAMPLE).unwrap();
        // style_lock is not a typed field but must survive.
        assert!(def.extra.contains_key("style_lock"));
        // coordinate_system.scale_rule likewise.
        assert!(def.coordinate_system.extra.contains_key("scale_rule"));
    }

    #[test]
    fn rejects_wrong_schema() {
        let bad = SAMPLE.replace("modular-character.v1", "something-else.v9");
        assert!(matches!(
            CharacterDefinition::from_json(&bad),
            Err(DefinitionError::WrongSchema { .. })
        ));
    }

    #[test]
    fn round_trips_through_value() {
        let def = CharacterDefinition::from_json(SAMPLE).unwrap();
        let reser = serde_json::to_string(&def).unwrap();
        let again = CharacterDefinition::from_json(&reser).unwrap();
        assert_eq!(again.slots.len(), def.slots.len());
        assert_eq!(again.gesture_presets.len(), def.gesture_presets.len());
    }
}
