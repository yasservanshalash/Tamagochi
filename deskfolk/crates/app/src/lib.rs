//! Deskfolk — a runtime for AI characters that live on your desktop.
//!
//! This crate is the *body*: windows, input, the OS. All the character
//! behavior lives in `deskfolk-engine`, and everything specific to a given
//! character lives in its package. Nothing here mentions Yasser except the
//! default package id.
//!
//! The companion is drawn by `deskfolk-render-win` into a native layered
//! window. Tauri is still here, but only for the Control Center and the tray —
//! a webview could never be the companion, because WebView2's host window
//! insists on drawing its own caption buttons no matter how thoroughly the
//! frame is stripped.

use std::sync::Arc;
use std::time::{Duration, Instant};

use deskfolk_engine::{Effect, Portal};
use deskfolk_package::{CharacterPackage, SpriteMasks, DEFAULT_ALPHA_THRESHOLD};
use deskfolk_render_win::{Companion, Icon, MenuEntry};
use parking_lot::Mutex;
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
    AppHandle, Manager, WebviewUrl, WebviewWindowBuilder,
};

mod audio;
mod config;
mod ear;
mod mind;
mod paths;
mod runtime;
mod voice;

use ear::Ear;
use mind::Mind;
use runtime::{Runtime, GRAB_PADDING, LOOP_MS};
use voice::Voice;

pub struct AppState {
    rt: Arc<Mutex<Runtime>>,
    audio: Arc<Mutex<audio::AudioSettings>>,
    voice: Arc<Voice>,
    ear: Arc<Ear>,
}

/// Menu ids are `deskfolk::out::<device name>` / `::in::<device name>`, so the
/// handler can recover the exact device the user picked without keeping a
/// parallel index that could drift from the menu.
const OUT_PREFIX: &str = "deskfolk::out::";
const IN_PREFIX: &str = "deskfolk::in::";

// ---------------------------------------------------------------------------
// The window's view of the application
// ---------------------------------------------------------------------------

/// What the companion window calls back into.
///
/// Every method here runs on the window's own thread, so each one takes the
/// runtime lock briefly and lets go. Nothing waits on the network or on the
/// mind while holding it.
struct CompanionHost {
    app: AppHandle,
    rt: Arc<Mutex<Runtime>>,
    mind: Arc<Mind>,
    voice: Arc<Voice>,
    ear: Arc<Ear>,
    audio: Arc<Mutex<audio::AudioSettings>>,
}

impl deskfolk_render_win::Host for CompanionHost {
    fn menu(&self) -> Vec<MenuEntry> {
        let asleep = self.rt.lock().engine.is_asleep();
        let settings = self.audio.lock().clone();
        let devices = audio::list_devices(&settings);

        vec![
            // The shortcut is in the label because a global hotkey nobody
            // knows about is a hotkey nobody uses.
            MenuEntry::item("talk", format!("Talk to him    {}", hotkey_label()))
                .with_icon(Icon::Mic),
            MenuEntry::check("wake_word", "Answers to his name", settings.wake)
                .with_icon(Icon::Wake),
            MenuEntry::Separator,
            device_submenu(
                "Microphone",
                Icon::Mic,
                IN_PREFIX,
                &devices.inputs,
                devices.selected_input.as_deref(),
                devices.default_input.as_deref(),
            ),
            device_submenu(
                "Speakers",
                Icon::Speaker,
                OUT_PREFIX,
                &devices.outputs,
                devices.selected_output.as_deref(),
                devices.default_output.as_deref(),
            ),
            MenuEntry::Separator,
            if asleep {
                MenuEntry::item("wake", "Wake up").with_icon(Icon::Wake)
            } else {
                MenuEntry::item("sleep", "Send him to sleep").with_icon(Icon::Sleep)
            },
            MenuEntry::item("center", "Control Center").with_icon(Icon::Panel),
            MenuEntry::Separator,
            MenuEntry::item("quit", "Quit Deskfolk").with_icon(Icon::Quit),
        ]
    }

    fn on_click(&self) {
        // Clicking again while his ear is open means you changed your mind.
        // Without a way out, a mic opened by accident holds him until it times
        // out.
        if self.ear.is_listening() {
            self.ear.cancel();
            self.rt.lock().engine.stop_listening();
            return;
        }

        // Interrupting him mid-sentence is the whole point of poking someone
        // who is talking; letting the old line play under the new one is not.
        self.voice.stop();

        let asleep = {
            let mut rt = self.rt.lock();
            rt.engine.touch();
            rt.engine.is_asleep()
        };

        if asleep {
            // Waking him already emits its own greeting effect.
            let _ = self.rt.lock().engine.wake_up();
            return;
        }

        // One click is the whole interaction: he opens his ear, you talk, and
        // he answers when you stop. Say nothing and it stays a poke — which is
        // why there is no separate "send" and no mode to get stuck in.
        if self.start_listening() {
            return;
        }

        let _ = self.rt.lock().engine.play_emotion("happy", 0, 0);
        mind::ask(
            self.rt.clone(),
            self.mind.clone(),
            self.voice.clone(),
            "user_poke".into(),
            String::new(),
        );
    }

    fn on_menu(&self, id: &str) {
        on_menu(&self.app, id);
    }

    fn on_hotkey(&self) {
        // Same as clicking him, but from wherever you happen to be — so
        // talking to him does not first require finding him on screen.
        tracing::info!("hotkey: opening his ear");
        self.start_listening();
    }

    fn on_moved(&self, x: i32, y: i32) {
        tracing::debug!("companion moved to ({x}, {y})");
    }
}

impl CompanionHost {
    /// Open his ear for one turn. `false` if there is no microphone to open,
    /// so the caller can fall back to treating it as a plain poke.
    fn start_listening(&self) -> bool {
        if !self.ear.is_enabled() {
            return false;
        }
        if self.ear.is_listening() {
            // Already open: a second press means "never mind".
            self.ear.cancel();
            self.rt.lock().engine.stop_listening();
            return true;
        }
        self.voice.stop();
        let hour = local_hour();
        {
            let mut rt = self.rt.lock();
            rt.engine.touch();
            if rt.engine.is_asleep() {
                rt.engine.soft_wake();
            }
            rt.engine.begin_listening();
        }
        if self.ear.listen(hour) {
            return true;
        }
        self.rt.lock().engine.stop_listening();
        false
    }
}

/// A submenu of audio devices with the active one check-marked.
///
/// "System default" is an explicit entry rather than an implicit blank, so the
/// user can always get back to it after picking a specific device.
fn device_submenu(
    title: &str,
    icon: Icon,
    prefix: &str,
    devices: &[String],
    selected: Option<&str>,
    default: Option<&str>,
) -> MenuEntry {
    let mut items = vec![MenuEntry::check(
        prefix,
        match default {
            Some(d) => format!("System default ({d})"),
            None => "System default".to_string(),
        },
        selected.is_none(),
    )];

    if devices.is_empty() {
        // Better an explicit disabled row than an empty menu that looks broken.
        items.push(MenuEntry::disabled("No devices found"));
    } else {
        items.push(MenuEntry::Separator);
        for name in devices {
            items.push(MenuEntry::check(
                format!("{prefix}{name}"),
                name,
                selected == Some(name.as_str()),
            ));
        }
    }

    MenuEntry::submenu(title, icon, items)
}

// ---------------------------------------------------------------------------
// Setup
// ---------------------------------------------------------------------------

pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("DESKFOLK_LOG")
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let handle = app.handle().clone();
            if let Err(e) = boot_companion(&handle) {
                tracing::error!("could not start the companion: {e:#}");
                return Err(e.into());
            }
            install_tray(&handle)?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("failed to launch Deskfolk");
}

fn boot_companion(app: &AppHandle) -> anyhow::Result<()> {
    let id = std::env::var("DESKFOLK_CHARACTER").unwrap_or_else(|_| "yasser".into());
    let root = paths::characters_dir(app).join(&id);
    tracing::info!("loading character package from {}", root.display());

    let pkg = CharacterPackage::load_dir(&root)?;
    for w in &pkg.warnings {
        tracing::warn!("package warning: {w}");
    }

    // The masks are what `compose` measures sprites with; the renderer decodes
    // its own pixels. Click-through no longer needs them — a layered window
    // hit-tests against the alpha we present, per pixel, for free.
    let masks = SpriteMasks::load(&pkg, DEFAULT_ALPHA_THRESHOLD, GRAB_PADDING)?;
    tracing::info!("{} sprites measured ({} KiB)", masks.len(), masks.bytes() / 1024);

    let scale = std::env::var("DESKFOLK_SCALE")
        .ok()
        .and_then(|s| s.parse::<f64>().ok())
        .filter(|s| *s > 0.1 && *s <= 6.0)
        .unwrap_or(1.3);

    let portal = match std::env::var("DESKFOLK_PORTAL").as_deref() {
        Ok("circle") => Portal::Circle,
        Ok("rounded") => Portal::Rounded { radius: 48 },
        _ => Portal::Untethered,
    };

    let stage = pkg.manifest.stage;
    let rt = Runtime::new(pkg.clone(), masks, scale);

    // Provider comes from settings once the Control Center lands; until then
    // it is resolved from the environment and any .env already on disk.
    let (provider_config, why) = config::resolve_provider();
    tracing::info!("mind: {} — {why}", provider_config.describe());
    let mind = Arc::new(Mind::new(&provider_config));

    let audio_settings = Arc::new(Mutex::new(audio::load_settings(app)));
    let devices = audio::list_devices(&audio_settings.lock());
    tracing::info!(
        "audio devices: {} output(s), {} input(s); using out={:?} in={:?}",
        devices.outputs.len(),
        devices.inputs.len(),
        devices.selected_output.as_deref().unwrap_or("system default"),
        devices.selected_input.as_deref().unwrap_or("system default"),
    );

    let voice = Arc::new(Voice::new(config::resolve_voice(), audio_settings.clone()));
    if !voice.is_enabled() {
        tracing::info!("voice: off — he will speak in subtitles only");
    }

    let rt = Arc::new(Mutex::new(rt));

    // What to do when he has actually heard something: remember the exchange,
    // start speaking the reply, and only then hand it to the engine — the
    // pose depends on whether audio is coming.
    let on_reply = {
        let (rt, mind, voice) = (rt.clone(), mind.clone(), voice.clone());
        Arc::new(move |h: ear::Heard| {
            tracing::info!("he replied to speech: {:?} ({})", h.say, h.emotion);
            mind::remember_exchange(&mind, &h.heard, &h.say);
            let has_audio = voice.speak(&h.say);
            let mut guard = rt.lock();
            guard.engine.settle();
            guard.inputs.voice_pending = has_audio;
            let emotion = if h.emotion.is_empty() { "talk".to_string() } else { h.emotion };
            let _ = guard.engine.apply_reply(&deskfolk_engine::Reply {
                say: h.say,
                emotion,
                glitch: h.glitch,
                action: h.action,
                has_audio,
            });
        }) as Arc<dyn Fn(ear::Heard) + Send + Sync>
    };
    // Nothing said, or the brain could not be reached: come back to rest
    // rather than holding whichever pose the turn died in.
    let on_idle = {
        let rt = rt.clone();
        Arc::new(move || rt.lock().engine.settle()) as Arc<dyn Fn() + Send + Sync>
    };
    // You stopped talking. Transcription and a local model take seconds, and
    // he should look like he is working on it rather than still waiting.
    let on_thinking = {
        let rt = rt.clone();
        Arc::new(move || rt.lock().engine.begin_thinking()) as Arc<dyn Fn() + Send + Sync>
    };
    // He heard his name: put his ear up straight away, so the listening ring
    // appears while the rest of the sentence is still arriving.
    let on_wake = {
        let rt = rt.clone();
        Arc::new(move || rt.lock().engine.begin_listening()) as Arc<dyn Fn() + Send + Sync>
    };
    let ear = Arc::new(Ear::new(
        config::resolve_voice(),
        audio_settings.clone(),
        ear::Ears { on_reply, on_idle, on_wake, on_thinking },
    ));

    app.manage(AppState {
        rt: rt.clone(),
        audio: audio_settings.clone(),
        voice: voice.clone(),
        ear: ear.clone(),
    });

    let host = Arc::new(CompanionHost {
        app: app.clone(),
        rt: rt.clone(),
        mind: mind.clone(),
        voice: voice.clone(),
        ear: ear.clone(),
        audio: audio_settings,
    });

    // DESKFOLK_LAYER=desktop makes him an actual resident of the desktop —
    // parented into the wallpaper surface, behind the icons — rather than a
    // window floating over everything.
    let on_desktop = matches!(std::env::var("DESKFOLK_LAYER").as_deref(), Ok("desktop"));

    // Pixel art only survives whole-number scaling. `DESKFOLK_PIXEL_SNAP=off`
    // buys an arbitrary size at the cost of a visibly ragged silhouette.
    let pixel_snap = !matches!(
        std::env::var("DESKFOLK_PIXEL_SNAP").as_deref(),
        Ok("off" | "0" | "false")
    );
    if pixel_snap && (scale.fract() > 0.01 && scale.fract() < 0.99) {
        tracing::info!(
            "scale {scale} is not a whole number; rounding so art pixels stay square \
             (set DESKFOLK_PIXEL_SNAP=off to keep {scale})"
        );
    }

    let companion = Companion::spawn(
        &pkg,
        deskfolk_render_win::Config {
            name: pkg.manifest.name.clone(),
            stage,
            scale,
            portal,
            on_desktop,
            pixel_snap,
            hotkey: hotkey_spec(),
        },
        host,
    )
    .map_err(|e| anyhow::anyhow!("{e}"))?;

    std::thread::Builder::new()
        .name("deskfolk-companion-loop".into())
        .spawn(move || companion_loop(companion, rt, mind, voice, ear))?;

    Ok(())
}

// ---------------------------------------------------------------------------
// The loop
// ---------------------------------------------------------------------------

/// How often to re-assert always-on-top. A full-screen application can quietly
/// take the top slot; without this he disappears behind it and never returns.
const RAISE_EVERY: Duration = Duration::from_secs(5);

fn companion_loop(
    companion: Companion,
    rt: Arc<Mutex<Runtime>>,
    mind: Arc<Mind>,
    voice: Arc<Voice>,
    ear: Arc<Ear>,
) {
    let period = Duration::from_millis(LOOP_MS);
    let mut last = Instant::now();
    let mut last_raise = Instant::now();

    loop {
        std::thread::sleep(period);

        let now = Instant::now();
        let dt = now.duration_since(last).as_millis() as i64;
        last = now;

        if now.duration_since(last_raise) > RAISE_EVERY {
            last_raise = now;
            companion.raise();
        }

        let hour = local_hour();
        let (frame, effects) = {
            let mut guard = rt.lock();
            guard.inputs.local_hour = hour;
            // The voice thread is the only thing that knows whether sound is
            // actually coming out; the engine's nap gating, viseme picking and
            // mic gating all hang off these three values.
            guard.inputs.voice_audible = voice.audible();
            guard.inputs.voice_pending = voice.pending();
            // While the mic is open the level is *your* voice, not his — it
            // is what makes the listening ring pulse as you speak.
            guard.inputs.voice_level = if ear.is_listening() {
                ear.level()
            } else {
                voice.level()
            };
            guard.inputs.cursor = cursor_in_stage(&companion, &guard);
            let inputs = guard.inputs.clone();
            let effects = guard.engine.tick(dt, &inputs);
            let frame = guard.current_frame();
            (frame, effects)
        };

        for e in &effects {
            handle_effect(&rt, &mind, &voice, &ear, hour, e);
        }

        // Only wake the window when something visible actually changed.
        let changed = {
            let mut guard = rt.lock();
            if guard.last_sent.as_ref() == Some(&frame) {
                false
            } else {
                guard.last_sent = Some(frame.clone());
                true
            }
        };
        if changed {
            companion.present(frame);
        }
    }
}

/// Where the cursor is in his coordinate space, so he can look at it.
fn cursor_in_stage(companion: &Companion, rt: &Runtime) -> Option<(i32, i32)> {
    let (cx, cy) = deskfolk_render_win::cursor_pos();
    let (ox, oy) = companion.position();
    Some(rt.cursor_to_stage(
        (cx as f64, cy as f64),
        (ox as f64, oy as f64),
        companion.dpi_factor(),
    ))
}

fn handle_effect(
    rt: &Arc<Mutex<Runtime>>,
    mind: &Arc<Mind>,
    voice: &Arc<Voice>,
    ear: &Arc<Ear>,
    hour: u8,
    e: &Effect,
) {
    match e {
        Effect::Log(msg) => tracing::info!("pet: {msg}"),
        Effect::Think { event, text } => mind::ask(
            rt.clone(),
            mind.clone(),
            voice.clone(),
            event.clone(),
            text.clone(),
        ),
        // He asked to listen — the engine only emits this once he has finished
        // speaking, which is what stops him recording his own voice.
        Effect::OpenMic => {
            rt.lock().engine.begin_listening();
            if !ear.listen(hour) {
                rt.lock().engine.stop_listening();
            }
        }
        Effect::StopVoice => voice.stop(),
    }
}

fn install_tray(app: &AppHandle) -> tauri::Result<()> {
    let menu = Menu::with_items(
        app,
        &[
            &MenuItem::with_id(app, "center", "Control Center", true, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "quit", "Quit Deskfolk", true, None::<&str>)?,
        ],
    )?;

    let mut builder = TrayIconBuilder::with_id("deskfolk")
        .tooltip("Deskfolk")
        .menu(&menu)
        .on_menu_event(|app, event| on_menu(app, event.id.as_ref()));
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

fn on_menu(app: &AppHandle, id: &str) {
    // Device picks carry the device name in the id.
    if let Some(name) = id.strip_prefix(OUT_PREFIX) {
        set_device(app, Some(name.to_string()), None);
        return;
    }
    if let Some(name) = id.strip_prefix(IN_PREFIX) {
        set_device(app, None, Some(name.to_string()));
        return;
    }

    match id {
        "quit" => app.exit(0),
        "center" => open_control_center(app),
        "wake_word" => {
            if let Some(state) = app.try_state::<AppState>() {
                let on = {
                    let mut s = state.audio.lock();
                    s.wake = !s.wake;
                    s.wake
                };
                state.ear.set_wake(on);
                audio::save_settings(app, &state.audio.lock());
                tracing::info!(
                    "name-spotting {} — the microphone is {} while he is awake",
                    if on { "on" } else { "off" },
                    if on { "held open" } else { "closed" }
                );
            }
        }
        "sleep" => {
            if let Some(state) = app.try_state::<AppState>() {
                state.voice.stop();
                state.ear.cancel();
                state.rt.lock().engine.sleep();
            }
        }
        "wake" => {
            if let Some(state) = app.try_state::<AppState>() {
                let _ = state.rt.lock().engine.wake_up();
            }
        }
        "talk" => {
            if let Some(state) = app.try_state::<AppState>() {
                state.voice.stop();
                {
                    let mut rt = state.rt.lock();
                    rt.engine.touch();
                    rt.engine.begin_listening();
                }
                if !state.ear.listen(local_hour()) {
                    state.rt.lock().engine.stop_listening();
                    tracing::warn!("no microphone available; nothing to talk into");
                }
            }
        }
        _ => {}
    }
}

/// Persist a device choice. An empty name means "system default", which is
/// how the menu's first entry comes through.
fn set_device(app: &AppHandle, output: Option<String>, input: Option<String>) {
    let Some(state) = app.try_state::<AppState>() else { return };
    {
        let mut s = state.audio.lock();
        if let Some(o) = output {
            s.output = if o.is_empty() { None } else { Some(o) };
            tracing::info!("speakers set to {:?}", s.output.as_deref().unwrap_or("system default"));
        }
        if let Some(i) = input {
            s.input = if i.is_empty() { None } else { Some(i) };
            tracing::info!("microphone set to {:?}", s.input.as_deref().unwrap_or("system default"));
        }
    }
    audio::save_settings(app, &state.audio.lock());
}

fn open_control_center(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("control-center") {
        let _ = w.show();
        let _ = w.set_focus();
        return;
    }
    let _ = WebviewWindowBuilder::new(
        app,
        "control-center",
        WebviewUrl::App("index.html".into()),
    )
    .title("Deskfolk")
    .inner_size(1100.0, 720.0)
    .min_inner_size(880.0, 560.0)
    .build();
}

fn local_hour() -> u8 {
    use chrono::Timelike;
    chrono::Local::now().hour() as u8
}

/// The chord that starts him listening, or `None` to register nothing.
///
/// Ctrl+Alt+Y by default: three keys deep enough that nothing else claims it,
/// and the letter is his. `DESKFOLK_HOTKEY=off` turns it off.
fn hotkey_spec() -> Option<String> {
    match std::env::var("DESKFOLK_HOTKEY") {
        Ok(v) if matches!(v.as_str(), "off" | "0" | "false" | "none") => None,
        Ok(v) if !v.trim().is_empty() => Some(v.trim().to_string()),
        _ => Some("ctrl+alt+y".to_string()),
    }
}

/// How to write that chord on a menu row.
fn hotkey_label() -> String {
    hotkey_spec()
        .map(|s| {
            s.split('+')
                .map(|p| {
                    let p = p.trim();
                    let mut c = p.chars();
                    match c.next() {
                        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                        None => String::new(),
                    }
                })
                .collect::<Vec<_>>()
                .join("+")
        })
        .unwrap_or_default()
}
