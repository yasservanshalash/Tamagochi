//! Modular character assembly.
//!
//! A parallel, data-driven character pipeline that sits *beside* the existing
//! single-sprite [`ClipPlayer`](crate::ClipPlayer)/[`compose`](crate::compose)
//! path — it changes none of it. Where the classic engine names one whole
//! sprite per frame, the modular pipeline assembles a character from independent
//! parts laid out on a shared 320×320 canvas:
//!
//! ```text
//! CharacterDefinition  (the spec: slots, pivots, anchors, layer order)   definition.rs
//!         │
//!         ▼
//! SpriteManifest       (parts → sheet regions in canonical coords)        manifest.rs
//!         │
//!         ▼
//! CharacterState       (which variant per slot, view, hidden layers)      state.rs
//!         │   ▲ AnimationPreset mutates only the slots a gesture names     preset.rs
//!         ▼   │
//! CompatibilityResolver (rejects impossible combinations, reports)        compat.rs
//!         │
//!         ▼
//! LayerResolver        (back-to-front draw list, reports missing parts)   layer.rs
//!         │
//!         ▼
//! (render-win::modular_paint blits each region at its canonical dest)
//! ```
//!
//! [`audit`] reports how well a manifest satisfies a definition — the honest
//! accounting of which parts exist versus which the spec demands.

pub mod audit;
pub mod compat;
pub mod definition;
pub mod layer;
pub mod manifest;
pub mod preset;
pub mod state;

pub use audit::{audit, ConformanceReport, SlotStatus};
pub use compat::{CompatWarning, CompatibilityResolver};
pub use definition::{Bounds, CharacterDefinition, DefinitionError, Point, Slot};
pub use layer::{DrawPart, LayerResolver, MissingPart, PlanEntry};
pub use manifest::{Region, SpriteInstance, SpriteManifest, Warning, WarningKind};
pub use preset::{Applied, AnimationPreset};
pub use state::{CharacterState, View};
