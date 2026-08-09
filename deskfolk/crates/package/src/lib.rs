//! Character packages — the `.dfpk` format.
//!
//! A package is **data only**. It contains no engine code, and the engine
//! contains no knowledge of any particular character. The two meet through
//! [`Roles`]: the engine asks for "the sleep clip", the package says which of
//! *its* clips plays that role. That indirection is what lets a frog, a
//! pirate or a dragon drop into the same runtime with nothing recompiled.
//!
//! On disk a package is either a directory or a zip with the same layout:
//!
//! ```text
//! yasser.dfpk/
//!   character.json     manifest — everything below
//!   sprites/*.png      frames referenced by name (no extension in the manifest)
//!   voice/             optional voice assets
//!   README.md
//! ```

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub mod mask;
mod validate;
pub use mask::{AlphaMask, SpriteMasks, DEFAULT_ALPHA_THRESHOLD};
pub use validate::{Problem, Severity};

/// Manifest format version. Bumped only on breaking changes; the loader
/// refuses anything newer than it understands rather than guessing.
pub const FORMAT_VERSION: u32 = 1;

#[derive(Debug, thiserror::Error)]
pub enum PackageError {
    #[error("package not found: {0}")]
    NotFound(PathBuf),
    #[error("character.json is missing from {0}")]
    NoManifest(PathBuf),
    #[error("character.json is not valid JSON: {0}")]
    BadJson(#[from] serde_json::Error),
    #[error("package declares format {found}, this runtime supports up to {supported}")]
    FutureFormat { found: u32, supported: u32 },
    #[error("package is invalid:\n{}", .0.iter().map(|p| format!("  - {p}")).collect::<Vec<_>>().join("\n"))]
    Invalid(Vec<Problem>),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

// ---------------------------------------------------------------------------
// Manifest
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub format: u32,
    /// Stable machine id, e.g. `yasser`. Used for save-data paths.
    pub id: String,
    /// Display name shown to the user.
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub author: Option<String>,
    /// One line under the name in the Control Center, e.g.
    /// "Always here. Probably judging you."
    #[serde(default)]
    pub tagline: Option<String>,

    pub stage: Stage,
    #[serde(default)]
    pub sprites: SpriteSet,
    pub clips: BTreeMap<String, Clip>,
    pub emotions: BTreeMap<String, Emotion>,
    /// Mouth frames ordered by openness, indexed by voice level bucket.
    #[serde(default)]
    pub visemes: Vec<String>,
    pub roles: Roles,
    #[serde(default)]
    pub life: Life,
    #[serde(default)]
    pub personality: Personality,
    #[serde(default)]
    pub voice: Voice,
}

/// The character's own coordinate space. Sprite positions in clips are
/// expressed in these units and scaled at draw time, so a package authored
/// for a 412×412 stage still looks right on a 4K display.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Stage {
    pub width: u32,
    pub height: u32,
    /// Where the character's feet sit. Frames are drawn bottom-center anchored
    /// here, which is what makes sprites of differing heights line up.
    pub anchor_x: i32,
    pub anchor_y: i32,
    /// Where the FX overlay (hearts, notes, sweat) sits relative to the
    /// character: `fx_dx` from the character's center, with its *bottom* edge
    /// `fx_dy` below the character's top edge — i.e. beside the head.
    #[serde(default = "default_fx_dx")]
    pub fx_dx: i32,
    #[serde(default = "default_fx_dy")]
    pub fx_dy: i32,
    /// Center point for the full-frame glitch overlay. Defaults to the middle
    /// of the stage.
    #[serde(default)]
    pub glitch_center: Option<[i32; 2]>,
}

fn default_fx_dx() -> i32 {
    44
}

fn default_fx_dy() -> i32 {
    34
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpriteSet {
    #[serde(default = "default_sprite_dir")]
    pub dir: String,
    /// Pixel art must never be smoothed. Packages that are not pixel art can
    /// opt into linear filtering.
    #[serde(default)]
    pub filter: Filter,
}

impl Default for SpriteSet {
    fn default() -> Self {
        Self { dir: default_sprite_dir(), filter: Filter::default() }
    }
}

fn default_sprite_dir() -> String {
    "sprites".into()
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Filter {
    #[default]
    Nearest,
    Linear,
}

/// A named animation: frames, whether it loops, and an optional FX overlay
/// (hearts, music notes, sweat drops) drawn above-right of the head.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Clip {
    pub frames: Vec<Frame>,
    #[serde(default, rename = "loop")]
    pub looping: bool,
    #[serde(default)]
    pub fx: Vec<String>,
    /// ms between FX frames; 0 means the FX does not cycle.
    #[serde(default)]
    pub fx_ms: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Frame {
    /// Sprite basename, no extension — resolved against `sprites.dir`.
    pub img: String,
    pub ms: u32,
    /// Nudge from the anchor, in stage units. Small offsets here are most of
    /// what sells breathing and weight shifts.
    #[serde(default)]
    pub dx: i32,
    #[serde(default)]
    pub dy: i32,
}

/// Maps a mood name the mind can emit onto a clip and how it is played.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Emotion {
    pub clip: String,
    /// `true`: becomes the standing state and holds for `hold_ms`.
    /// `false`: a one-shot that plays over the current state and returns.
    #[serde(default)]
    pub base: bool,
    #[serde(default)]
    pub hold_ms: u32,
}

/// Clips the engine needs by function. The engine never names a clip
/// directly; it asks for a role and the package answers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Roles {
    pub idle: String,
    pub blink: String,
    pub talk: String,
    pub think: String,
    pub sleep: String,
    /// Played when startled awake.
    pub startle: String,
    /// Played when the mind reports a high glitch value.
    #[serde(default)]
    pub glitch: Option<String>,
    /// Full-frame overlay frames flashed during a glitch.
    #[serde(default)]
    pub glitch_fx: Vec<String>,
    /// The single sprite to use as his avatar/portrait in the UI (the talking
    /// face reads best). Optional — falls back to the talk clip's first frame —
    /// so every character, base or community, has a portrait without extra work.
    #[serde(default)]
    pub portrait: Option<String>,
}

// ---------------------------------------------------------------------------
// Aliveness tuning
// ---------------------------------------------------------------------------

/// Timings that govern the life loop. Every one of these is a dial a
/// character author can turn: a cat naps sooner, a ghost never sleeps.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Life {
    pub blink_every: RangeMs,
    /// Idle this long with no interaction and the character naps.
    pub nap_after_ms: u64,
    /// At night the bar is much lower, but not zero — he shouldn't nod off
    /// mid-conversation.
    pub night_nap_after_ms: u64,
    /// [start, end) in local hours, wrapping past midnight.
    pub night_hours: [u8; 2],
    /// How long after an interaction the character still counts as engaged
    /// (and will talk to itself).
    pub engaged_ms: u64,
    pub self_talk_every: RangeMs,
    #[serde(default = "default_true")]
    pub self_talk: bool,
    /// Idle fidgets played spontaneously while nothing else is happening —
    /// stretching, looking around, sipping coffee. The anti-frozen-sprite rule.
    #[serde(default)]
    pub fidget_every: Option<RangeMs>,
    #[serde(default)]
    pub fidgets: Vec<String>,

    /// How far the character leans toward the cursor, in stage units. Small
    /// on purpose: this should read as attention, not as the sprite sliding
    /// around. A dragon might lean further than a cat.
    #[serde(default = "default_gaze_px")]
    pub gaze_px: i32,
    /// How close the cursor has to get before he registers it.
    #[serde(default = "default_notice_radius")]
    pub notice_radius: i32,
    /// Played when the cursor arrives after being away — the double-take.
    #[serde(default)]
    pub notice_clip: Option<String>,

    /// How long you can be continuously at the machine before he says
    /// something unprompted about it ("you've been at this three hours").
    /// This is him starting the conversation rather than answering — set to 0
    /// for a character who never nags.
    #[serde(default = "default_nudge_after_ms")]
    pub nudge_after_ms: u64,
    /// Minimum gap between nudges, so he mentions it once and then lets it go.
    #[serde(default = "default_nudge_cooldown_ms")]
    pub nudge_cooldown_ms: u64,
}

fn default_nudge_after_ms() -> u64 {
    3 * 60 * 60 * 1000
}

fn default_nudge_cooldown_ms() -> u64 {
    45 * 60 * 1000
}

fn default_gaze_px() -> i32 {
    7
}

fn default_notice_radius() -> i32 {
    190
}

impl Default for Life {
    fn default() -> Self {
        Self {
            blink_every: RangeMs { min: 3_000, max: 8_000 },
            nap_after_ms: 20 * 60 * 1000,
            night_nap_after_ms: 90_000,
            night_hours: [22, 8],
            engaged_ms: 10 * 60 * 1000,
            self_talk_every: RangeMs { min: 60_000, max: 150_000 },
            self_talk: true,
            fidget_every: Some(RangeMs { min: 25_000, max: 70_000 }),
            fidgets: Vec::new(),
            gaze_px: default_gaze_px(),
            notice_radius: default_notice_radius(),
            notice_clip: None,
            nudge_after_ms: default_nudge_after_ms(),
            nudge_cooldown_ms: default_nudge_cooldown_ms(),
        }
    }
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct RangeMs {
    pub min: u64,
    pub max: u64,
}

impl RangeMs {
    /// Uniform pick. Tolerates an inverted or degenerate range rather than
    /// panicking on a hand-edited manifest.
    pub fn pick(&self, rng: &mut impl FnMut() -> f64) -> u64 {
        let (lo, hi) = if self.min <= self.max { (self.min, self.max) } else { (self.max, self.min) };
        if hi == lo {
            return lo;
        }
        lo + (rng() * (hi - lo) as f64) as u64
    }
}

// ---------------------------------------------------------------------------
// Personality & voice — data, never code
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Personality {
    /// Free-form character brief handed to the mind.
    #[serde(default)]
    pub prompt: String,
    /// Named 0..1 dials surfaced in the Control Center's Soul panel
    /// (reserved↔chatty, earnest↔dry, hands-off↔protective, …).
    #[serde(default)]
    pub traits: BTreeMap<String, f32>,
    /// Starting drives for the life loop (mood, energy, boredom, trust …).
    #[serde(default)]
    pub drives: BTreeMap<String, f32>,
    /// Lines used when the mind is unreachable, so the character still
    /// behaves like a character instead of showing an error dialog.
    #[serde(default)]
    pub offline_lines: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Voice {
    /// Preferred TTS engine hint, e.g. `groq-orpheus`, `kokoro`, `piper`.
    #[serde(default)]
    pub engine: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub pitch: Option<f32>,
    #[serde(default)]
    pub speed: Option<f32>,
}

// ---------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------

/// A loaded, validated package plus the root it was loaded from.
#[derive(Debug, Clone)]
pub struct CharacterPackage {
    pub manifest: Manifest,
    pub root: PathBuf,
    /// Non-fatal problems found during validation, worth surfacing in the
    /// Workshop but not worth refusing to run.
    pub warnings: Vec<Problem>,
}

impl CharacterPackage {
    /// Load from an unpacked package directory.
    pub fn load_dir(root: impl AsRef<Path>) -> Result<Self, PackageError> {
        let root = root.as_ref().to_path_buf();
        if !root.exists() {
            return Err(PackageError::NotFound(root));
        }
        let manifest_path = root.join("character.json");
        if !manifest_path.exists() {
            return Err(PackageError::NoManifest(root));
        }
        let raw = std::fs::read_to_string(&manifest_path)?;
        let manifest: Manifest = serde_json::from_str(&raw)?;

        if manifest.format > FORMAT_VERSION {
            return Err(PackageError::FutureFormat {
                found: manifest.format,
                supported: FORMAT_VERSION,
            });
        }

        let problems = validate::check(&manifest, &root);
        let (fatal, warnings): (Vec<_>, Vec<_>) =
            problems.into_iter().partition(|p| p.severity == Severity::Error);
        if !fatal.is_empty() {
            return Err(PackageError::Invalid(fatal));
        }

        Ok(Self { manifest, root, warnings })
    }

    /// Absolute path to a sprite referenced by basename.
    pub fn sprite_path(&self, name: &str) -> PathBuf {
        self.root.join(&self.manifest.sprites.dir).join(format!("{name}.png"))
    }

    /// Every sprite basename the manifest references, deduplicated. Used to
    /// preload the atlas and to build alpha hit-masks.
    pub fn referenced_sprites(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut push = |s: &str| {
            if !out.iter().any(|e| e == s) {
                out.push(s.to_string());
            }
        };
        for clip in self.manifest.clips.values() {
            for f in &clip.frames {
                push(&f.img);
            }
            for fx in &clip.fx {
                push(fx);
            }
        }
        for v in &self.manifest.visemes {
            push(v);
        }
        for g in &self.manifest.roles.glitch_fx {
            push(g);
        }
        out
    }

    /// Resolve a role name to the clip it points at.
    pub fn role_clip(&self, role: Role) -> &str {
        let r = &self.manifest.roles;
        match role {
            Role::Idle => &r.idle,
            Role::Blink => &r.blink,
            Role::Talk => &r.talk,
            Role::Think => &r.think,
            Role::Sleep => &r.sleep,
            Role::Startle => &r.startle,
            Role::Glitch => r.glitch.as_deref().unwrap_or(&r.idle),
        }
    }

    pub fn clip(&self, name: &str) -> Option<&Clip> {
        self.manifest.clips.get(name)
    }

    /// The sprite basename to show as his avatar: the declared `portrait` role
    /// if any, otherwise the first frame of his talk clip (the talking face),
    /// otherwise the idle clip's first frame. Always resolves to *something* a
    /// UI can render, for any character.
    pub fn portrait_sprite(&self) -> Option<String> {
        if let Some(p) = &self.manifest.roles.portrait {
            return Some(p.clone());
        }
        let first_frame = |clip: &str| {
            self.clip(clip).and_then(|c| c.frames.first()).map(|f| f.img.clone())
        };
        first_frame(&self.manifest.roles.talk).or_else(|| first_frame(&self.manifest.roles.idle))
    }

    /// Absolute path to the avatar PNG, if one resolves.
    pub fn portrait_path(&self) -> Option<PathBuf> {
        self.portrait_sprite().map(|s| self.sprite_path(&s))
    }
}

/// A one-line summary of a character package, for the picker in the wizard and
/// the Control Center — read without loading the whole package's sprites.
#[derive(Debug, Clone, Serialize)]
pub struct PackageSummary {
    pub id: String,
    pub name: String,
    pub tagline: Option<String>,
    pub author: Option<String>,
    /// Folder id under the characters dir, which the app loads by.
    pub dir: String,
    /// Avatar PNG path, if one resolves.
    pub portrait: Option<PathBuf>,
}

/// Discover the character packages under a directory: every immediate subfolder
/// that contains a valid `character.json`. Base and community packs are found
/// identically — a folder is a folder. Unreadable/invalid folders are skipped,
/// never fatal, so one broken pack can't hide the rest.
pub fn scan(characters_dir: impl AsRef<Path>) -> Vec<PackageSummary> {
    let dir = characters_dir.as_ref();
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        match CharacterPackage::load_dir(&path) {
            Ok(pkg) => {
                let folder = path.file_name().and_then(|s| s.to_str()).unwrap_or("").to_string();
                out.push(PackageSummary {
                    id: pkg.manifest.id.clone(),
                    name: pkg.manifest.name.clone(),
                    tagline: pkg.manifest.tagline.clone(),
                    author: pkg.manifest.author.clone(),
                    dir: folder,
                    portrait: pkg.portrait_path(),
                });
            }
            Err(e) => tracing::debug!("skipping {}: {e}", path.display()),
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Idle,
    Blink,
    Talk,
    Think,
    Sleep,
    Startle,
    Glitch,
}
