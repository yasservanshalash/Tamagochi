//! Listening to him being talked to.
//!
//! `audio.rs` has had a working `Recorder` for a while and the menu has
//! remembered your microphone for just as long, but nothing ever opened it:
//! `Effect::OpenMic` was a log line, so "Talk to him" put up a subtitle that
//! asked you to click to send something that was never recorded. This is the
//! other half of the loop.
//!
//! The interaction is deliberately one step. The alpha's flow — open the mic,
//! say your piece, then find him again and click to send — asks you to do
//! something a person you are talking to would never ask for. Here he decides
//! you have finished the same way a person does: **you stop talking.** A short
//! run of silence after speech ends the turn and sends it.
//!
//! Saying nothing at all is not an error either. If no speech arrives he
//! simply stops listening and treats it as the poke it was.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::Deserialize;

use crate::audio::{AudioSettings, Recorder, SOURCE_RATE};

/// Peak level (0..=100) that counts as somebody speaking rather than room
/// noise. A headset mic idles near zero; a keyboard tap spikes briefly, which
/// is why speech also has to persist before it opens the turn.
const SPEECH_LEVEL: u8 = 9;
/// How long speech must persist before it counts, so a cough or a key press
/// does not start a sentence.
const SPEECH_MS: u64 = 120;
/// Silence after speech that ends the turn. Long enough to think mid-sentence,
/// short enough that he does not sit there after you have finished.
const SILENCE_MS: u64 = 900;
/// If nothing is ever said, give up and treat it as a poke.
const NO_SPEECH_MS: u64 = 3_000;
/// Hard ceiling on one turn, so a stuck-open mic cannot record forever.
const MAX_TURN_MS: u64 = 20_000;

/// What came back from the brain after it heard you.
#[derive(Debug, Clone, Deserialize)]
pub struct Heard {
    #[serde(default)]
    pub say: String,
    #[serde(default)]
    pub emotion: String,
    #[serde(default)]
    pub glitch: u8,
    #[serde(default)]
    pub action: String,
    /// The transcript, so the conversation can be remembered as text.
    #[serde(default)]
    pub heard: String,
}

enum Cmd {
    Listen { hour: u8 },
    Cancel,
}

#[derive(Default)]
struct EarState {
    listening: AtomicBool,
    level: AtomicU8,
}

pub struct Ear {
    tx: Option<Sender<Cmd>>,
    state: Arc<EarState>,
}

impl Ear {
    /// Start the listening thread. `base` is the brain's URL; `None` disables
    /// the microphone entirely and every method becomes a no-op.
    pub fn new(
        base: Option<String>,
        settings: Arc<Mutex<AudioSettings>>,
        on_reply: Arc<dyn Fn(Heard) + Send + Sync>,
        on_idle: Arc<dyn Fn() + Send + Sync>,
    ) -> Self {
        let state = Arc::new(EarState::default());
        let Some(base) = base else {
            tracing::info!("mic: disabled");
            return Self { tx: None, state };
        };

        let (tx, rx) = mpsc::channel();
        let thread_state = state.clone();
        match std::thread::Builder::new()
            .name("deskfolk-ear".into())
            .spawn(move || run(rx, thread_state, settings, base, on_reply, on_idle))
        {
            Ok(_) => {
                tracing::info!("mic: ready");
                Self { tx: Some(tx), state }
            }
            Err(e) => {
                tracing::warn!("mic: could not start the listening thread: {e}");
                Self { tx: None, state }
            }
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.tx.is_some()
    }

    pub fn is_listening(&self) -> bool {
        self.state.listening.load(Ordering::Relaxed)
    }

    /// Loudness of what the mic is hearing, 0..=100 — drives the ring pulse.
    pub fn level(&self) -> u8 {
        self.state.level.load(Ordering::Relaxed)
    }

    /// Open the mic for one turn. Ignored if a turn is already in progress, so
    /// an impatient second click cannot cut the first one short.
    pub fn listen(&self, hour: u8) -> bool {
        let Some(tx) = &self.tx else { return false };
        if self.is_listening() {
            return true;
        }
        self.state.listening.store(true, Ordering::SeqCst);
        if tx.send(Cmd::Listen { hour }).is_err() {
            self.state.listening.store(false, Ordering::SeqCst);
            return false;
        }
        true
    }

    pub fn cancel(&self) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(Cmd::Cancel);
        }
    }
}

fn run(
    rx: Receiver<Cmd>,
    state: Arc<EarState>,
    settings: Arc<Mutex<AudioSettings>>,
    base: String,
    on_reply: Arc<dyn Fn(Heard) + Send + Sync>,
    on_idle: Arc<dyn Fn() + Send + Sync>,
) {
    let client = match reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(4))
        // Transcription plus a local model's reply is not fast.
        .timeout(Duration::from_secs(120))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("mic: no HTTP client ({e}); staying deaf");
            return;
        }
    };

    while let Ok(cmd) = rx.recv() {
        let hour = match cmd {
            Cmd::Cancel => continue,
            Cmd::Listen { hour } => hour,
        };

        let pcm = match record_turn(&settings, &state, &rx) {
            Some(pcm) => pcm,
            None => {
                state.listening.store(false, Ordering::SeqCst);
                state.level.store(0, Ordering::Relaxed);
                on_idle();
                continue;
            }
        };
        state.listening.store(false, Ordering::SeqCst);
        state.level.store(0, Ordering::Relaxed);

        let url = format!(
            "{}/pet/converse?screen=desktop-pc&hour={hour}&battery=100&charging=1",
            base.trim_end_matches('/')
        );
        tracing::info!("mic: sending {:.1}s of speech", secs(pcm.len()));
        match client
            .post(&url)
            .header("Content-Type", "application/octet-stream")
            .body(pcm)
            .send()
        {
            Ok(r) if r.status().is_success() => match r.json::<Heard>() {
                Ok(reply) => {
                    tracing::info!("mic: heard {:?}", reply.heard);
                    on_reply(reply);
                }
                Err(e) => {
                    tracing::warn!("mic: malformed reply: {e}");
                    on_idle();
                }
            },
            Ok(r) => {
                tracing::warn!("mic: {url} said {}", r.status());
                on_idle();
            }
            Err(e) => {
                tracing::warn!("mic: {url} unreachable ({e})");
                on_idle();
            }
        }
    }
}

/// Record until he decides the turn is over. `None` means nothing was said.
fn record_turn(
    settings: &Arc<Mutex<AudioSettings>>,
    state: &Arc<EarState>,
    rx: &Receiver<Cmd>,
) -> Option<Vec<u8>> {
    let Some(rec) = Recorder::open(&settings.lock()) else {
        tracing::warn!("mic: no usable input device");
        return None;
    };

    let mut pcm: Vec<u8> = Vec::new();
    let started = Instant::now();
    let mut speech_since: Option<Instant> = None;
    let mut speaking = false;
    let mut quiet_since: Option<Instant> = None;

    loop {
        match rx.try_recv() {
            Ok(Cmd::Cancel) | Err(TryRecvError::Disconnected) => return None,
            Ok(Cmd::Listen { .. }) | Err(TryRecvError::Empty) => {}
        }

        std::thread::sleep(Duration::from_millis(16));
        pcm.extend_from_slice(&rec.drain());
        let level = rec.level();
        state.level.store(level, Ordering::Relaxed);

        if level >= SPEECH_LEVEL {
            quiet_since = None;
            match speech_since {
                Some(t) if !speaking && t.elapsed() >= Duration::from_millis(SPEECH_MS) => {
                    speaking = true;
                }
                None => speech_since = Some(Instant::now()),
                _ => {}
            }
        } else {
            speech_since = None;
            if speaking {
                let q = *quiet_since.get_or_insert_with(Instant::now);
                if q.elapsed() >= Duration::from_millis(SILENCE_MS) {
                    // He heard you finish. That is the whole interaction.
                    return Some(pcm);
                }
            }
        }

        let waited = started.elapsed();
        if !speaking && waited >= Duration::from_millis(NO_SPEECH_MS) {
            // Worth a log line rather than a debug one: "he opened the mic and
            // heard nothing" is the first thing to check when someone says
            // talking to him does not work.
            tracing::info!("mic: nothing said in {NO_SPEECH_MS}ms; closing, treated as a poke");
            return None;
        }
        if waited >= Duration::from_millis(MAX_TURN_MS) {
            tracing::info!("mic: hit the {MAX_TURN_MS}ms ceiling; sending what there is");
            return if speaking { Some(pcm) } else { None };
        }
    }
}

fn secs(bytes: usize) -> f64 {
    bytes as f64 / (SOURCE_RATE as f64 * 2.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pcm_length_converts_to_seconds() {
        // 16kHz mono PCM16 is 32,000 bytes a second; getting this wrong only
        // shows up as a nonsense number in the log, so pin it.
        assert!((secs(32_000) - 1.0).abs() < 1e-9);
        assert_eq!(secs(0), 0.0);
    }

    #[test]
    fn a_disabled_ear_never_claims_to_listen() {
        let ear = Ear::new(
            None,
            Arc::new(Mutex::new(AudioSettings::default())),
            Arc::new(|_| {}),
            Arc::new(|| {}),
        );
        assert!(!ear.is_enabled());
        assert!(!ear.listen(12));
        assert!(!ear.is_listening());
    }

    #[test]
    fn a_reply_parses_from_the_brains_converse_shape() {
        // The brain adds `heard` to the usual think reply; everything else is
        // optional because a local model's JSON is not always complete.
        let raw = r#"{"say":"yo","emotion":"happy","glitch":0,"action":"none","heard":"hey man"}"#;
        let h: Heard = serde_json::from_str(raw).unwrap();
        assert_eq!(h.say, "yo");
        assert_eq!(h.heard, "hey man");
    }

    #[test]
    fn a_sparse_reply_still_parses() {
        let h: Heard = serde_json::from_str(r#"{"say":"hm"}"#).unwrap();
        assert_eq!(h.say, "hm");
        assert!(h.emotion.is_empty());
        assert_eq!(h.glitch, 0);
    }

    #[test]
    fn the_turn_ends_sooner_than_it_gives_up_waiting() {
        // If silence-after-speech were longer than the no-speech timeout, a
        // pause mid-sentence would be read as "said nothing" and dropped.
        assert!(SILENCE_MS < NO_SPEECH_MS);
        assert!(NO_SPEECH_MS < MAX_TURN_MS);
    }
}
