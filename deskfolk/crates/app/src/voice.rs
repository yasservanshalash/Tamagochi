//! Giving him a voice.
//!
//! `audio.rs` has had a working `Player` for a while and the menu has
//! remembered your speakers for just as long — but nothing ever connected the
//! two to the brain, so every reply arrived as a silent speech bubble. This is
//! that connection.
//!
//! The brain owns synthesis, as it always has: Orpheus, the sentence chunking,
//! the RPM throttle and the "never silently switch voices" rule are all tuned
//! in `think_server.py` and are not worth reimplementing in Rust. What was
//! missing was a *door* — `/pet/tts_live` can only replay a token minted inside
//! `/pet/think`, and Deskfolk runs its own LLM, so it could never mint one.
//! `POST /pet/speak` is that door: text in, streamed PCM out.
//!
//! Everything here runs on one dedicated thread. `cpal`'s stream is happiest
//! owned by a single thread, the HTTP read is blocking by design (it is how
//! the first sentence starts playing while the second is still being
//! synthesised), and keeping it off the async runtime means a slow TTS chunk
//! can never stall the mind or the window.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use crate::audio::{AudioSettings, Player};

/// Give up on a silent stream rather than gating him forever. If the brain
/// accepts the request and then never produces audio, he must still be allowed
/// to nap and to open his mic again.
const STREAM_TIMEOUT: Duration = Duration::from_secs(90);

/// How long to wait for the *first* audio before abandoning the line.
///
/// Separate from the overall timeout, and shorter, because a late voice is
/// worse than no voice. The brain holds the connection open while it retries a
/// rate-limited synthesiser — 93 seconds, measured — and the engine has long
/// since stopped waiting and moved on. Audio arriving then makes him start
/// talking out of nowhere over a subtitle that has already gone.
///
/// Deliberately under the engine's patience, so the engine learns the voice is
/// not coming from this rather than from its own deadline.
const FIRST_AUDIO_TIMEOUT: Duration = Duration::from_secs(12);

/// How long to keep the output device open after he stops talking. Reopening
/// per sentence adds an audible gap on some Windows stacks; holding it forever
/// keeps a device busy that the user may want elsewhere.
const DEVICE_IDLE: Duration = Duration::from_secs(30);

enum Cmd {
    Speak(String),
    Stop,
}

#[derive(Default)]
struct VoiceState {
    /// Sound is coming out of the speakers right now.
    audible: AtomicBool,
    /// Asked for, not yet arrived. The engine treats this as "do not nap and
    /// do not open the mic" without letting him mouth-flap at silence.
    pending: AtomicBool,
    level: AtomicU8,
}

pub struct Voice {
    tx: Option<Sender<Cmd>>,
    state: Arc<VoiceState>,
}

impl Voice {
    /// Start the voice thread. `base` is the brain's URL; `None` disables
    /// speech entirely and every method becomes a no-op.
    pub fn new(base: Option<String>, settings: Arc<Mutex<AudioSettings>>) -> Self {
        let state = Arc::new(VoiceState::default());
        let Some(base) = base else {
            tracing::info!("voice: disabled");
            return Self { tx: None, state };
        };

        let (tx, rx) = mpsc::channel();
        let thread_state = state.clone();
        let base_for_thread = base.clone();
        match std::thread::Builder::new()
            .name("deskfolk-voice".into())
            .spawn(move || run(rx, thread_state, settings, base_for_thread))
        {
            Ok(_) => {
                tracing::info!("voice: speaking through {base}/pet/speak");
                Self { tx: Some(tx), state }
            }
            Err(e) => {
                tracing::warn!("voice: could not start the audio thread: {e}");
                Self { tx: None, state }
            }
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.tx.is_some()
    }

    /// Speak a line, interrupting whatever he was saying.
    ///
    /// Returns whether audio is actually on its way, which the engine needs
    /// *synchronously*: `has_audio` decides whether he holds a thinking pose
    /// waiting for sound or animates the reply straight away, and getting it
    /// wrong is the "silent mime" the alpha spent a long time eliminating.
    pub fn speak(&self, text: &str) -> bool {
        let Some(tx) = &self.tx else { return false };
        let text = text.trim();
        if text.is_empty() {
            return false;
        }
        // Set before sending: the caller applies the reply to the engine on
        // this thread, and it must already see the gate closed.
        self.state.pending.store(true, Ordering::SeqCst);
        if tx.send(Cmd::Speak(text.to_string())).is_err() {
            self.state.pending.store(false, Ordering::SeqCst);
            return false;
        }
        true
    }

    /// Cut him off mid-sentence.
    pub fn stop(&self) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(Cmd::Stop);
        }
        self.state.pending.store(false, Ordering::SeqCst);
    }

    pub fn audible(&self) -> bool {
        self.state.audible.load(Ordering::Relaxed)
    }

    pub fn pending(&self) -> bool {
        self.state.pending.load(Ordering::Relaxed)
    }

    pub fn level(&self) -> u8 {
        self.state.level.load(Ordering::Relaxed)
    }
}

// ---------------------------------------------------------------------------
// The voice thread
// ---------------------------------------------------------------------------

fn run(
    rx: Receiver<Cmd>,
    state: Arc<VoiceState>,
    settings: Arc<Mutex<AudioSettings>>,
    base: String,
) {
    let client = match reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(4))
        // Generous, because a long reply legitimately streams for a while:
        // the brain synthesises sentence by sentence and holds the connection
        // open in between. This is only a backstop for a socket that stalls
        // outright — `STREAM_TIMEOUT` is the deadline that normally applies.
        .timeout(Duration::from_secs(120))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("voice: no HTTP client ({e}); he stays silent");
            return;
        }
    };

    let mut device: Option<(Option<String>, Player)> = None;
    let mut idle_since: Option<Instant> = None;
    let mut next = rx.recv().ok();

    while let Some(cmd) = next.take() {
        match cmd {
            Cmd::Stop => {
                if let Some((_, p)) = &device {
                    p.stop();
                }
                state.audible.store(false, Ordering::SeqCst);
                state.pending.store(false, Ordering::SeqCst);
                state.level.store(0, Ordering::Relaxed);
                idle_since = Some(Instant::now());
            }
            Cmd::Speak(text) => {
                let want = settings.lock().output.clone();
                // Reopen when the user picks a different device, so a change
                // in the menu takes effect on the very next line he speaks.
                if device.as_ref().map(|(d, _)| d != &want).unwrap_or(true) {
                    device = Player::open(&settings.lock()).map(|p| (want, p));
                }
                match &device {
                    Some((_, player)) => {
                        let interrupted = speak(&client, &base, &text, player, &state, &rx);
                        idle_since = Some(Instant::now());
                        if let Some(cmd) = interrupted {
                            next = Some(cmd);
                            continue;
                        }
                    }
                    None => {
                        tracing::warn!("voice: no usable output device; skipping this line");
                        state.pending.store(false, Ordering::SeqCst);
                    }
                }
            }
        }

        // Wait for the next line, releasing the device if he goes quiet.
        next = loop {
            let wait = match idle_since {
                Some(since) if device.is_some() => {
                    DEVICE_IDLE.saturating_sub(since.elapsed())
                }
                _ => Duration::from_secs(3600),
            };
            match rx.recv_timeout(wait) {
                Ok(cmd) => break Some(cmd),
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if device.take().is_some() {
                        tracing::debug!("voice: released the output device after a quiet spell");
                    }
                    idle_since = None;
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break None,
            }
        };
    }
}

/// Stream one line. Returns the command that interrupted it, if any.
fn speak(
    client: &reqwest::blocking::Client,
    base: &str,
    text: &str,
    player: &Player,
    state: &VoiceState,
    rx: &Receiver<Cmd>,
) -> Option<Cmd> {
    use std::io::Read;

    // Anything still queued belongs to the previous line.
    player.stop();
    state.audible.store(false, Ordering::SeqCst);
    state.pending.store(true, Ordering::SeqCst);

    let started = Instant::now();
    let url = format!("{}/pet/speak", base.trim_end_matches('/'));
    let response = client
        .post(&url)
        .json(&serde_json::json!({ "text": text }))
        .send();

    let mut response = match response {
        Ok(r) if r.status().is_success() => r,
        Ok(r) => {
            // A brain without a TTS backend answers 503. That is a
            // configuration fact, not a crash — he simply stays text-only.
            tracing::warn!("voice: {} said {} for {:?}", url, r.status(), truncate(text));
            state.pending.store(false, Ordering::SeqCst);
            return None;
        }
        Err(e) => {
            tracing::warn!("voice: {url} unreachable ({e}); staying silent");
            state.pending.store(false, Ordering::SeqCst);
            return None;
        }
    };

    let mut feeder = PcmFeeder::default();
    let mut buf = [0u8; 8192];
    let mut fed = 0usize;
    let interrupted = loop {
        match rx.try_recv() {
            Ok(cmd) => break Some(cmd),
            Err(TryRecvError::Disconnected) => break Some(Cmd::Stop),
            Err(TryRecvError::Empty) => {}
        }
        if fed == 0 && started.elapsed() > FIRST_AUDIO_TIMEOUT {
            tracing::warn!(
                "voice: no audio after {}s — abandoning so he doesn't start \
                 talking after he has given up",
                FIRST_AUDIO_TIMEOUT.as_secs()
            );
            break None;
        }
        if started.elapsed() > STREAM_TIMEOUT {
            tracing::warn!("voice: gave up after {}s of streaming", STREAM_TIMEOUT.as_secs());
            break None;
        }
        match response.read(&mut buf) {
            Ok(0) => break None,
            Ok(n) => {
                let pcm = feeder.push(&buf[..n]);
                if !pcm.is_empty() {
                    player.feed_pcm16(&pcm);
                    fed += pcm.len();
                }
                publish(player, state);
            }
            Err(e) => {
                tracing::warn!("voice: stream ended early: {e}");
                break None;
            }
        }
    };

    if let Some(cmd) = interrupted {
        player.stop();
        state.audible.store(false, Ordering::SeqCst);
        state.pending.store(false, Ordering::SeqCst);
        state.level.store(0, Ordering::Relaxed);
        return Some(cmd);
    }

    if fed == 0 {
        // Synthesis failed on the brain's side; it streams a header and
        // nothing else. Clearing `pending` here is what lets him nap again.
        tracing::warn!("voice: no audio came back for {:?}", truncate(text));
        state.pending.store(false, Ordering::SeqCst);
        return None;
    }
    tracing::info!(
        "voice: {} KiB ({:.1}s) for {:?}",
        fed / 1024,
        fed as f64 / 32_000.0,
        truncate(text)
    );

    // Let the buffer drain — the stream is done, the speakers are not.
    while player.is_playing() {
        match rx.try_recv() {
            Ok(cmd) => {
                player.stop();
                state.audible.store(false, Ordering::SeqCst);
                state.pending.store(false, Ordering::SeqCst);
                state.level.store(0, Ordering::Relaxed);
                return Some(cmd);
            }
            Err(TryRecvError::Disconnected) => break,
            Err(TryRecvError::Empty) => {}
        }
        publish(player, state);
        std::thread::sleep(Duration::from_millis(16));
    }

    state.audible.store(false, Ordering::SeqCst);
    state.pending.store(false, Ordering::SeqCst);
    state.level.store(0, Ordering::Relaxed);
    None
}

fn publish(player: &Player, state: &VoiceState) {
    let playing = player.is_playing();
    if playing {
        // The first sample reaching the device is the moment he starts
        // talking: `pending` closes and the talk clip takes over.
        state.pending.store(false, Ordering::SeqCst);
    }
    state.audible.store(playing, Ordering::SeqCst);
    state.level.store(player.level(), Ordering::Relaxed);
}

fn truncate(s: &str) -> String {
    let s = s.trim();
    if s.chars().count() <= 48 {
        return s.to_string();
    }
    s.chars().take(45).chain("...".chars()).collect()
}

// ---------------------------------------------------------------------------
// Byte plumbing
// ---------------------------------------------------------------------------

/// Turns a chunked WAV-shaped HTTP body into sample-aligned PCM16.
///
/// Two things bite here, and both are silent failures rather than errors:
///
/// * The brain prefixes a 44-byte WAV header with placeholder sizes. Fed to
///   the player as samples it is a short burst of noise.
/// * A network chunk can split a 16-bit sample down the middle. `feed_pcm16`
///   drops an odd trailing byte, so every sample after the split would be
///   assembled from the wrong pair of bytes — the whole rest of the sentence
///   becomes static.
#[derive(Default)]
struct PcmFeeder {
    header_done: bool,
    prelude: Vec<u8>,
    carry: Option<u8>,
}

impl PcmFeeder {
    const HEADER: usize = 44;

    fn push(&mut self, chunk: &[u8]) -> Vec<u8> {
        let mut data: Vec<u8> = Vec::with_capacity(chunk.len() + 1);

        if !self.header_done {
            self.prelude.extend_from_slice(chunk);
            let looks_like_wav = self.prelude.len() < 4 || self.prelude.starts_with(b"RIFF");
            if !looks_like_wav {
                // Raw PCM: take it all.
                self.header_done = true;
                data.append(&mut self.prelude);
            } else if self.prelude.len() >= Self::HEADER {
                self.header_done = true;
                data.extend_from_slice(&self.prelude[Self::HEADER..]);
                self.prelude.clear();
            } else {
                return Vec::new();
            }
        } else {
            data.extend_from_slice(chunk);
        }

        if let Some(b) = self.carry.take() {
            data.insert(0, b);
        }
        if data.len() % 2 == 1 {
            self.carry = data.pop();
        }
        data
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav_header() -> Vec<u8> {
        let mut h = Vec::new();
        h.extend_from_slice(b"RIFF");
        h.extend_from_slice(&[0; 4]);
        h.extend_from_slice(b"WAVE");
        h.extend_from_slice(b"fmt ");
        h.extend_from_slice(&16u32.to_le_bytes());
        h.extend_from_slice(&[0; 16]);
        h.extend_from_slice(b"data");
        h.extend_from_slice(&[0; 4]);
        assert_eq!(h.len(), 44);
        h
    }

    #[test]
    fn the_wav_header_never_reaches_the_speakers() {
        let mut f = PcmFeeder::default();
        let mut body = wav_header();
        body.extend_from_slice(&[1, 2, 3, 4]);
        assert_eq!(f.push(&body), vec![1, 2, 3, 4]);
    }

    #[test]
    fn a_header_split_across_chunks_is_still_stripped() {
        // 8 KiB reads mean the header usually arrives whole, but a slow first
        // flush can split it — and a half-stripped header is pure noise.
        let mut f = PcmFeeder::default();
        let body = wav_header();
        assert!(f.push(&body[..20]).is_empty(), "nothing until the header is complete");
        let mut rest = body[20..].to_vec();
        rest.extend_from_slice(&[9, 9]);
        assert_eq!(f.push(&rest), vec![9, 9]);
    }

    #[test]
    fn a_sample_split_across_chunks_keeps_its_alignment() {
        // The regression this exists for: drop the odd byte instead of
        // carrying it and every later sample is built from the wrong pair.
        let mut f = PcmFeeder::default();
        let mut body = wav_header();
        body.push(0xAA);
        assert!(f.push(&body).is_empty(), "a lone byte is not a sample yet");
        assert_eq!(f.push(&[0xBB, 0xCC]), vec![0xAA, 0xBB]);
        assert_eq!(f.push(&[0xDD]), vec![0xCC, 0xDD]);
    }

    #[test]
    fn raw_pcm_without_a_header_is_passed_straight_through() {
        // Kokoro and Piper paths can produce bare PCM; discarding 44 bytes of
        // it would clip the first 22 samples off every sentence.
        let mut f = PcmFeeder::default();
        let pcm: Vec<u8> = (0..64).collect();
        assert_eq!(f.push(&pcm), pcm);
    }

    #[test]
    fn an_empty_stream_yields_nothing() {
        let mut f = PcmFeeder::default();
        assert!(f.push(&[]).is_empty());
        assert!(f.push(&wav_header()).is_empty());
    }

    #[test]
    fn everything_after_the_header_survives_across_many_chunks() {
        let mut f = PcmFeeder::default();
        let mut out = Vec::new();
        out.extend(f.push(&wav_header()));
        for i in 0..100u8 {
            out.extend(f.push(&[i]));
        }
        // One byte may still be held back as the carry; the rest is in order.
        assert!(out.len() >= 99, "got {}", out.len());
        assert_eq!(out[0], 0);
        assert_eq!(out[98], 98);
    }

    #[test]
    fn a_disabled_voice_never_claims_audio_is_coming() {
        // `has_audio` drives the engine's pose. A voice that cannot speak must
        // report false, or he freezes in a thinking clip waiting for sound.
        let v = Voice::new(None, Arc::new(Mutex::new(AudioSettings::default())));
        assert!(!v.is_enabled());
        assert!(!v.speak("hello"));
        assert!(!v.pending());
    }

    #[test]
    fn blank_lines_are_not_spoken() {
        let v = Voice::new(None, Arc::new(Mutex::new(AudioSettings::default())));
        assert!(!v.speak("   "));
    }

    #[test]
    fn the_voice_gives_up_before_the_engine_stops_waiting() {
        // The engine holds a thinking pose for 15s. If the voice waited
        // longer, audio would arrive after he had already moved on and he
        // would start talking out of nowhere — which is exactly what a
        // rate-limited synthesiser produced: 93 seconds, measured.
        assert!(FIRST_AUDIO_TIMEOUT < Duration::from_secs(15));
        assert!(FIRST_AUDIO_TIMEOUT < STREAM_TIMEOUT);
    }

    #[test]
    fn long_lines_are_truncated_for_the_log_only() {
        let long = "a".repeat(200);
        assert_eq!(truncate(&long).chars().count(), 48);
        assert_eq!(truncate("short"), "short");
    }
}
