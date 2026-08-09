//! Connecting the engine's "say something" to an actual model.
//!
//! The engine only ever emits [`Effect::Think`]; it has no idea a network
//! exists. This module is the seam: it assembles a request from the character
//! package, runs it off the UI thread, and feeds the reply back in.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use deskfolk_ai::{Provider, ProviderConfig, Role, ThinkRequest, ThinkReply, Turn};
use deskfolk_engine::Reply;
use parking_lot::Mutex;

use crate::journal;
use crate::music;
use crate::runtime::Runtime;
use crate::voice::Voice;

/// How much conversation to carry. Long enough that he remembers the thread,
/// short enough that a companion left running for a week doesn't grow an
/// unbounded prompt.
const HISTORY_TURNS: usize = 16;

pub struct Mind {
    provider: Box<dyn Provider>,
    history: Mutex<Vec<Turn>>,
    /// One request in flight at a time. Without this an idle loop that fires
    /// self-talk every tick would stack requests forever.
    busy: AtomicBool,
}

impl Mind {
    pub fn new(config: &ProviderConfig) -> Self {
        let provider = config.build();
        tracing::info!("mind provider: {}", provider.name());
        Self {
            provider,
            history: Mutex::new(Vec::new()),
            busy: AtomicBool::new(false),
        }
    }

    pub fn is_busy(&self) -> bool {
        self.busy.load(Ordering::SeqCst)
    }

    pub fn remember(&self, role: Role, text: &str) {
        if text.trim().is_empty() {
            return;
        }
        let mut h = self.history.lock();
        h.push(Turn { role, text: text.to_string() });
        let len = h.len();
        if len > HISTORY_TURNS {
            h.drain(0..len - HISTORY_TURNS);
        }
    }

    pub fn history(&self) -> Vec<Turn> {
        self.history.lock().clone()
    }
}

/// Should this reply be synthesised, or is a subtitle enough?
///
/// He mutters to himself on an idle timer, and until the voice was wired that
/// cost nothing. Now every mutter is a synthesis request: in one session 25 of
/// 43 of them were self-talk, which is what emptied the Orpheus quota — so by
/// the time someone actually spoke to him, the reply came back 429 and silent.
/// Ambient thinking-out-loud stays on screen; things said *to* him get a voice.
///
/// `DESKFOLK_VOICE_SELF_TALK=1` gives him his voice back for those, for anyone
/// running a TTS backend without a quota to spend.
fn worth_speaking(event: &str) -> bool {
    if event != "self_talk" {
        return true;
    }
    matches!(
        std::env::var("DESKFOLK_VOICE_SELF_TALK").as_deref(),
        Ok("1" | "true" | "on")
    )
}

/// Do whatever the reply asked of the music, if anything.
///
/// Separate from the emotion and the action because it is not mutually
/// exclusive with either: he can skip a track *and* answer you in the same
/// breath, which is what a person in the room would do.
pub fn obey_music(wish: Option<&deskfolk_ai::MusicWish>) {
    let Some(w) = wish else { return };
    let Some(parsed) = music::Wish::parse(&w.r#do, &w.query) else {
        if !w.r#do.trim().is_empty() {
            tracing::debug!("music: ignoring invented verb {:?}", w.r#do);
        }
        return;
    };
    if music::grant(&parsed) {
        journal::did(parsed.describe());
    } else {
        journal::trouble(format!("{} — needs the Spotify Web API", parsed.describe()));
    }
}

/// Record a spoken exchange, so what he was told out loud is part of the same
/// thread as everything he was told in text.
pub fn remember_exchange(mind: &Mind, user: &str, companion: &str) {
    mind.remember(Role::User, user);
    mind.remember(Role::Companion, companion);
}

/// Build a request from the live character package plus current context.
///
/// Everything character-specific — the brief, the emotion vocabulary — comes
/// from the package, so this function never mentions any particular companion.
pub fn build_request(rt: &Runtime, mind: &Mind, event: &str, text: &str) -> ThinkRequest {
    let pkg = rt.engine.package();
    let mut senses = std::collections::BTreeMap::new();
    senses.insert("hour".to_string(), rt.inputs.local_hour.to_string());
    senses.insert("screen".to_string(), "desktop-pc".to_string());

    ThinkRequest {
        system: pkg.manifest.personality.prompt.clone(),
        history: mind.history(),
        event: event.to_string(),
        text: text.to_string(),
        // Only moods this character actually has clips for.
        emotions: pkg.manifest.emotions.keys().cloned().collect(),
        senses,
    }
}

/// Ask the mind, off the UI thread, and apply whatever comes back.
///
/// Failures are deliberately in-character: a companion that pops up a network
/// error stops being a companion. The package supplies its own offline lines.
pub fn ask(
    rt: Arc<Mutex<Runtime>>,
    mind: Arc<Mind>,
    voice: Arc<Voice>,
    nudge: Arc<Mutex<Option<crate::stroll::Facing>>>,
    event: String,
    text: String,
) {
    if mind.busy.swap(true, Ordering::SeqCst) {
        tracing::debug!("mind busy, dropping '{event}'");
        return;
    }

    let request = {
        let mut guard = rt.lock();
        guard.inputs.busy = true;
        build_request(&guard, &mind, &event, &text)
    };
    if !text.trim().is_empty() {
        mind.remember(Role::User, &text);
    }

    tauri::async_runtime::spawn(async move {
        let asked_at = std::time::Instant::now();
        let outcome = mind.provider.think(&request).await;
        let took = asked_at.elapsed();
        mind.busy.store(false, Ordering::SeqCst);

        match outcome {
            Ok(reply) => {
                tracing::info!("mind said: {:?} ({})", reply.say, reply.emotion);
                mind.remember(Role::Companion, &reply.say);
                // Ask for the voice *before* taking the runtime lock: `speak`
                // marks the voice pending synchronously, and `apply_reply`
                // needs that already true to decide whether he holds a
                // thinking pose or starts mouthing straight away.
                let has_audio = if worth_speaking(&event) {
                    voice.speak(&reply.say)
                } else {
                    false
                };
                obey_music(reply.music.as_ref());
                journal::said(journal::Said {
                    event: event.clone(),
                    heard: text.clone(),
                    say: reply.say.clone(),
                    emotion: reply.emotion.clone(),
                    glitch: reply.glitch,
                    action: reply.action.clone(),
                    spoken: has_audio,
                    took,
                });
                // A walk he decided on goes to the wander loop, the same
                // channel the menu and a spoken order use. The engine still
                // gets the reply for the pose and for listen/sleep.
                if crate::agent::route_walk(&reply.action, &nudge) {
                    journal::did(format!("set off walking ({})", reply.action));
                }
                let mut guard = rt.lock();
                guard.inputs.busy = false;
                guard.inputs.voice_pending = has_audio;
                apply(&mut guard, &reply, has_audio);
            }
            Err(e) => {
                tracing::warn!("mind unreachable: {e}");
                journal::trouble(format!("mind unreachable on '{event}': {e}"));
                let mut guard = rt.lock();
                guard.inputs.busy = false;
                let line = offline_line(&guard);
                // An offline line is still something he says out loud — the
                // voice comes from the brain, which may be up even when the
                // model is not.
                let has_audio = voice.speak(&line);
                guard.inputs.voice_pending = has_audio;
                guard.engine.say(line, None);
                let _ = guard.engine.play_emotion("confused", 0, 0);
            }
        }
    });
}

fn apply(rt: &mut Runtime, reply: &ThinkReply, has_audio: bool) {
    let _ = rt.engine.apply_reply(&Reply {
        say: reply.say.clone(),
        emotion: reply.emotion.clone(),
        glitch: reply.glitch,
        action: reply.action.clone(),
        has_audio,
    });
}

/// Pick one of the character's own offline lines, so a dead network still
/// sounds like the character rather than like software.
fn offline_line(rt: &Runtime) -> String {
    let lines = &rt.engine.package().manifest.personality.offline_lines;
    if lines.is_empty() {
        return "...".to_string();
    }
    // Cheap rotation off the clock — no RNG needed and never the same twice
    // in a row for typical line counts.
    let idx = (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0) as usize)
        % lines.len();
    lines[idx].clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mind() -> Mind {
        Mind::new(&ProviderConfig::Sidecar { base_url: "http://127.0.0.1:1".into() })
    }

    #[test]
    fn history_is_bounded() {
        let m = mind();
        for i in 0..(HISTORY_TURNS * 3) {
            m.remember(Role::User, &format!("line {i}"));
        }
        assert_eq!(m.history().len(), HISTORY_TURNS, "prompt must not grow forever");
    }

    #[test]
    fn history_keeps_the_most_recent_turns() {
        let m = mind();
        for i in 0..(HISTORY_TURNS + 5) {
            m.remember(Role::User, &format!("line {i}"));
        }
        let h = m.history();
        assert_eq!(h.last().unwrap().text, format!("line {}", HISTORY_TURNS + 4));
        assert!(!h.iter().any(|t| t.text == "line 0"), "oldest turns should be dropped");
    }

    #[test]
    fn blank_turns_are_not_remembered() {
        // Silent replies would otherwise pad the prompt with empty turns.
        let m = mind();
        m.remember(Role::Companion, "   ");
        m.remember(Role::User, "");
        assert!(m.history().is_empty());
    }

    #[test]
    fn idle_muttering_does_not_burn_the_voice_quota() {
        // 25 of 43 synthesis requests in one session were self-talk, which is
        // what left nothing for the reply when someone actually spoke to him.
        assert!(!worth_speaking("self_talk"));
    }

    #[test]
    fn anything_said_to_him_still_gets_a_voice() {
        for event in ["user_speech", "user_poke", "wake_greet", "talk_button"] {
            assert!(worth_speaking(event), "{event} should be spoken aloud");
        }
    }

    #[test]
    fn busy_flag_admits_one_request_at_a_time() {
        let m = mind();
        assert!(!m.busy.swap(true, Ordering::SeqCst), "first caller gets through");
        assert!(m.busy.swap(true, Ordering::SeqCst), "second caller is turned away");
        assert!(m.is_busy());
    }
}
