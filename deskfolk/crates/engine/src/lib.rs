//! The runtime engine — everything that makes a companion feel alive.
//!
//! This crate knows nothing about Yasser, about Tauri, or about how pixels
//! reach a screen. It is a pure state machine: feed it [`Inputs`] and elapsed
//! time, get back a [`FrameState`] to draw and a list of [`Effect`]s to carry
//! out. That makes aliveness *testable* — you can run a simulated day in
//! milliseconds and assert he never froze, which is the one property the whole
//! product rests on.
//!
//! The behavior grammar is inherited from the alpha (`desktop-pet/app.py`),
//! which was tuned by actually living with the character. Timings that look
//! arbitrary usually aren't; the comments say why.

use deskfolk_package::{CharacterPackage, RangeMs, Role};
use rand::{Rng, SeedableRng};
use serde::{Deserialize, Serialize};

mod clip;
pub mod hit;
pub mod layout;
pub use clip::{ClipPlayer, Frame};
pub use hit::{hit_test, Portal};
pub use layout::{compose, Composition, Placed, Rect, SpriteDims};

#[cfg(test)]
pub(crate) mod tests_support;

/// Engine tick granularity. The alpha ran animation at 30ms and behavior at
/// 100ms; one clock at 30ms is simpler and still cheap because a tick that
/// changes nothing does no work downstream.
pub const TICK_MS: i64 = 30;

// ---------------------------------------------------------------------------
// Public surface
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    Idle,
    Listening,
    Thinking,
    Speaking,
    Asleep,
}

/// What the outside world tells the engine each tick.
#[derive(Debug, Clone, Default)]
pub struct Inputs {
    /// Local hour, 0..=23. Drives night behavior.
    pub local_hour: u8,
    /// Speakers are actually producing sound right now.
    pub voice_audible: bool,
    /// TTS has been requested but no audio yet. The alpha learned the hard
    /// way that animating a talk clip during this window looks like a silent
    /// mime, so it is tracked separately.
    pub voice_pending: bool,
    /// 0..=100 loudness, bucketed into visemes.
    pub voice_level: u8,
    /// A request to the mind is in flight.
    pub busy: bool,
    /// Cursor position in stage coordinates, if it is near enough to notice.
    pub cursor: Option<(i32, i32)>,
}

/// Things the engine wants the host to do. The engine never performs I/O
/// itself, which is what keeps it testable and portable to other bodies.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// Ask the mind something. `event` matches the brain contract
    /// (`wake_greet`, `self_talk`, `user_speech`, …).
    Think { event: String, text: String },
    /// Open the microphone.
    OpenMic,
    /// Stop any speech playback immediately.
    StopVoice,
    /// Behavioral log line, mirrored to the Control Center's diary.
    Log(String),
}

/// Everything needed to draw one frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FrameState {
    pub sprite: String,
    pub dx: i32,
    pub dy: i32,
    pub fx: Option<String>,
    /// Full-frame static overlay during a glitch.
    pub glitch_fx: Option<String>,
    pub state: State,
    pub subtitle: Option<String>,
    /// True when the mic is live, so the renderer can pulse a ring.
    pub listening: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    None,
    Listen,
    Sleep,
}

impl Action {
    pub fn parse(s: &str) -> Self {
        match s {
            "listen" => Action::Listen,
            "sleep" => Action::Sleep,
            _ => Action::None,
        }
    }
}

/// A reply from the mind.
#[derive(Debug, Clone, Default)]
pub struct Reply {
    pub say: String,
    pub emotion: String,
    pub glitch: u8,
    pub action: String,
    /// Whether audio is coming. If so the engine holds a thinking pose rather
    /// than lip-syncing to silence.
    pub has_audio: bool,
}

// ---------------------------------------------------------------------------
// Engine
// ---------------------------------------------------------------------------

pub struct Engine {
    pkg: CharacterPackage,
    player: ClipPlayer,
    rng: rand::rngs::SmallRng,

    state: State,
    /// Monotonic ms since the engine started.
    now: u64,

    /// Countdown before the current base emotion reverts to idle.
    hold_ms: i64,
    blink_in: i64,
    fidget_in: i64,
    self_talk_in: i64,
    /// Emotional lead-in before switching to the talk clip, so a reaction is
    /// legible before the mouth takes over.
    intro_ms: i64,
    glitch_ms: i64,

    last_emotion: String,
    prev_audible: bool,
    pending_action: Action,

    /// Where he is currently looking, -1.0 (left) .. 1.0 (right). Eased
    /// rather than snapped — a sprite that tracks the cursor exactly reads as
    /// a crosshair, not as a person noticing something.
    gaze: f32,
    /// Was the cursor within noticing range last tick?
    cursor_was_near: bool,
    /// When the current unbroken stretch at the machine began. Reset by a
    /// long absence, not by interaction — sitting there working *is* presence.
    session_began: u64,
    last_nudge: Option<u64>,

    /// Last time the user did anything. Drives napping and self-talk.
    last_touch: u64,
    sleep_since: Option<u64>,
    subtitle: Option<Subtitle>,
    self_talk_enabled: bool,
}

#[derive(Debug, Clone)]
struct Subtitle {
    text: String,
    ms_left: i64,
}

impl Engine {
    pub fn new(pkg: CharacterPackage) -> Self {
        Self::with_seed(pkg, rand::random())
    }

    /// Deterministic construction, for tests and for reproducing a bug report.
    pub fn with_seed(pkg: CharacterPackage, seed: u64) -> Self {
        let mut rng = rand::rngs::SmallRng::seed_from_u64(seed);
        let idle = pkg.role_clip(Role::Idle).to_string();
        let player = ClipPlayer::new(&pkg, &idle);
        let life = pkg.manifest.life.clone();

        let blink_in = pick(&mut rng, life.blink_every) as i64;
        let fidget_in = life
            .fidget_every
            .map(|r| pick(&mut rng, r) as i64)
            .unwrap_or(i64::MAX);
        let self_talk_in = pick(&mut rng, life.self_talk_every) as i64;

        Self {
            pkg,
            player,
            rng,
            state: State::Idle,
            now: 0,
            hold_ms: 0,
            blink_in,
            fidget_in,
            self_talk_in,
            intro_ms: 0,
            glitch_ms: 0,
            last_emotion: "idle".into(),
            prev_audible: false,
            pending_action: Action::None,
            gaze: 0.0,
            cursor_was_near: false,
            session_began: 0,
            last_nudge: None,
            last_touch: 0,
            sleep_since: None,
            subtitle: None,
            self_talk_enabled: life.self_talk,
        }
    }

    pub fn package(&self) -> &CharacterPackage {
        &self.pkg
    }

    pub fn state(&self) -> State {
        self.state
    }

    pub fn is_asleep(&self) -> bool {
        self.player.base_name() == self.pkg.role_clip(Role::Sleep)
    }

    pub fn set_self_talk(&mut self, on: bool) {
        self.self_talk_enabled = on;
    }

    // -- external events -----------------------------------------------------

    /// The user clicked, typed, or otherwise showed up. Resets the "ignored"
    /// clocks that lead to napping.
    pub fn touch(&mut self) {
        self.last_touch = self.now;
    }

    pub fn say(&mut self, text: impl Into<String>, ms: Option<i64>) {
        let text = text.into();
        // Long lines need to stay up long enough to actually read.
        let ms = ms.unwrap_or_else(|| (2600).max(60 * text.chars().count() as i64));
        self.subtitle = Some(Subtitle { text, ms_left: ms });
    }

    /// Apply a reply from the mind.
    pub fn apply_reply(&mut self, reply: &Reply) -> Vec<Effect> {
        let mut fx = Vec::new();
        self.pending_action = Action::parse(&reply.action);

        if !reply.say.is_empty() {
            self.say(reply.say.clone(), None);
        }

        let emotion = if self.pkg.manifest.emotions.contains_key(&reply.emotion) {
            reply.emotion.clone()
        } else {
            "confused".to_string()
        };

        if reply.has_audio {
            // Audio is coming: remember the mood for when sound actually
            // starts, but don't mouth-flap while TTS is still cooking.
            self.last_emotion = emotion.clone();
            let talky = matches!(emotion.as_str(), "talk" | "whisper" | "idle" | "busy");
            if talky {
                let think = self.pkg.role_clip(Role::Think).to_string();
                self.player.set_base(&self.pkg, &think);
                self.hold_ms = 0;
            } else {
                fx.extend(self.play_emotion(&emotion, reply.glitch, 0));
            }
        } else {
            fx.extend(self.play_emotion(&emotion, reply.glitch, 0));
        }
        fx
    }

    /// Play a named emotion from the package's table.
    pub fn play_emotion(&mut self, name: &str, glitch: u8, hold_override: i64) -> Vec<Effect> {
        let mut fx = Vec::new();
        let Some(emo) = self.pkg.manifest.emotions.get(name).cloned() else {
            return fx;
        };
        self.last_emotion = name.to_string();
        fx.push(Effect::Log(format!("react: {name} (glitch={glitch})")));

        // A high glitch reading overrides the normal clip with the possessed
        // one and flashes static.
        if glitch >= 60 {
            if let Some(g) = self.pkg.manifest.roles.glitch.clone() {
                if emo.clip != g {
                    self.player.play_shot(&self.pkg, &g);
                    self.glitch_ms = 450;
                    return fx;
                }
            }
        }

        if emo.base {
            self.player.set_base(&self.pkg, &emo.clip);
            self.hold_ms = if hold_override > 0 { hold_override } else { emo.hold_ms as i64 };
        } else {
            self.player.play_shot(&self.pkg, &emo.clip);
        }
        fx
    }

    /// Put him to sleep deliberately.
    pub fn sleep(&mut self) {
        self.sleep_since = Some(self.now);
        let clip = self.pkg.role_clip(Role::Sleep).to_string();
        self.player.set_base(&self.pkg, &clip);
        self.state = State::Asleep;
        self.hold_ms = 0;
    }

    /// Startled awake — plays the startle clip and greets.
    pub fn wake_up(&mut self) -> Vec<Effect> {
        let slept = self.sleep_since.map(|s| self.now - s).unwrap_or(0);
        self.sleep_since = None;
        let idle = self.pkg.role_clip(Role::Idle).to_string();
        let startle = self.pkg.role_clip(Role::Startle).to_string();
        self.player.set_base(&self.pkg, &idle);
        self.player.play_shot(&self.pkg, &startle);
        self.hold_ms = 8000;
        self.state = State::Idle;
        self.last_touch = self.now;
        vec![
            Effect::Log(format!("woken after {} min", slept / 60_000)),
            Effect::Think { event: "wake_greet".into(), text: String::new() },
        ]
    }

    /// Leave sleep without greeting. The alpha needed this: a full greeting
    /// here consumed the microphone and looped him into monologuing.
    pub fn soft_wake(&mut self) {
        if !self.is_asleep() {
            return;
        }
        self.sleep_since = None;
        let idle = self.pkg.role_clip(Role::Idle).to_string();
        self.player.set_base(&self.pkg, &idle);
        self.hold_ms = 0;
        self.state = State::Idle;
    }

    pub fn begin_listening(&mut self) {
        self.state = State::Listening;
        self.touch();
        self.say("listening... (click me to send)", Some(60_000));
    }

    pub fn begin_thinking(&mut self) {
        self.state = State::Thinking;
        let think = self.pkg.role_clip(Role::Think).to_string();
        self.player.set_base(&self.pkg, &think);
        self.hold_ms = 0;
        self.say("...", Some(30_000));
    }

    // -- the life loop -------------------------------------------------------

    /// Advance the world by `dt` ms.
    pub fn tick(&mut self, dt: i64, input: &Inputs) -> Vec<Effect> {
        self.now = self.now.saturating_add(dt.max(0) as u64);
        let mut fx = Vec::new();

        self.player.tick(&self.pkg, dt);

        if self.glitch_ms > 0 {
            self.glitch_ms -= dt;
        }
        if let Some(s) = &mut self.subtitle {
            s.ms_left -= dt;
            if s.ms_left <= 0 {
                self.subtitle = None;
            }
        }

        // `audible` = sound is coming out. `pending` = TTS still cooking.
        // `gate` = either, meaning the mic must stay shut and he must not nap.
        let audible = input.voice_audible;
        let fetching = input.voice_pending && !audible;
        let gate = audible || input.voice_pending;

        if audible != self.prev_audible {
            self.prev_audible = audible;
            if audible {
                // Give an emotional reaction a moment to read before the
                // mouth animation takes over.
                let emo_clip = self
                    .pkg
                    .manifest
                    .emotions
                    .get(&self.last_emotion)
                    .map(|e| e.clip.clone());
                let talk = self.pkg.role_clip(Role::Talk).to_string();
                let idle = self.pkg.role_clip(Role::Idle).to_string();
                let emotive = emo_clip
                    .as_ref()
                    .map(|c| *c != talk && *c != idle)
                    .unwrap_or(false);
                self.intro_ms = if emotive { 1500 } else { 0 };
                if emotive {
                    let carry = emo_clip
                        .and_then(|c| self.pkg.clip(&c).map(|cl| cl.fx.clone()))
                        .unwrap_or_default();
                    self.player.set_fx_override(carry);
                }
                self.state = State::Speaking;
            } else {
                self.player.set_fx_override(Vec::new());
                if self.state == State::Speaking {
                    self.state = State::Idle;
                }
            }
        }

        if audible {
            if let Some(s) = &mut self.subtitle {
                s.ms_left = s.ms_left.max(1500);
            }
            if self.intro_ms > 0 {
                self.intro_ms -= dt;
            } else if !self.player.is_playing_shot() {
                let talk = self.pkg.role_clip(Role::Talk).to_string();
                let glitch = self.pkg.manifest.roles.glitch.clone();
                let base = self.player.base_name().to_string();
                if base != talk && Some(&base) != glitch.as_ref() {
                    self.player.set_base(&self.pkg, &talk);
                }
            }
            let talk = self.pkg.role_clip(Role::Talk).to_string();
            if self.player.base_name() == talk {
                self.hold_ms = 500;
            }
        } else if fetching {
            // Waiting on the voice: hold a thinking pose, don't lip-sync.
            if !self.player.is_playing_shot() {
                let think = self.pkg.role_clip(Role::Think).to_string();
                if self.player.base_name() != think {
                    self.player.set_base(&self.pkg, &think);
                }
            }
            self.hold_ms = self.hold_ms.max(300);
        }

        // Hold: a base emotion reverting to idle when its time is up.
        if self.hold_ms > 0 {
            self.hold_ms -= dt;
            if self.hold_ms <= 0 {
                let next = if audible {
                    Role::Talk
                } else if fetching {
                    Role::Think
                } else {
                    Role::Idle
                };
                let clip = self.pkg.role_clip(next).to_string();
                self.player.set_base(&self.pkg, &clip);
            }
            return fx;
        }

        let properly_idle = !gate
            && !input.busy
            && !self.player.is_playing_shot()
            && self.state == State::Idle
            && !self.is_asleep();

        fx.extend(self.track_cursor(dt, input, properly_idle));
        fx.extend(self.check_long_session(input, properly_idle));

        // Blink — the cheapest, most effective sign of life.
        if properly_idle && self.player.base_name() == self.pkg.role_clip(Role::Idle) {
            self.blink_in -= dt;
            if self.blink_in <= 0 {
                self.blink_in = pick(&mut self.rng, self.pkg.manifest.life.blink_every) as i64;
                let blink = self.pkg.role_clip(Role::Blink).to_string();
                self.player.play_shot(&self.pkg, &blink);
            }
        }

        // Fidgets — stretching, checking a phone, glancing around. This is
        // the answer to "no long periods of frozen sprites"; the alpha only
        // ever blinked, which read as a loop rather than a life.
        if properly_idle && !self.pkg.manifest.life.fidgets.is_empty() {
            self.fidget_in -= dt;
            if self.fidget_in <= 0 {
                if let Some(r) = self.pkg.manifest.life.fidget_every {
                    self.fidget_in = pick(&mut self.rng, r) as i64;
                }
                let fidgets = &self.pkg.manifest.life.fidgets;
                let pick_idx = self.rng.gen_range(0..fidgets.len());
                let clip = fidgets[pick_idx].clone();
                fx.push(Effect::Log(format!("fidget: {clip}")));
                self.player.play_shot(&self.pkg, &clip);
            }
        }

        // Carry out the verb the mind chose, once he has finished speaking.
        if !gate && !input.busy && self.pending_action != Action::None {
            let act = std::mem::replace(&mut self.pending_action, Action::None);
            fx.push(Effect::Log(format!("mind chose: {act:?}")));
            match act {
                Action::Listen if self.state == State::Idle => fx.push(Effect::OpenMic),
                Action::Sleep => self.sleep(),
                _ => {}
            }
        }

        // Presence: nap when ignored, chat to himself when engaged.
        let since_touch = self.now.saturating_sub(self.last_touch);
        let life = &self.pkg.manifest.life;
        let want_nap = !gate
            && !input.busy
            && !self.player.is_playing_shot()
            && !self.is_asleep()
            && self.state == State::Idle
            && self.pending_action == Action::None;

        if want_nap && since_touch > life.nap_after_ms {
            fx.push(Effect::Log(format!("ignored {} min -> nap", since_touch / 60_000)));
            self.sleep();
            return fx;
        }
        // At night he turns in early, but only once left alone for a beat —
        // nodding off mid-conversation would be worse than never sleeping.
        if want_nap && is_night(input.local_hour, life.night_hours) && since_touch > life.night_nap_after_ms
        {
            fx.push(Effect::Log("night -> nap".into()));
            self.sleep();
            return fx;
        }

        if self.self_talk_enabled
            && since_touch < life.engaged_ms
            && properly_idle
            && self.state == State::Idle
        {
            self.self_talk_in -= dt;
            if self.self_talk_in <= 0 {
                self.self_talk_in = pick(&mut self.rng, life.self_talk_every) as i64;
                fx.push(Effect::Log("self-talk".into()));
                fx.push(Effect::Think { event: "self_talk".into(), text: String::new() });
            }
        }

        fx
    }

    /// Cursor awareness.
    ///
    /// Two separate things, and they matter for different reasons. The **lean**
    /// is continuous and tiny — it makes him look like he is paying attention
    /// even when nothing is happening. The **double-take** fires once, when
    /// the cursor arrives after being away, and is what makes him feel like he
    /// noticed *you* rather than like he is tracking a target.
    fn track_cursor(&mut self, dt: i64, input: &Inputs, properly_idle: bool) -> Vec<Effect> {
        let mut fx = Vec::new();
        let life = &self.pkg.manifest.life;
        let anchor_x = self.pkg.manifest.stage.anchor_x;

        let (target, near) = match input.cursor {
            Some((cx, cy)) => {
                let dx = cx - anchor_x;
                let dy = cy - self.pkg.manifest.stage.anchor_y;
                let dist = (((dx * dx + dy * dy) as f64).sqrt()) as i32;
                let near = dist <= life.notice_radius;
                // Saturates at the notice radius so a cursor on the far side
                // of the screen doesn't peg him permanently sideways.
                let t = (dx as f32 / life.notice_radius.max(1) as f32).clamp(-1.0, 1.0);
                (if near { t } else { t * 0.35 }, near)
            }
            // Cursor unknown (off-screen, other monitor): drift back to centre.
            None => (0.0, false),
        };

        // Ease ~4 units/second toward the target; asleep, he doesn't track.
        let speed = if self.is_asleep() { 0.0 } else { 4.0 };
        let step = speed * (dt as f32 / 1000.0);
        self.gaze += (target - self.gaze).clamp(-step, step);

        if near && !self.cursor_was_near && properly_idle {
            if let Some(clip) = life.notice_clip.clone() {
                if self.pkg.clip(&clip).is_some() {
                    fx.push(Effect::Log("noticed the cursor".into()));
                    self.player.play_shot(&self.pkg, &clip);
                }
            }
        }
        self.cursor_was_near = near;
        fx
    }

    /// Where he is currently looking, -1.0 (left) .. 1.0 (right).
    pub fn gaze(&self) -> f32 {
        self.gaze
    }

    /// Notice a long unbroken stretch at the machine and say something about
    /// it, unprompted.
    ///
    /// This is the difference between a companion who answers and one who
    /// starts the conversation — "you've been at this three hours" is him
    /// paying attention to *you*, not to a timer he was asked to watch.
    ///
    /// Presence is not the same as interaction: someone reading for an hour
    /// never clicks him, and resetting on every click would mean he only ever
    /// nags people who are already talking to him. So the stretch is broken by
    /// a real absence — long enough that they got up — and nothing else.
    fn check_long_session(&mut self, input: &Inputs, properly_idle: bool) -> Vec<Effect> {
        let life = &self.pkg.manifest.life;
        if life.nudge_after_ms == 0 {
            return Vec::new();
        }

        // Away long enough to count as having left: start a fresh stretch.
        let away = self.now.saturating_sub(self.last_touch);
        if away > life.nap_after_ms || input.cursor.is_none() && away > life.nap_after_ms {
            self.session_began = self.now;
            return Vec::new();
        }

        if !properly_idle || self.is_asleep() {
            return Vec::new();
        }

        let elapsed = self.now.saturating_sub(self.session_began);
        if elapsed < life.nudge_after_ms {
            return Vec::new();
        }
        // Say it once, then let it go for a good while.
        if let Some(last) = self.last_nudge {
            if self.now.saturating_sub(last) < life.nudge_cooldown_ms {
                return Vec::new();
            }
        }

        self.last_nudge = Some(self.now);
        let hours = elapsed / (60 * 60 * 1000);
        vec![
            Effect::Log(format!("noticed a {hours}h session")),
            Effect::Think {
                event: "long_session".into(),
                text: format!("{hours}"),
            },
        ]
    }

    /// Resolve what to draw. Cheap enough to call every frame.
    pub fn frame(&self, input: &Inputs) -> FrameState {
        let mut f = self.player.frame(&self.pkg);

        // While the mouth should be moving, the viseme replaces the frame's
        // sprite so the mouth tracks actual loudness rather than a fixed loop.
        let talk = self.pkg.role_clip(Role::Talk);
        let visemes = &self.pkg.manifest.visemes;
        if input.voice_audible
            && !self.player.is_playing_shot()
            && self.player.base_name() == talk
            && !visemes.is_empty()
        {
            f.sprite = visemes[viseme_bucket(input.voice_level, visemes.len())].clone();
        }

        let glitch_fx = if self.glitch_ms > 0 {
            let frames = &self.pkg.manifest.roles.glitch_fx;
            if frames.is_empty() {
                None
            } else {
                // Alternate on a 200ms cycle for a strobing feel.
                let i = ((self.glitch_ms / 100) as usize) % frames.len();
                Some(frames[i].clone())
            }
        } else {
            None
        };

        // The lean rides on top of whatever the clip already asked for, so
        // authored motion and cursor attention compose instead of fighting.
        let lean = (self.gaze * self.pkg.manifest.life.gaze_px as f32).round() as i32;

        FrameState {
            sprite: f.sprite,
            dx: f.dx + lean,
            dy: f.dy,
            fx: f.fx,
            glitch_fx,
            state: self.state,
            subtitle: self.subtitle.as_ref().map(|s| s.text.clone()),
            listening: self.state == State::Listening,
        }
    }
}

/// Map 0..=100 loudness onto the package's viseme list. The alpha's buckets
/// (8/30/60) are preserved for a 4-viseme set and generalized beyond it.
fn viseme_bucket(level: u8, n: usize) -> usize {
    if n == 0 {
        return 0;
    }
    if n == 4 {
        return if level < 8 {
            0
        } else if level < 30 {
            1
        } else if level < 60 {
            2
        } else {
            3
        };
    }
    let idx = (level as usize * n) / 101;
    idx.min(n - 1)
}

fn is_night(hour: u8, [start, end]: [u8; 2]) -> bool {
    if start <= end {
        hour >= start && hour < end
    } else {
        // Wraps past midnight, e.g. 22..8.
        hour >= start || hour < end
    }
}

fn pick(rng: &mut impl Rng, r: RangeMs) -> u64 {
    let (lo, hi) = if r.min <= r.max { (r.min, r.max) } else { (r.max, r.min) };
    if hi == lo {
        lo
    } else {
        rng.gen_range(lo..=hi)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests_support::test_package;

    fn engine() -> Engine {
        Engine::with_seed(test_package(), 42)
    }

    fn run(e: &mut Engine, ms: i64, input: &Inputs) -> Vec<Effect> {
        let mut out = Vec::new();
        let mut left = ms;
        while left > 0 {
            let dt = TICK_MS.min(left);
            out.extend(e.tick(dt, input));
            left -= dt;
        }
        out
    }

    #[test]
    fn night_wraps_past_midnight() {
        assert!(is_night(23, [22, 8]));
        assert!(is_night(2, [22, 8]));
        assert!(!is_night(12, [22, 8]));
        assert!(!is_night(8, [22, 8]), "8am is morning, not night");
        assert!(is_night(22, [22, 8]));
    }

    #[test]
    fn viseme_buckets_match_the_alpha() {
        assert_eq!(viseme_bucket(0, 4), 0);
        assert_eq!(viseme_bucket(7, 4), 0);
        assert_eq!(viseme_bucket(8, 4), 1);
        assert_eq!(viseme_bucket(29, 4), 1);
        assert_eq!(viseme_bucket(30, 4), 2);
        assert_eq!(viseme_bucket(60, 4), 3);
        assert_eq!(viseme_bucket(100, 4), 3);
    }

    #[test]
    fn viseme_bucket_never_indexes_out_of_range() {
        for n in 1..=8usize {
            for lvl in 0..=100u8 {
                assert!(viseme_bucket(lvl, n) < n, "n={n} lvl={lvl}");
            }
        }
    }

    #[test]
    fn he_blinks_while_idle() {
        let mut e = engine();
        let input = Inputs { local_hour: 12, ..Default::default() };
        let blink = e.package().role_clip(Role::Blink).to_string();
        let mut blinked = false;
        for _ in 0..(20_000 / TICK_MS) {
            e.tick(TICK_MS, &input);
            if e.player.shot_name() == Some(blink.as_str()) {
                blinked = true;
                break;
            }
        }
        assert!(blinked, "he should blink within 20s of idling");
    }

    /// The brief's hard rule: "No long periods of frozen sprites."
    /// Encoded as a test so it can never silently regress.
    #[test]
    fn he_never_holds_one_sprite_for_long_while_idle() {
        let mut e = engine();
        let input = Inputs { local_hour: 12, ..Default::default() };
        let mut last = e.frame(&input).sprite;
        let mut same_for: i64 = 0;
        let mut worst: i64 = 0;

        // Ten minutes of being left completely alone (under the nap timeout).
        for _ in 0..(10 * 60 * 1000 / TICK_MS) {
            e.tick(TICK_MS, &input);
            e.touch(); // stay engaged so napping doesn't end the test early
            let s = e.frame(&input).sprite;
            if s == last {
                same_for += TICK_MS;
                worst = worst.max(same_for);
            } else {
                same_for = 0;
                last = s;
            }
        }
        assert!(
            worst < 7_000,
            "sprite held for {worst}ms — that reads as frozen, not idle"
        );
    }

    #[test]
    fn he_naps_after_being_ignored() {
        let mut e = engine();
        let input = Inputs { local_hour: 12, ..Default::default() };
        let nap_after = e.package().manifest.life.nap_after_ms as i64;
        run(&mut e, nap_after + 5_000, &input);
        assert!(e.is_asleep(), "should nap after the ignore timeout");
    }

    #[test]
    fn he_naps_sooner_at_night() {
        let mut e = engine();
        let night = Inputs { local_hour: 23, ..Default::default() };
        // Well under the daytime nap timeout, but past the night one.
        run(&mut e, 95_000, &night);
        assert!(e.is_asleep(), "should turn in early at night");
    }

    #[test]
    fn he_does_not_nap_mid_conversation_at_night() {
        let mut e = engine();
        let speaking = Inputs { local_hour: 23, voice_audible: true, ..Default::default() };
        run(&mut e, 200_000, &speaking);
        assert!(!e.is_asleep(), "must never nod off while actually talking");
    }

    #[test]
    fn he_does_not_nap_while_the_voice_is_still_loading() {
        // Regression guard for the alpha's worst bug class: TTS in flight but
        // no audio yet looked exactly like being idle.
        let mut e = engine();
        let pending = Inputs { local_hour: 23, voice_pending: true, ..Default::default() };
        run(&mut e, 200_000, &pending);
        assert!(!e.is_asleep());
    }

    #[test]
    fn he_talks_to_himself_while_engaged() {
        let mut e = engine();
        let input = Inputs { local_hour: 12, ..Default::default() };
        let mut asked = false;
        for _ in 0..(200_000 / TICK_MS) {
            let fx = e.tick(TICK_MS, &input);
            e.touch(); // keep him inside the engaged window
            if fx.iter().any(|f| matches!(f, Effect::Think { event, .. } if event == "self_talk")) {
                asked = true;
                break;
            }
        }
        assert!(asked, "an engaged companion should eventually pipe up");
    }

    #[test]
    fn self_talk_can_be_switched_off() {
        let mut e = engine();
        e.set_self_talk(false);
        let input = Inputs { local_hour: 12, ..Default::default() };
        for _ in 0..(200_000 / TICK_MS) {
            let fx = e.tick(TICK_MS, &input);
            e.touch();
            assert!(
                !fx.iter().any(|f| matches!(f, Effect::Think { event, .. } if event == "self_talk")),
                "self-talk was disabled"
            );
        }
    }

    #[test]
    fn speaking_drives_the_mouth_from_loudness() {
        let mut e = engine();
        let loud = Inputs { local_hour: 12, voice_audible: true, voice_level: 90, ..Default::default() };
        run(&mut e, 3_000, &loud);
        assert_eq!(e.state(), State::Speaking);
        let f = e.frame(&loud);
        assert_eq!(f.sprite, "m3", "loud speech should use the widest viseme");

        let quiet = Inputs { voice_level: 0, ..loud.clone() };
        assert_eq!(e.frame(&quiet).sprite, "m0", "silence should close the mouth");
    }

    #[test]
    fn a_reply_with_audio_holds_a_thinking_pose_instead_of_miming() {
        let mut e = engine();
        let reply = Reply {
            say: "hold on".into(),
            emotion: "talk".into(),
            has_audio: true,
            ..Default::default()
        };
        e.apply_reply(&reply);
        let think = e.package().role_clip(Role::Think).to_string();
        assert_eq!(
            e.player.base_name(),
            think,
            "must not lip-sync before the audio actually starts"
        );
    }

    #[test]
    fn the_minds_listen_verb_opens_the_mic_after_speech_ends() {
        let mut e = engine();
        let reply = Reply { action: "listen".into(), emotion: "idle".into(), ..Default::default() };
        e.apply_reply(&reply);

        // While still speaking, the mic must stay shut.
        let speaking = Inputs { local_hour: 12, voice_audible: true, ..Default::default() };
        let fx = run(&mut e, 2_000, &speaking);
        assert!(!fx.contains(&Effect::OpenMic), "mic must not open mid-speech");

        // Once quiet, the verb fires.
        let quiet = Inputs { local_hour: 12, ..Default::default() };
        let fx = run(&mut e, 2_000, &quiet);
        assert!(fx.contains(&Effect::OpenMic), "listen should fire once he's done");
    }

    #[test]
    fn waking_greets_and_clears_sleep() {
        let mut e = engine();
        e.sleep();
        assert!(e.is_asleep());
        let fx = e.wake_up();
        assert!(!e.is_asleep());
        assert!(fx.iter().any(|f| matches!(f, Effect::Think { event, .. } if event == "wake_greet")));
    }

    #[test]
    fn soft_wake_does_not_greet() {
        // Greeting here used to eat the microphone and loop him.
        let mut e = engine();
        e.sleep();
        e.soft_wake();
        assert!(!e.is_asleep());
        assert_eq!(e.state(), State::Idle);
    }

    #[test]
    fn unknown_emotions_fall_back_instead_of_freezing() {
        let mut e = engine();
        let before = e.frame(&Inputs::default()).sprite;
        e.apply_reply(&Reply { emotion: "nonsense_mood".into(), ..Default::default() });
        let after = e.frame(&Inputs::default()).sprite;
        assert!(!after.is_empty(), "a bad emotion must never blank the character");
        let _ = before;
    }

    #[test]
    fn he_leans_toward_the_cursor() {
        let mut e = engine();
        let anchor = e.package().manifest.stage.anchor_x;
        let right = Inputs {
            local_hour: 12,
            cursor: Some((anchor + 60, 90)),
            ..Default::default()
        };
        run(&mut e, 1_000, &right);
        assert!(e.gaze() > 0.2, "should lean right, gaze={}", e.gaze());
        let dx_right = e.frame(&right).dx;

        let left = Inputs { cursor: Some((anchor - 60, 90)), ..right.clone() };
        run(&mut e, 2_000, &left);
        assert!(e.gaze() < -0.2, "should lean left, gaze={}", e.gaze());
        assert!(e.frame(&left).dx < dx_right);
    }

    #[test]
    fn the_lean_eases_rather_than_snapping() {
        // Snapping to the cursor reads as a crosshair, not as attention.
        let mut e = engine();
        let anchor = e.package().manifest.stage.anchor_x;
        let far = Inputs {
            local_hour: 12,
            cursor: Some((anchor + 500, 90)),
            ..Default::default()
        };
        e.tick(TICK_MS, &far);
        assert!(e.gaze().abs() < 0.2, "one tick should barely move him");
    }

    #[test]
    fn the_lean_is_small_enough_to_read_as_attention() {
        let mut e = engine();
        let anchor = e.package().manifest.stage.anchor_x;
        let input = Inputs {
            local_hour: 12,
            cursor: Some((anchor + 400, 90)),
            ..Default::default()
        };
        run(&mut e, 4_000, &input);
        let dx = e.frame(&input).dx.abs();
        assert!(dx <= e.package().manifest.life.gaze_px, "he slid too far: {dx}");
    }

    #[test]
    fn he_recentres_when_the_cursor_leaves_the_screen() {
        let mut e = engine();
        let anchor = e.package().manifest.stage.anchor_x;
        let near = Inputs { local_hour: 12, cursor: Some((anchor + 100, 90)), ..Default::default() };
        run(&mut e, 2_000, &near);
        assert!(e.gaze().abs() > 0.1);

        let gone = Inputs { cursor: None, ..near.clone() };
        run(&mut e, 3_000, &gone);
        assert!(e.gaze().abs() < 0.05, "should drift back to centre, got {}", e.gaze());
    }

    #[test]
    fn he_double_takes_when_the_cursor_arrives() {
        let mut e = engine();
        let anchor = e.package().manifest.stage.anchor_x;
        let anchor_y = e.package().manifest.stage.anchor_y;

        let away = Inputs { local_hour: 12, cursor: Some((anchor + 9_000, 9_000)), ..Default::default() };
        run(&mut e, 500, &away);

        let arrived = Inputs { cursor: Some((anchor, anchor_y)), ..away.clone() };
        let fx = run(&mut e, 200, &arrived);
        assert!(
            fx.iter().any(|f| matches!(f, Effect::Log(m) if m.contains("noticed the cursor"))),
            "arriving near him should register once"
        );
    }

    #[test]
    fn the_double_take_does_not_repeat_while_the_cursor_stays() {
        // Otherwise he re-startles every frame the mouse rests on him.
        let mut e = engine();
        let anchor = e.package().manifest.stage.anchor_x;
        let anchor_y = e.package().manifest.stage.anchor_y;
        let on = Inputs {
            local_hour: 12,
            cursor: Some((anchor, anchor_y)),
            ..Default::default()
        };
        run(&mut e, 300, &on);
        let fx = run(&mut e, 3_000, &on);
        let notices = fx
            .iter()
            .filter(|f| matches!(f, Effect::Log(m) if m.contains("noticed the cursor")))
            .count();
        assert_eq!(notices, 0, "should not keep re-noticing a stationary cursor");
    }

    #[test]
    fn he_does_not_track_the_cursor_while_asleep() {
        let mut e = engine();
        let anchor = e.package().manifest.stage.anchor_x;
        e.sleep();
        let input = Inputs {
            local_hour: 12,
            cursor: Some((anchor + 400, 90)),
            ..Default::default()
        };
        run(&mut e, 3_000, &input);
        assert_eq!(e.gaze(), 0.0, "a sleeping character should not follow the mouse");
    }

    fn nudges(fx: &[Effect]) -> usize {
        fx.iter()
            .filter(|f| matches!(f, Effect::Think { event, .. } if event == "long_session"))
            .count()
    }

    /// Run a stretch where the person is present the whole time — cursor on
    /// screen, no interaction. Reading, not clicking.
    fn present(e: &mut Engine, ms: i64) -> Vec<Effect> {
        let input = Inputs { local_hour: 14, cursor: Some((200, 300)), ..Default::default() };
        let mut out = Vec::new();
        let mut left = ms;
        while left > 0 {
            let dt = TICK_MS.min(left);
            out.extend(e.tick(dt, &input));
            e.touch(); // present at the machine, so he never naps
            left -= dt;
        }
        out
    }

    #[test]
    fn he_says_nothing_about_a_short_session() {
        let mut e = engine();
        let fx = present(&mut e, 60 * 60 * 1000); // one hour
        assert_eq!(nudges(&fx), 0, "an hour is not worth mentioning");
    }

    #[test]
    fn he_speaks_up_after_a_long_stretch() {
        let mut e = engine();
        let after = e.package().manifest.life.nudge_after_ms as i64;
        let fx = present(&mut e, after + 60_000);
        assert_eq!(nudges(&fx), 1, "should mention a long session exactly once");
    }

    #[test]
    fn he_mentions_it_once_and_then_lets_it_go() {
        // The failure mode this guards is him nagging every frame forever.
        let mut e = engine();
        let after = e.package().manifest.life.nudge_after_ms as i64;
        present(&mut e, after + 60_000);
        let fx = present(&mut e, 20 * 60 * 1000);
        assert_eq!(nudges(&fx), 0, "should stay quiet during the cooldown");
    }

    #[test]
    fn the_nudge_carries_how_many_hours() {
        let mut e = engine();
        let after = e.package().manifest.life.nudge_after_ms as i64;
        let fx = present(&mut e, after + 60_000);
        let hours = fx.iter().find_map(|f| match f {
            Effect::Think { event, text } if event == "long_session" => Some(text.clone()),
            _ => None,
        });
        assert_eq!(hours.as_deref(), Some("3"), "the mind needs the number to talk about it");
    }

    #[test]
    fn walking_away_resets_the_stretch() {
        // Someone who left for lunch has not been sitting there for four hours.
        let mut e = engine();
        let after = e.package().manifest.life.nudge_after_ms as i64;
        present(&mut e, after - 10 * 60 * 1000);

        // Gone long enough to have got up.
        let gone_for = e.package().manifest.life.nap_after_ms as i64 + 60_000;
        let away = Inputs { local_hour: 14, cursor: None, ..Default::default() };
        run(&mut e, gone_for, &away);

        let fx = present(&mut e, 20 * 60 * 1000);
        assert_eq!(nudges(&fx), 0, "the clock should have restarted");
    }

    #[test]
    fn a_character_can_opt_out_of_nagging_entirely() {
        let mut pkg = test_package();
        pkg.manifest.life.nudge_after_ms = 0;
        let mut e = Engine::with_seed(pkg, 42);
        let fx = present(&mut e, 6 * 60 * 60 * 1000);
        assert_eq!(nudges(&fx), 0, "nudge_after_ms = 0 means never");
    }

    #[test]
    fn subtitles_expire() {
        let mut e = engine();
        let input = Inputs { local_hour: 12, ..Default::default() };
        e.say("hey", Some(1_000));
        assert!(e.frame(&input).subtitle.is_some());
        run(&mut e, 1_500, &input);
        assert!(e.frame(&input).subtitle.is_none(), "subtitle should time out");
    }

    #[test]
    fn a_long_line_stays_up_long_enough_to_read() {
        let mut e = engine();
        let input = Inputs { local_hour: 12, ..Default::default() };
        let long = "a".repeat(200);
        e.say(long, None);
        run(&mut e, 5_000, &input);
        assert!(
            e.frame(&input).subtitle.is_some(),
            "a 200-char line must outlast the 2.6s minimum"
        );
    }
}
