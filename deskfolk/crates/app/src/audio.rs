//! Sound devices, playback, and the settings that pick them.
//!
//! The alpha could only choose an output device through the `PET_AUDIO_OUT`
//! environment variable, and there was no way to choose an input at all. That
//! is fine for a prototype you run yourself and unacceptable for something
//! people install, so device selection is a first-class, persisted setting
//! here — enumerated from the OS and changeable at runtime.
//!
//! One hard-won detail carried over from the alpha: **playback is stereo
//! f32**. A mono stream was silently inaudible on the creator's Razer/THX
//! stack — no error, just nothing — which cost a long debugging session.
//! Everything is upmixed to stereo at the device's own sample rate.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

/// The brain streams PCM16 mono at this rate; everything else is derived.
pub const SOURCE_RATE: u32 = 16_000;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AudioSettings {
    /// Device names, not indices — indices reshuffle when you plug in a
    /// headset, which would silently move the user's choice to another device.
    #[serde(default)]
    pub output: Option<String>,
    #[serde(default)]
    pub input: Option<String>,
    /// Listen for his name continuously. Off by default and deliberately so:
    /// it holds the microphone open for as long as he is awake, which is not
    /// something to switch on without being asked.
    #[serde(default)]
    pub wake: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeviceList {
    pub outputs: Vec<String>,
    pub inputs: Vec<String>,
    pub default_output: Option<String>,
    pub default_input: Option<String>,
    pub selected_output: Option<String>,
    pub selected_input: Option<String>,
}

fn host() -> cpal::Host {
    cpal::default_host()
}

/// Everything the OS will let us play to or record from.
pub fn list_devices(settings: &AudioSettings) -> DeviceList {
    let h = host();
    let outputs = h
        .output_devices()
        .map(|ds| ds.filter_map(|d| d.name().ok()).collect())
        .unwrap_or_default();
    let inputs = h
        .input_devices()
        .map(|ds| ds.filter_map(|d| d.name().ok()).collect())
        .unwrap_or_default();
    DeviceList {
        outputs,
        inputs,
        default_output: h.default_output_device().and_then(|d| d.name().ok()),
        default_input: h.default_input_device().and_then(|d| d.name().ok()),
        selected_output: settings.output.clone(),
        selected_input: settings.input.clone(),
    }
}

/// Resolve a saved name back to a device, falling back to the system default.
///
/// A remembered device that is currently unplugged must not mean silence —
/// fall back and say so, rather than appearing broken.
fn pick_output(name: Option<&str>) -> Option<cpal::Device> {
    let h = host();
    if let Some(want) = name {
        if let Ok(mut devices) = h.output_devices() {
            if let Some(d) = devices.find(|d| d.name().map(|n| n == want).unwrap_or(false)) {
                return Some(d);
            }
        }
        tracing::warn!("output device '{want}' not available, using the system default");
    }
    h.default_output_device()
}

/// Used by the recorder, which is built and tested but not yet connected —
/// click-to-talk is the next task, and the menu already remembers the choice.
#[allow(dead_code)]
fn pick_input(name: Option<&str>) -> Option<cpal::Device> {
    let h = host();
    if let Some(want) = name {
        if let Ok(mut devices) = h.input_devices() {
            if let Some(d) = devices.find(|d| d.name().map(|n| n == want).unwrap_or(false)) {
                return Some(d);
            }
        }
        tracing::warn!("input device '{want}' not available, using the system default");
    }
    h.default_input_device()
}

// ---------------------------------------------------------------------------
// Playback
// ---------------------------------------------------------------------------

/// Shared state between the caller feeding PCM and the audio callback.
#[derive(Default)]
struct PlayBuffer {
    samples: std::collections::VecDeque<f32>,
    /// Rolling peak, 0..=100 — what drives the mouth visemes.
    level: u8,
}

/// Streams PCM16 mono into the selected output device as stereo f32.
pub struct Player {
    buffer: Arc<Mutex<PlayBuffer>>,
    _stream: Option<cpal::Stream>,
    device_rate: u32,
}

impl Player {
    /// Open the selected device. Returns `None` if there is no usable output,
    /// which the caller should treat as "no voice", never as a fatal error.
    pub fn open(settings: &AudioSettings) -> Option<Self> {
        let device = pick_output(settings.output.as_deref())?;
        let name = device.name().unwrap_or_else(|_| "<unknown>".into());
        let config = device.default_output_config().ok()?;
        let device_rate = config.sample_rate().0;
        let channels = config.channels() as usize;

        let buffer = Arc::new(Mutex::new(PlayBuffer::default()));
        let cb_buffer = buffer.clone();

        let stream = device
            .build_output_stream(
                &config.config(),
                move |out: &mut [f32], _| {
                    let mut b = cb_buffer.lock();
                    let mut peak = 0.0f32;
                    for frame in out.chunks_mut(channels) {
                        let s = b.samples.pop_front().unwrap_or(0.0);
                        peak = peak.max(s.abs());
                        // Same sample to every channel: mono content, stereo
                        // (or more) device. Feeding a mono stream directly was
                        // what produced silence on some Windows stacks.
                        for c in frame.iter_mut() {
                            *c = s;
                        }
                    }
                    b.level = (peak * 100.0).min(100.0) as u8;
                },
                |err| tracing::warn!("audio output error: {err}"),
                None,
            )
            .map_err(|e| tracing::warn!("could not open output '{name}': {e}"))
            .ok()?;

        stream.play().ok()?;
        tracing::info!("audio out: '{name}' at {device_rate}Hz, {channels}ch");
        Some(Self { buffer, _stream: Some(stream), device_rate })
    }

    /// Feed a chunk of PCM16 mono at [`SOURCE_RATE`].
    pub fn feed_pcm16(&self, pcm: &[u8]) {
        let resampled = resample_mono(pcm, SOURCE_RATE, self.device_rate);
        let mut b = self.buffer.lock();
        b.samples.extend(resampled);
    }

    /// Loudness of what is playing right now, 0..=100.
    pub fn level(&self) -> u8 {
        self.buffer.lock().level
    }

    /// Is there still audio queued to play?
    pub fn is_playing(&self) -> bool {
        !self.buffer.lock().samples.is_empty()
    }

    /// Drop everything queued — used when the user interrupts him.
    pub fn stop(&self) {
        let mut b = self.buffer.lock();
        b.samples.clear();
        b.level = 0;
    }
}

/// PCM16 little-endian mono -> f32 mono at `to` Hz, by linear interpolation.
///
/// Good enough for speech and cheap enough to run on the feeding thread; a
/// proper resampler would be overkill for a 16k voice stream.
fn resample_mono(pcm: &[u8], from: u32, to: u32) -> Vec<f32> {
    let src: Vec<f32> = pcm
        .chunks_exact(2)
        .map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0)
        .collect();
    if src.is_empty() {
        return Vec::new();
    }
    if from == to {
        return src;
    }
    let ratio = to as f64 / from as f64;
    let out_len = ((src.len() as f64) * ratio).round() as usize;
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let pos = i as f64 / ratio;
        let idx = pos.floor() as usize;
        let frac = (pos - idx as f64) as f32;
        let a = src.get(idx).copied().unwrap_or(0.0);
        let b = src.get(idx + 1).copied().unwrap_or(a);
        out.push(a + (b - a) * frac);
    }
    out
}

// ---------------------------------------------------------------------------
// Capture
// ---------------------------------------------------------------------------

/// Records from the selected input as PCM16 mono at [`SOURCE_RATE`], which is
/// exactly what the brain's `/pet/converse` endpoint expects.
///
/// Built and tested, not yet wired: click-to-talk is a task of its own, and
/// the engine's `Effect::OpenMic` is still a log line. Playback *is* connected
/// — see `voice.rs`.
#[allow(dead_code)]
pub struct Recorder {
    _stream: cpal::Stream,
    rx: Receiver<Vec<u8>>,
    level: Arc<Mutex<u8>>,
}

#[allow(dead_code)]
impl Recorder {
    pub fn open(settings: &AudioSettings) -> Option<Self> {
        let device = pick_input(settings.input.as_deref())?;
        let name = device.name().unwrap_or_else(|_| "<unknown>".into());
        let config = device.default_input_config().ok()?;
        let device_rate = config.sample_rate().0;
        let channels = config.channels() as usize;

        let (tx, rx): (Sender<Vec<u8>>, Receiver<Vec<u8>>) = mpsc::channel();
        let level = Arc::new(Mutex::new(0u8));
        let cb_level = level.clone();

        let stream = device
            .build_input_stream(
                &config.config(),
                move |data: &[f32], _| {
                    // Downmix to mono first, then resample to 16k.
                    let mono: Vec<f32> = data
                        .chunks(channels)
                        .map(|f| f.iter().sum::<f32>() / channels as f32)
                        .collect();
                    let mut peak = 0.0f32;
                    for s in &mono {
                        peak = peak.max(s.abs());
                    }
                    *cb_level.lock() = (peak * 100.0).min(100.0) as u8;

                    let bytes = to_pcm16(&resample_f32(&mono, device_rate, SOURCE_RATE));
                    let _ = tx.send(bytes);
                },
                |err| tracing::warn!("audio input error: {err}"),
                None,
            )
            .map_err(|e| tracing::warn!("could not open input '{name}': {e}"))
            .ok()?;

        stream.play().ok()?;
        tracing::info!("audio in: '{name}' at {device_rate}Hz, {channels}ch");
        Some(Self { _stream: stream, rx, level })
    }

    /// Everything captured since the last call.
    pub fn drain(&self) -> Vec<u8> {
        let mut out = Vec::new();
        while let Ok(chunk) = self.rx.try_recv() {
            out.extend_from_slice(&chunk);
        }
        out
    }

    pub fn level(&self) -> u8 {
        *self.level.lock()
    }
}

/// Capture-side resampling; see [`Recorder`].
#[allow(dead_code)]
fn resample_f32(src: &[f32], from: u32, to: u32) -> Vec<f32> {
    if src.is_empty() || from == to {
        return src.to_vec();
    }
    let ratio = to as f64 / from as f64;
    let out_len = ((src.len() as f64) * ratio).round() as usize;
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let pos = i as f64 / ratio;
        let idx = pos.floor() as usize;
        let frac = (pos - idx as f64) as f32;
        let a = src.get(idx).copied().unwrap_or(0.0);
        let b = src.get(idx + 1).copied().unwrap_or(a);
        out.push(a + (b - a) * frac);
    }
    out
}

#[allow(dead_code)]
fn to_pcm16(samples: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(samples.len() * 2);
    for s in samples {
        let v = (s.clamp(-1.0, 1.0) * 32767.0) as i16;
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

// ---------------------------------------------------------------------------
// Persistence
// ---------------------------------------------------------------------------

pub fn settings_path(app: &tauri::AppHandle) -> PathBuf {
    use tauri::Manager;
    app.path()
        .app_data_dir()
        .unwrap_or_else(|_| std::env::temp_dir().join("deskfolk"))
        .join("audio.json")
}

pub fn load_settings(app: &tauri::AppHandle) -> AudioSettings {
    let path = settings_path(app);
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_settings(app: &tauri::AppHandle, settings: &AudioSettings) {
    let path = settings_path(app);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    match serde_json::to_string_pretty(settings) {
        Ok(json) => {
            if let Err(e) = std::fs::write(&path, json) {
                tracing::warn!("could not save audio settings: {e}");
            }
        }
        Err(e) => tracing::warn!("could not serialise audio settings: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pcm16_round_trips_through_resampling_at_the_same_rate() {
        let pcm: Vec<u8> = [0i16, 16384, -16384, 32767]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect();
        let out = resample_mono(&pcm, 16_000, 16_000);
        assert_eq!(out.len(), 4);
        assert!((out[1] - 0.5).abs() < 0.01);
        assert!((out[2] + 0.5).abs() < 0.01);
    }

    #[test]
    fn upsampling_lengthens_by_the_rate_ratio() {
        // 16k -> 48k is the common case on Windows.
        let pcm: Vec<u8> = (0..100i16).flat_map(|v| v.to_le_bytes()).collect();
        let out = resample_mono(&pcm, 16_000, 48_000);
        assert_eq!(out.len(), 300);
    }

    #[test]
    fn downsampling_shortens_by_the_rate_ratio() {
        let src: Vec<f32> = (0..300).map(|i| i as f32 / 300.0).collect();
        assert_eq!(resample_f32(&src, 48_000, 16_000).len(), 100);
    }

    #[test]
    fn empty_and_odd_length_input_never_panics() {
        // A truncated network chunk must not take the audio thread down.
        assert!(resample_mono(&[], 16_000, 48_000).is_empty());
        assert!(resample_mono(&[0x01], 16_000, 48_000).is_empty(), "odd byte is dropped");
        assert!(resample_f32(&[], 48_000, 16_000).is_empty());
    }

    #[test]
    fn pcm16_conversion_clamps_rather_than_wrapping() {
        // Without the clamp, an over-unity sample wraps to full-scale negative
        // and you hear a loud click.
        let bytes = to_pcm16(&[2.0, -2.0]);
        let a = i16::from_le_bytes([bytes[0], bytes[1]]);
        let b = i16::from_le_bytes([bytes[2], bytes[3]]);
        assert_eq!(a, 32767);
        assert_eq!(b, -32767);
    }

    #[test]
    fn settings_round_trip_through_json() {
        let s = AudioSettings {
            output: Some("Razer BlackShark".into()),
            input: Some("Blue Yeti".into()),
            wake: true,
        };
        let back: AudioSettings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back.output.as_deref(), Some("Razer BlackShark"));
        assert_eq!(back.input.as_deref(), Some("Blue Yeti"));
        assert!(back.wake);
    }

    #[test]
    fn missing_settings_file_yields_defaults_not_an_error() {
        let s: AudioSettings = serde_json::from_str("{}").unwrap();
        assert!(s.output.is_none() && s.input.is_none());
    }

    #[test]
    fn an_older_settings_file_does_not_switch_the_microphone_on() {
        // Anyone upgrading has a file with no `wake` key at all. Defaulting
        // that to true would silently start holding their mic open.
        let s: AudioSettings =
            serde_json::from_str(r#"{"output":"Speakers","input":"Mic"}"#).unwrap();
        assert!(!s.wake, "name-spotting must be opt-in, always");
    }

    #[test]
    fn devices_are_remembered_by_name_not_index() {
        // Indices reshuffle when a headset is plugged in; a saved index would
        // silently start pointing at a different device.
        let json = serde_json::to_string(&AudioSettings {
            output: Some("Speakers (THX)".into()),
            input: None,
            wake: false,
        })
        .unwrap();
        assert!(json.contains("Speakers (THX)"));
    }
}
