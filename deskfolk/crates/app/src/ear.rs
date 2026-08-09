//! Listening to him being talked to.
//!
//! Two ways in. **Click him and talk** — he opens his ear and ends the turn
//! when you stop, so there is no second click and no mode to get stuck in.
//! Or, if you turn it on, **say his name** and he starts listening on his own.
//!
//! # Deciding what counts as speech
//!
//! A fixed loudness threshold does not survive contact with real microphones.
//! A headset at low gain peaks around 6 on a 0..100 scale while someone talks
//! normally; a desk mic in a noisy room idles higher than that. Pick one number
//! and you either cut people off mid-sentence or never stop recording.
//!
//! So the threshold is measured, not assumed: the first fraction of a second
//! establishes the room's noise floor and speech is whatever sits clearly
//! above it. Both numbers are logged for every turn, because "he did not hear
//! me" is otherwise impossible to tell apart from "he heard me and had nothing
//! to say".

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::Deserialize;

use crate::audio::{AudioSettings, Recorder, SOURCE_RATE};

/// How long to sample the room before deciding what speech sounds like.
///
/// Every millisecond here is dead time between clicking him and him being able
/// to hear you, so it is as short as it can be while still holding enough
/// samples to be worth a percentile.
const CALIBRATE_MS: u64 = 200;
/// A backstop on waiting for the device to start delivering, so a mic that
/// never produces a sample cannot hold a turn open forever.
const CALIBRATE_CAP_MS: u64 = 1_500;
/// Speech must clear the noise floor by at least this much, on a 0..=100 peak
/// scale — and by a quarter of the floor again in a room that is already loud,
/// since noise that loud fluctuates by more than a fixed margin.
const OVER_FLOOR: u8 = 4;
/// The bar is never below this, so silence itself cannot read as speech.
const MIN_THRESHOLD: u8 = 4;
/// How long speech must persist before it counts, so a key press or a cough
/// does not open a turn.
const SPEECH_MS: u64 = 110;
/// Silence after speech that ends the turn. Long enough to think mid-sentence.
const SILENCE_MS: u64 = 1_000;
/// If nothing is ever said after a click, give up and treat it as a poke.
const NO_SPEECH_MS: u64 = 4_000;
/// Hard ceiling on one turn, so a stuck-open mic cannot record forever.
const MAX_TURN_MS: u64 = 25_000;
/// The same ceiling while merely watching for his name, which is far shorter
/// because nothing good comes of a long one: the name is a word, the audio is
/// posted to the brain to be checked, and — since the listening thread is
/// inside this recording — nothing else can be serviced until it ends.
const WAKE_MAX_TURN_MS: u64 = 6_000;
/// Audio kept before speech is detected, so his name is never clipped off the
/// front of the very utterance that contains it.
const PREROLL_MS: u64 = 700;

/// What came back from the brain after it heard you.
#[derive(Debug, Clone, Default, Deserialize)]
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
    /// Anything he wants done to the music. Spoken turns never pass through
    /// `mind::ask`, so without this they are the one path where "skip this"
    /// would be understood and then ignored.
    #[serde(default)]
    pub music: Option<deskfolk_ai::MusicWish>,
}

#[derive(Debug, Deserialize)]
struct WakeReply {
    #[serde(default)]
    wake: bool,
    #[serde(default)]
    heard: String,
}

enum Cmd {
    Listen { hour: u8 },
    Cancel,
    SetWake(bool),
}

#[derive(Default)]
struct EarState {
    listening: AtomicBool,
    level: AtomicU8,
    cancel: AtomicBool,
    /// Set the moment a deliberate turn is asked for. Name-spotting polls it
    /// and drops what it is doing, because a `Cmd::Listen` sitting in the
    /// channel is invisible from inside a recording — the hotkey would
    /// otherwise do nothing at all until the current watch ended.
    wanted: AtomicBool,
}

/// How one recording is bounded, and whether it defers to a deliberate turn.
#[derive(Clone, Copy)]
struct Turn {
    /// Give up if nothing is ever said within this long.
    give_up_ms: u64,
    /// Stop and send what there is once the turn has run this long.
    max_ms: u64,
    /// Whether to abandon the recording when the user asks to talk.
    yields: bool,
}

/// A turn the user asked for: it already has the mic, and yields to nobody.
const DELIBERATE: Turn = Turn {
    give_up_ms: NO_SPEECH_MS,
    max_ms: MAX_TURN_MS,
    yields: false,
};

/// Waiting to hear his name. Silence is the normal state of a room he is
/// waiting in, so this never gives up on it — but it does step aside.
const WATCHING: Turn = Turn {
    give_up_ms: u64::MAX,
    max_ms: WAKE_MAX_TURN_MS,
    yields: true,
};

pub struct Ear {
    tx: Option<Sender<Cmd>>,
    state: Arc<EarState>,
}

/// Everything the listening thread needs to report back.
pub struct Ears {
    pub on_reply: Arc<dyn Fn(Heard) + Send + Sync>,
    pub on_idle: Arc<dyn Fn() + Send + Sync>,
    /// He heard his name and is about to take a turn.
    pub on_wake: Arc<dyn Fn() + Send + Sync>,
    /// You have finished talking and he has gone away to think about it.
    ///
    /// This matters more than it sounds: transcription plus a local model is
    /// several seconds, and without it he holds the listening pose through all
    /// of them — so he looks like he is still waiting for you to speak when he
    /// is actually working on the answer.
    pub on_thinking: Arc<dyn Fn() + Send + Sync>,
    /// Is his own voice coming out of the speakers right now? Barging in only
    /// means something while it is.
    pub voice_audible: Arc<dyn Fn() -> bool + Send + Sync>,
    /// The user talked over him. Returns true if he yielded the floor — in
    /// which case what follows is speech meant for him, no name required.
    pub on_barge: Arc<dyn Fn() -> bool + Send + Sync>,
}

impl Ear {
    pub fn new(
        base: Option<String>,
        settings: Arc<Mutex<AudioSettings>>,
        ears: Ears,
    ) -> Self {
        let state = Arc::new(EarState::default());
        let Some(base) = base else {
            tracing::info!("mic: disabled");
            return Self { tx: None, state };
        };

        let (tx, rx) = mpsc::channel();
        let thread_state = state.clone();
        let start_wake = settings.lock().wake;
        match std::thread::Builder::new()
            .name("deskfolk-ear".into())
            .spawn(move || run(rx, thread_state, settings, base, ears, start_wake))
        {
            Ok(_) => {
                tracing::info!(
                    "mic: ready{}",
                    if start_wake { ", listening for his name" } else { "" }
                );
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

    /// Open the mic for one turn. Ignored if a turn is already running, so an
    /// impatient second click cannot cut the first one short.
    pub fn listen(&self, hour: u8) -> bool {
        let Some(tx) = &self.tx else { return false };
        if self.is_listening() {
            return true;
        }
        self.state.cancel.store(false, Ordering::SeqCst);
        // Tell name-spotting to let go before queueing the command, so the
        // recording it is inside ends now rather than up to a ceiling later.
        self.state.wanted.store(true, Ordering::SeqCst);
        self.state.listening.store(true, Ordering::SeqCst);
        if tx.send(Cmd::Listen { hour }).is_err() {
            self.state.wanted.store(false, Ordering::SeqCst);
            self.state.listening.store(false, Ordering::SeqCst);
            return false;
        }
        true
    }

    pub fn cancel(&self) {
        // A flag as well as a message: the recording loop is inside a turn and
        // polls this, rather than coming back to the channel between turns.
        self.state.cancel.store(true, Ordering::SeqCst);
        if let Some(tx) = &self.tx {
            let _ = tx.send(Cmd::Cancel);
        }
    }

    /// Turn name-spotting on or off.
    pub fn set_wake(&self, on: bool) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(Cmd::SetWake(on));
        }
    }
}

// ---------------------------------------------------------------------------
// The listening thread
// ---------------------------------------------------------------------------

fn run(
    rx: Receiver<Cmd>,
    state: Arc<EarState>,
    settings: Arc<Mutex<AudioSettings>>,
    base: String,
    ears: Ears,
    start_wake: bool,
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

    let mut watching = start_wake;
    // Held open across watch ticks. Reopening per tick would click the device
    // and, on headsets with sidetone, blip the user's own voice at them.
    let mut watch_rec: Option<Recorder> = None;

    loop {
        match rx.recv_timeout(Duration::from_millis(40)) {
            Ok(Cmd::SetWake(on)) => {
                watching = on;
                if !on {
                    watch_rec = None;
                    state.level.store(0, Ordering::Relaxed);
                }
                tracing::info!("mic: name-spotting {}", if on { "on" } else { "off" });
            }
            Ok(Cmd::Cancel) => {}
            Ok(Cmd::Listen { hour }) => {
                // A deliberate turn gets the device to itself. The request has
                // been picked up, so name-spotting need not keep standing down.
                state.wanted.store(false, Ordering::SeqCst);
                watch_rec = None;
                take_turn(&client, &base, &settings, &state, &ears, hour, None);
            }
            Err(RecvTimeoutError::Disconnected) => return,
            Err(RecvTimeoutError::Timeout) => {
                if watching && !state.listening.load(Ordering::Relaxed) {
                    watch_tick(
                        &client, &base, &settings, &state, &ears, &mut watch_rec,
                    );
                }
            }
        }
    }
}

/// One deliberate turn: record, send, report.
fn take_turn(
    client: &reqwest::blocking::Client,
    base: &str,
    settings: &Arc<Mutex<AudioSettings>>,
    state: &Arc<EarState>,
    ears: &Ears,
    hour: u8,
    preloaded: Option<Vec<u8>>,
) {
    state.listening.store(true, Ordering::SeqCst);
    let pcm = match preloaded {
        Some(p) => Some(p),
        None => match Recorder::open(&settings.lock()) {
            Some(rec) => record_turn(&rec, state, DELIBERATE, None).map(|(pcm, _, _)| pcm),
            None => {
                tracing::warn!("mic: no usable input device");
                None
            }
        },
    };
    state.listening.store(false, Ordering::SeqCst);
    state.level.store(0, Ordering::Relaxed);

    let Some(pcm) = pcm else {
        (ears.on_idle)();
        return;
    };

    // He has your sentence; everything from here is him working on it.
    (ears.on_thinking)();

    match converse(client, base, hour, pcm) {
        Some(reply) => {
            tracing::info!("mic: heard {:?}", reply.heard);
            (ears.on_reply)(reply);
        }
        None => (ears.on_idle)(),
    }
}

/// One pass of name-spotting: wait for an utterance, ask the brain whether it
/// was his name, and take a turn if it was.
fn watch_tick(
    client: &reqwest::blocking::Client,
    base: &str,
    settings: &Arc<Mutex<AudioSettings>>,
    state: &Arc<EarState>,
    ears: &Ears,
    rec: &mut Option<Recorder>,
) {
    if rec.is_none() {
        *rec = Recorder::open(&settings.lock());
        if rec.is_none() {
            tracing::warn!("mic: no input device for name-spotting; turning it off");
            std::thread::sleep(Duration::from_secs(5));
            return;
        }
    }
    let Some(recorder) = rec.as_ref() else { return };

    let Some((pcm, _, barged)) = record_turn(recorder, state, WATCHING, Some(ears)) else {
        return;
    };

    if barged {
        // He yielded mid-sentence to hear this: it is obviously addressed to
        // him, so it skips the name check and goes straight to a turn.
        tracing::info!("mic: barge-in — treating the interruption as a turn");
        take_turn(client, base, settings, state, ears, local_hour(), Some(pcm));
        return;
    }

    let url = format!("{}/pet/wake", base.trim_end_matches('/'));
    let woke = match client
        .post(&url)
        .header("Content-Type", "application/octet-stream")
        .body(pcm.clone())
        .send()
    {
        Ok(r) if r.status().is_success() => r.json::<WakeReply>().ok(),
        Ok(r) => {
            tracing::warn!("mic: {url} said {}", r.status());
            None
        }
        Err(e) => {
            tracing::warn!("mic: {url} unreachable ({e})");
            None
        }
    };

    let Some(w) = woke else { return };
    if !w.wake {
        tracing::debug!("mic: not for him — {:?}", w.heard);
        return;
    }

    tracing::info!("mic: he heard his name in {:?}", w.heard);
    (ears.on_wake)();
    // Send the *same* audio on, so "Yasser, what's the weather" works in one
    // breath rather than making you say his name and then wait for a prompt.
    let hour = local_hour();
    take_turn(client, base, settings, state, ears, hour, Some(pcm));
}

fn converse(
    client: &reqwest::blocking::Client,
    base: &str,
    hour: u8,
    pcm: Vec<u8>,
) -> Option<Heard> {
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
            Ok(reply) => Some(reply),
            Err(e) => {
                tracing::warn!("mic: malformed reply: {e}");
                None
            }
        },
        Ok(r) => {
            tracing::warn!("mic: {url} said {}", r.status());
            None
        }
        Err(e) => {
            tracing::warn!("mic: {url} unreachable ({e})");
            None
        }
    }
}

/// Record one utterance. `None` means nothing was said, or it was cancelled.
///
/// Returns the audio and the peak level reached, which is the number worth
/// having when someone reports that he did not hear them.
fn record_turn(
    rec: &Recorder,
    state: &Arc<EarState>,
    turn: Turn,
    // Present only while name-spotting: lets the recorder notice the user
    // talking over his voice and report the barge-in.
    ears: Option<&Ears>,
) -> Option<(Vec<u8>, u8, bool)> {
    let give_up_ms = turn.give_up_ms;
    let mut pre: VecDeque<Vec<u8>> = VecDeque::new();
    let mut pre_bytes = 0usize;
    let preroll_cap = (SOURCE_RATE as usize * 2 * PREROLL_MS as usize) / 1000;

    let mut pcm: Vec<u8> = Vec::new();
    let started = Instant::now();
    let mut samples: Vec<u8> = Vec::new();
    let mut floor: u8 = 0;
    let mut threshold: Option<u8> = None;
    let mut peak: u8 = 0;
    let mut speech_since: Option<Instant> = None;
    let mut speaking = false;
    let mut quiet_since: Option<Instant> = None;
    // Barge-in bookkeeping: fired at most once per recording pass.
    let mut barge_since: Option<Instant> = None;
    let mut barge_done = false;
    let mut barged = false;

    let mut audio_since: Option<Instant> = None;
    // The quietest the room actually got once we were judging it. If this
    // never falls below the bar, the bar was measured wrong — which is
    // invisible from the peak alone.
    let mut quietest: u8 = u8::MAX;

    loop {
        if state.cancel.swap(false, Ordering::SeqCst) {
            return None;
        }
        // Stand down for a turn the user actually asked for. `wanted` stays
        // set; the command loop clears it when it picks the turn up.
        if turn.yields && state.wanted.load(Ordering::Relaxed) {
            return None;
        }
        std::thread::sleep(Duration::from_millis(16));

        let chunk = rec.drain();
        let level = rec.level();
        state.level.store(level, Ordering::Relaxed);
        peak = peak.max(level);

        // Establish the room before judging anything against it.
        let Some(thresh) = threshold else {
            if !chunk.is_empty() {
                audio_since.get_or_insert_with(Instant::now);
                pre_bytes += chunk.len();
                pre.push_back(chunk);
                while pre_bytes > preroll_cap {
                    if let Some(old) = pre.pop_front() {
                        pre_bytes -= old.len();
                    }
                }
            }
            // Only sample the room once the device is actually delivering.
            // Opening a stream takes longer than the calibration window, so
            // timing this from the loop start measured audio that had not
            // arrived yet: every sample zero, floor 0, bar at the minimum —
            // and then ordinary room tone read as continuous speech, so the
            // turn only ever ended at the ceiling.
            if audio_since.is_some() {
                samples.push(level);
            }
            let settled = audio_since
                .is_some_and(|t| t.elapsed() >= Duration::from_millis(CALIBRATE_MS));
            if settled || started.elapsed() >= Duration::from_millis(CALIBRATE_CAP_MS) {
                floor = quiet_level(&mut samples);
                let t = speech_bar(floor);
                tracing::info!(
                    "mic: noise floor {floor} (of {} samples, loudest {}), speech above {t}",
                    samples.len(),
                    samples.iter().copied().max().unwrap_or(0),
                );
                threshold = Some(t);
            }
            continue;
        };

        quietest = quietest.min(level);

        // Barge-in: sustained sound well above the bar while his own voice is
        // playing. The raised bar plus the sustain requirement keeps speaker
        // echo from tripping it on most setups (headphones are immune).
        if let Some(e) = ears {
            if !barge_done && (e.voice_audible)() {
                let barge_bar = thresh.saturating_add(12).max(30);
                if level >= barge_bar {
                    let since = *barge_since.get_or_insert_with(Instant::now);
                    if since.elapsed() >= Duration::from_millis(350) {
                        barge_done = true;
                        barged = (e.on_barge)();
                    }
                } else {
                    barge_since = None;
                }
            }
        }

        if speaking {
            pcm.extend_from_slice(&chunk);
        } else if !chunk.is_empty() {
            // Not speaking yet: hold recent audio so the first syllable
            // survives, and drop what is older than the pre-roll window.
            pre_bytes += chunk.len();
            pre.push_back(chunk);
            while pre_bytes > preroll_cap {
                if let Some(old) = pre.pop_front() {
                    pre_bytes -= old.len();
                }
            }
        }

        if level >= thresh {
            quiet_since = None;
            match speech_since {
                Some(t) if !speaking && t.elapsed() >= Duration::from_millis(SPEECH_MS) => {
                    speaking = true;
                    for old in pre.drain(..) {
                        pcm.extend_from_slice(&old);
                    }
                    pre_bytes = 0;
                }
                None => speech_since = Some(Instant::now()),
                _ => {}
            }
        } else {
            speech_since = None;
            if speaking {
                let q = *quiet_since.get_or_insert_with(Instant::now);
                if q.elapsed() >= Duration::from_millis(SILENCE_MS) {
                    tracing::info!(
                        "mic: turn ended — {:.1}s, peak {peak}, quietest {quietest} \
                         (floor {floor}, bar {thresh})",
                        secs(pcm.len())
                    );
                    return Some((pcm, peak, barged));
                }
            }
        }

        let waited = started.elapsed();
        if !speaking && waited >= Duration::from_millis(give_up_ms) {
            tracing::info!(
                "mic: nothing above {thresh} in {give_up_ms}ms (peak was {peak}); \
                 closing, treated as a poke"
            );
            return None;
        }
        if waited >= Duration::from_millis(turn.max_ms) {
            tracing::info!(
                "mic: hit the {}ms ceiling; {} (peak {peak}, quietest {quietest}, \
                 floor {floor}, bar {thresh})",
                turn.max_ms,
                if speaking { "sending what there is" } else { "nothing was said" },
            );
            return if speaking { Some((pcm, peak, barged)) } else { None };
        }
    }
}

/// The room's resting level: a low percentile of what was heard, not the peak.
///
/// Two failures to avoid, in opposite directions.
///
/// `level` is a *peak* over one audio callback, so it spikes on any transient —
/// a key press, a chair creak, the first frame after the device opens. Taking
/// the loudest sample let one of those set the floor: a headset idling at 2
/// calibrated to 44, and the bar then sat above anything a person would say.
///
/// The other is starting to talk immediately, which people do, because they
/// just clicked him to say something. Then a *middle* sample is your own voice,
/// the floor is your speaking level, and the bar goes above you — he sits there
/// with his ear open hearing nothing. A quarter-percentile leans on the quiet
/// gaps that exist even in continuous speech.
fn quiet_level(samples: &mut [u8]) -> u8 {
    if samples.is_empty() {
        return 0;
    }
    samples.sort_unstable();
    samples[samples.len() / 4]
}

/// How loud something has to be to count as speech in a room this noisy.
fn speech_bar(floor: u8) -> u8 {
    // A quarter of the floor again, because loud noise fluctuates by more than
    // a fixed margin — and never below the fixed margin in a quiet one.
    let margin = OVER_FLOOR.max(floor / 4);
    floor.saturating_add(margin).max(MIN_THRESHOLD)
}

fn local_hour() -> u8 {
    use chrono::Timelike;
    chrono::Local::now().hour() as u8
}

fn secs(bytes: usize) -> f64 {
    bytes as f64 / (SOURCE_RATE as f64 * 2.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_quiet_headset_gets_a_low_bar() {
        // The bug this replaces: a fixed threshold of 9 against a headset that
        // peaks around 6 while someone talks normally. He heard nothing and
        // closed the turn, which read as "the mic is not picking me up".
        assert!(speech_bar(0) <= 6, "silence should not demand a shout");
        assert!(speech_bar(2) <= 8);
    }

    #[test]
    fn the_bar_is_always_above_the_floor_it_was_measured_from() {
        // The bug this replaces: the bar was clamped to 22 while the measured
        // floor was 44, so the room itself counted as speech, the turn never
        // ended on silence, and he sent whatever the ceiling cut off.
        for floor in [0u8, 1, 5, 20, 44, 80, 200, 255] {
            assert!(
                speech_bar(floor) > floor || floor >= 250,
                "floor {floor} produced bar {}",
                speech_bar(floor)
            );
        }
    }

    #[test]
    fn the_bar_is_never_zero() {
        // At zero, silence itself counts as speech and the turn never ends.
        assert!(speech_bar(0) >= MIN_THRESHOLD);
        assert!(MIN_THRESHOLD > 0);
    }

    #[test]
    fn one_transient_cannot_set_the_noise_floor() {
        // A key press mid-calibration is a single loud sample among quiet
        // ones. Taking the loudest is how a headset idling at 2 calibrated
        // to 44.
        let mut quiet_room_with_a_click = [3, 2, 3, 44, 3, 2, 3];
        assert!(quiet_level(&mut quiet_room_with_a_click) <= 3);
    }

    #[test]
    fn talking_straight_away_does_not_deafen_him() {
        // People click him *because* they have something to say, so half the
        // calibration window can be their own voice. A middle sample would
        // then be speech, and the bar would go above the speaker.
        let mut talking_immediately = [3, 30, 35, 2, 33, 31, 4, 36];
        let floor = quiet_level(&mut talking_immediately);
        assert!(floor <= 5, "floor {floor} was set from the speaker's voice");
        assert!(speech_bar(floor) < 30, "bar would sit above normal speech");
    }

    #[test]
    fn a_genuinely_loud_room_still_reads_as_loud() {
        // It must not simply discard high readings — only unrepresentative
        // ones. A room loud throughout should calibrate loud.
        let mut loud = [40, 44, 41, 43, 45, 42, 44];
        assert!(quiet_level(&mut loud) >= 40);
    }

    #[test]
    fn an_empty_calibration_does_not_panic() {
        assert_eq!(quiet_level(&mut []), 0);
    }

    #[test]
    fn calibration_finishes_well_before_he_gives_up() {
        // If the room were still being measured when the poke timeout fired,
        // every click would close before speech could ever be detected.
        assert!(CALIBRATE_MS * 4 < NO_SPEECH_MS);
        // Waiting for the device to wake must not eat the whole give-up
        // window, or a slow mic would look identical to an empty room.
        assert!(CALIBRATE_CAP_MS > CALIBRATE_MS);
        assert!(CALIBRATE_CAP_MS < NO_SPEECH_MS);
    }

    #[test]
    fn watching_yields_but_a_deliberate_turn_does_not() {
        // The hotkey is invisible from inside a recording, so name-spotting
        // has to poll for it. A deliberate turn must never stand down — it
        // would abandon the very turn the flag was set to ask for.
        assert!(WATCHING.yields);
        assert!(!DELIBERATE.yields);
    }

    #[test]
    fn watching_holds_the_thread_far_more_briefly() {
        // Nothing else is serviced while a watch is recording, so its ceiling
        // bounds how long the hotkey can appear dead in the worst case.
        assert!(WATCHING.max_ms < DELIBERATE.max_ms);
        assert!(WATCHING.max_ms <= 8_000);
        // Long enough to still contain a name plus its pre-roll.
        assert!(WATCHING.max_ms > PREROLL_MS + SPEECH_MS + SILENCE_MS);
        // Watching waits out silence forever; only a deliberate turn gives up.
        assert_eq!(WATCHING.give_up_ms, u64::MAX);
        assert_eq!(DELIBERATE.give_up_ms, NO_SPEECH_MS);
    }

    #[test]
    fn a_room_of_zeros_is_not_a_measured_floor() {
        // The regression: levels read before the device delivers are all zero,
        // so the floor came out 0 and the bar sat at the minimum, which room
        // tone then cleared continuously — every turn ran to the ceiling.
        let mut nothing: Vec<u8> = vec![];
        assert_eq!(quiet_level(&mut nothing), 0);
        assert_eq!(speech_bar(0), MIN_THRESHOLD);

        // Measured from real audio instead, an idling headset sits above it.
        let mut room = vec![6, 7, 6, 8, 7, 6, 9, 7, 6, 7, 8, 6];
        let floor = quiet_level(&mut room);
        assert!(floor >= 6, "floor {floor} should reflect the room, not silence");
        assert!(speech_bar(floor) > 9, "bar must clear the room's own tone");
    }

    #[test]
    fn a_pause_mid_sentence_does_not_end_the_turn_early() {
        assert!(SILENCE_MS < NO_SPEECH_MS);
        assert!(NO_SPEECH_MS < MAX_TURN_MS);
        assert!(SPEECH_MS < SILENCE_MS);
    }

    #[test]
    fn pcm_length_converts_to_seconds() {
        assert!((secs(32_000) - 1.0).abs() < 1e-9);
        assert_eq!(secs(0), 0.0);
    }

    #[test]
    fn a_disabled_ear_never_claims_to_listen() {
        let ear = Ear::new(
            None,
            Arc::new(Mutex::new(AudioSettings::default())),
            Ears {
                on_reply: Arc::new(|_| {}),
                on_idle: Arc::new(|| {}),
                on_wake: Arc::new(|| {}),
                on_thinking: Arc::new(|| {}),
                voice_audible: Arc::new(|| false),
                on_barge: Arc::new(|| true),
            },
        );
        assert!(!ear.is_enabled());
        assert!(!ear.listen(12));
        assert!(!ear.is_listening());
    }

    #[test]
    fn a_reply_parses_from_the_brains_converse_shape() {
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
    }

    #[test]
    fn a_wake_reply_parses() {
        let w: WakeReply =
            serde_json::from_str(r#"{"wake":true,"heard":"yo yasser"}"#).unwrap();
        assert!(w.wake);
        assert_eq!(w.heard, "yo yasser");
        let miss: WakeReply = serde_json::from_str(r#"{"wake":false,"heard":""}"#).unwrap();
        assert!(!miss.wake);
    }

    #[test]
    fn the_preroll_window_is_long_enough_to_hold_his_name() {
        // The brain's wake list includes "asser" and "acer" precisely because
        // capture used to clip the first syllable. The pre-roll is what stops
        // that happening here.
        assert!(PREROLL_MS >= SPEECH_MS * 4);
    }
}
