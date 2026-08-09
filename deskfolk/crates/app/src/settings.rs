//! The single persisted source of truth for how the companion is set up.
//!
//! Until now behaviour was scattered across environment variables and a tiny
//! `audio.json`. The onboarding wizard and the Control Center need one place to
//! read and write everything — who he is, how he thinks, how much he meddles,
//! how he sounds, and which brain he runs on — so this is that place: a single
//! serde struct saved as `settings.json` next to `audio.json` in the app data
//! dir. Every field has a default, so an older or partial file still loads, and
//! a fresh install starts from the design's sensible defaults.
//!
//! Nothing here is ever logged with a key in it — the provider is stored but the
//! only log-safe printer lives on `ProviderConfig` itself.

use std::path::PathBuf;

use deskfolk_ai::ProviderConfig;
use serde::{Deserialize, Serialize};

/// The default character id, matching the one the app boots when nothing is set.
pub fn default_character() -> String {
    "yasser".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    /// Has the user been through the wizard? Drives whether it opens on launch.
    #[serde(default)]
    pub first_run_complete: bool,
    /// Which package under `characters/` he is. Base and community packs are
    /// indistinguishable here — a folder id is a folder id.
    #[serde(default = "default_character")]
    pub character: String,
    /// What the user chose to call him — his display name and his wake word.
    /// `None` uses the package's own name.
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub brain: Brain,
    #[serde(default)]
    pub personality: Personality,
    #[serde(default)]
    pub habits: Habits,
    #[serde(default)]
    pub voice: Voice,
    #[serde(default)]
    pub connect: Connect,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            first_run_complete: false,
            character: default_character(),
            name: None,
            brain: Brain::default(),
            personality: Personality::default(),
            habits: Habits::default(),
            voice: Voice::default(),
            connect: Connect::default(),
        }
    }
}

// --- the brain: basic (bundled local) vs advanced (your own key/model) -------

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Brain {
    #[serde(default)]
    pub tier: Tier,
    /// The provider to use in the advanced tier. `None` (the basic default)
    /// means "use the bundled local brain" — no key, no cost, nothing leaves
    /// the machine.
    #[serde(default)]
    pub provider: Option<ProviderConfig>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    /// The free, private default: the bundled local model via the sidecar brain.
    #[default]
    Basic,
    /// The user brought their own cloud key and model.
    Advanced,
}

// --- personality & psychology ------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Personality {
    pub warmth: u8,
    pub humor: u8,
    pub edge: u8,
    pub chattiness: u8,
    pub curiosity: u8,
    pub guardedness: u8,
    /// How strongly the user's openness reshapes him — scales the rapport drift.
    #[serde(default)]
    pub mirror: Mirror,
    #[serde(default)]
    pub baseline_mood: BaselineMood,
    /// 18+ register: lets him swear and speak plainly about adult topics.
    #[serde(default)]
    pub adult_register: bool,
}

impl Default for Personality {
    fn default() -> Self {
        // The design's "rough defaults".
        Self {
            warmth: 70,
            humor: 80,
            edge: 45,
            chattiness: 55,
            curiosity: 65,
            guardedness: 30,
            mirror: Mirror::Strong,
            baseline_mood: BaselineMood::Chill,
            adult_register: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Mirror {
    Subtle,
    Medium,
    #[default]
    Strong,
}

impl Mirror {
    /// Multiplier applied to the rapport engine's per-turn drift.
    pub fn factor(self) -> f32 {
        match self {
            Mirror::Subtle => 0.5,
            Mirror::Medium => 1.0,
            Mirror::Strong => 1.6,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum BaselineMood {
    #[default]
    Chill,
    Upbeat,
    Deadpan,
}

// --- habits & autonomy -------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Habits {
    #[serde(default)]
    pub nudges: NudgeLevel,
    /// `None` = never quiet. Otherwise a daily window he stays put and silent in.
    #[serde(default)]
    pub quiet_hours: Option<QuietHours>,
    #[serde(default)]
    pub autonomy: Autonomy,
    /// Whether he may drive Spotify (play/pause/skip/volume).
    #[serde(default = "yes")]
    pub music_control: bool,
}

impl Default for Habits {
    fn default() -> Self {
        Self {
            nudges: NudgeLevel::Gentle,
            quiet_hours: None,
            autonomy: Autonomy::Roamer,
            music_control: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum NudgeLevel {
    Off,
    Rare,
    #[default]
    Gentle,
    Naggy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuietHours {
    /// Local hour 0..=23 the quiet window opens and closes. Wraps past midnight
    /// (e.g. 23 → 8), which is the common case.
    pub start_hour: u8,
    pub end_hour: u8,
}

impl QuietHours {
    /// Is `hour` (0..=23) inside the quiet window?
    pub fn contains(&self, hour: u8) -> bool {
        if self.start_hour <= self.end_hour {
            hour >= self.start_hour && hour < self.end_hour
        } else {
            // Wraps midnight.
            hour >= self.start_hour || hour < self.end_hour
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Autonomy {
    /// Stays where you put him.
    Homebody,
    /// Walks and teleports between your windows.
    #[default]
    Roamer,
    /// Roams more, further, more often.
    Free,
}

// --- voice & ears ------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Voice {
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default)]
    pub style: VoiceStyle,
    /// "Answers to his name" — always-on local wake word.
    #[serde(default = "yes")]
    pub wake_word: bool,
    /// Device *names*, not indices (indices reshuffle on replug).
    #[serde(default)]
    pub mic: Option<String>,
    #[serde(default)]
    pub speaker: Option<String>,
}

impl Default for Voice {
    fn default() -> Self {
        Self {
            enabled: true,
            style: VoiceStyle::default(),
            wake_word: true,
            mic: None,
            speaker: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum VoiceStyle {
    #[default]
    LowEasy,
    Bright,
    Gravel,
}

// --- optional hookups --------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Connect {
    #[serde(default)]
    pub spotify: bool,
    /// Lets him sit on your windows and notice when you've been at one thing too
    /// long. Titles only, never content.
    #[serde(default = "yes")]
    pub window_awareness: bool,
}

fn yes() -> bool {
    true
}

// --- persistence -------------------------------------------------------------

pub fn settings_path(app: &tauri::AppHandle) -> PathBuf {
    use tauri::Manager;
    app.path()
        .app_data_dir()
        .unwrap_or_else(|_| std::env::temp_dir().join("deskfolk"))
        .join("settings.json")
}

pub fn load(app: &tauri::AppHandle) -> Settings {
    let path = settings_path(app);
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(app: &tauri::AppHandle, settings: &Settings) {
    let path = settings_path(app);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    match serde_json::to_string_pretty(settings) {
        Ok(json) => {
            if let Err(e) = std::fs::write(&path, json) {
                tracing::warn!("could not save settings: {e}");
            }
        }
        Err(e) => tracing::warn!("could not serialise settings: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_design() {
        let s = Settings::default();
        assert!(!s.first_run_complete, "a fresh install has not run the wizard");
        assert_eq!(s.character, "yasser");
        assert_eq!(s.brain.tier, Tier::Basic, "basic = free local brain by default");
        assert!(s.brain.provider.is_none(), "no provider means the bundled local one");
        assert_eq!(s.personality.warmth, 70);
        assert_eq!(s.personality.humor, 80);
        assert!(!s.personality.adult_register, "clean by default; opt in");
        assert_eq!(s.personality.mirror, Mirror::Strong);
        assert_eq!(s.habits.autonomy, Autonomy::Roamer);
        assert!(s.voice.enabled && s.voice.wake_word);
    }

    #[test]
    fn survives_a_round_trip() {
        let mut s = Settings::default();
        s.first_run_complete = true;
        s.personality.edge = 90;
        s.habits.quiet_hours = Some(QuietHours { start_hour: 23, end_hour: 8 });
        s.brain.tier = Tier::Advanced;
        s.brain.provider = Some(ProviderConfig::OpenAiCompat {
            base_url: "https://openrouter.ai/api/v1".into(),
            api_key: "sk-test".into(),
            model: "some/model".into(),
        });
        let json = serde_json::to_string(&s).unwrap();
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert!(back.first_run_complete);
        assert_eq!(back.personality.edge, 90);
        assert_eq!(back.habits.quiet_hours, Some(QuietHours { start_hour: 23, end_hour: 8 }));
        assert_eq!(back.brain.tier, Tier::Advanced);
    }

    #[test]
    fn a_partial_file_still_loads() {
        // An older or hand-edited file with only a couple of fields must not
        // wipe the rest — every field has a default.
        let json = r#"{ "character": "frog", "personality": { "warmth": 10, "humor": 10,
            "edge": 10, "chattiness": 10, "curiosity": 10, "guardedness": 10 } }"#;
        let s: Settings = serde_json::from_str(json).unwrap();
        assert_eq!(s.character, "frog");
        assert_eq!(s.personality.warmth, 10);
        assert_eq!(s.personality.mirror, Mirror::Strong, "missing sub-field defaults");
        assert_eq!(s.habits.autonomy, Autonomy::Roamer, "missing section defaults");
        assert!(!s.first_run_complete);
    }

    #[test]
    fn quiet_hours_wrap_past_midnight() {
        let q = QuietHours { start_hour: 23, end_hour: 8 };
        assert!(q.contains(23) && q.contains(0) && q.contains(7));
        assert!(!q.contains(8) && !q.contains(12) && !q.contains(22));
        let day = QuietHours { start_hour: 9, end_hour: 17 };
        assert!(day.contains(12) && !day.contains(8) && !day.contains(20));
    }

    #[test]
    fn mirror_scales_drift() {
        assert!(Mirror::Subtle.factor() < Mirror::Medium.factor());
        assert!(Mirror::Strong.factor() > Mirror::Medium.factor());
    }
}
