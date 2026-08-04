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
mod journal;
mod mind;
mod stroll;
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
            MenuEntry::item("journal", "Today's log").with_icon(Icon::Panel),
            MenuEntry::submenu("Test animations", Icon::Panel, animation_menu(&self.rt)),
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
            journal::did("stopped listening (asked again)");
            return true;
        }
        // Worth a line even though nothing was said yet: a turn that opens and
        // produces no reply is otherwise a gap in the transcript with nothing
        // to explain it, and "he didn't hear me" is exactly what one wants to
        // look up afterwards.
        journal::did("opened his ear");
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
        .build(tauri::generate_context!())
        .expect("failed to launch Deskfolk")
        .run(|_app, event| {
            // Seal the journal however the session ends, not only via the menu
            // item — closing from the tray, a shutdown, or anything else that
            // unwinds cleanly should still leave the day summarised. `finish`
            // only writes once, so the menu path calling it first is fine.
            if matches!(event, tauri::RunEvent::Exit) {
                journal::finish();
            }
        });
}

/// Menu id prefix for "play this emotion now", used by the animation tester.
const ANIM_PREFIX: &str = "deskfolk::anim::";

/// How long a tested animation holds before he drops back to normal. Long
/// enough to watch a loop go round more than once.
const ANIM_HOLD_MS: i64 = 8_000;

/// Which drawer each preview animation belongs in.
///
/// Grouped rather than listed flat because the menu is a column of cards: a
/// single list of every emotion is taller than the screen, and the ones that
/// fell off the bottom would be unreachable.
const ANIM_GROUPS: [(&str, &[&str]); 3] = [
    (
        "Moving",
        &["walk", "walk_right", "hd_run", "hd_turn", "hd_jump", "hd_stand"],
    ),
    (
        "Sitting",
        &["hd_sit_down", "hd_drink", "hd_take_coffee", "hd_laptop", "hd_phone", "hd_sleep"],
    ),
    (
        "Gestures",
        &[
            "hd_wave", "hd_point", "hd_shrug", "hd_facepalm", "hd_arms_crossed",
            "hd_cheer", "hd_dance", "hd_yawn", "hd_thinking",
        ],
    ),
];

/// Every animation in the package, so a new one can be looked at without
/// waiting for the situation that triggers it.
///
/// Anything the package has that is not in a named group still appears, under
/// "Built-in" — adding a clip to `character.json` should be enough to be able
/// to see it, without also having to remember to list it here.
fn animation_menu(rt: &Arc<Mutex<Runtime>>) -> Vec<MenuEntry> {
    let guard = rt.lock();
    let have: Vec<String> =
        guard.engine.package().manifest.emotions.keys().cloned().collect();
    drop(guard);

    let entry = |n: &str| MenuEntry::item(format!("{ANIM_PREFIX}{n}"), pretty(n));
    let mut out = Vec::new();
    let mut placed: Vec<&str> = Vec::new();
    for (label, names) in ANIM_GROUPS {
        let items: Vec<MenuEntry> = names
            .iter()
            .filter(|n| have.iter().any(|h| h == *n))
            .map(|n| {
                placed.push(n);
                entry(n)
            })
            .collect();
        if !items.is_empty() {
            out.push(MenuEntry::submenu(label, Icon::Panel, items));
        }
    }
    let mut rest: Vec<&String> =
        have.iter().filter(|h| !placed.contains(&h.as_str())).collect();
    rest.sort();
    if !rest.is_empty() {
        out.push(MenuEntry::submenu(
            "Built-in",
            Icon::Panel,
            rest.iter().map(|n| entry(n)).collect(),
        ));
    }
    out
}

/// `hd_arms_crossed` reads as "Arms crossed" in a menu.
fn pretty(id: &str) -> String {
    let words = id.strip_prefix("hd_").unwrap_or(id).replace('_', " ");
    let mut c = words.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => words,
    }
}

/// What is running, for the journal's session header.
///
/// A transcript is worth much less without it: "he sounded flat today" is only
/// actionable next to which model, voice and register produced it.
///
/// With a sidecar the app genuinely does not know — the brain owns the mind,
/// the voice and the ears — so it has to ask. The probe runs on its own thread
/// because `reqwest::blocking` may not be called from inside the async
/// runtime, and a brain that does not answer quickly just leaves those lines
/// out rather than holding up his appearing on screen.
fn describe_stack(
    provider: &deskfolk_ai::ProviderConfig,
    pkg: &CharacterPackage,
    devices: &audio::DeviceList,
) -> Vec<(String, String)> {
    let mut stack = vec![
        ("Package".into(), format!("{} v{}", pkg.manifest.name, pkg.manifest.version)),
        ("Mind".into(), provider.describe()),
    ];
    if let deskfolk_ai::ProviderConfig::Sidecar { base_url } = provider {
        if let Some(h) = brain_health(base_url) {
            let get = |a: &str, b: &str| -> Option<String> {
                h.get(a)?.get(b)?.as_str().map(str::to_string)
            };
            if let Some(m) = h.get("model").and_then(|v| v.as_str()) {
                stack.push(("Model".into(), m.to_string()));
            }
            if let (Some(m), Some(v)) = (get("tts", "model"), get("tts", "voice")) {
                stack.push(("Voice".into(), format!("{m}, voice {v}")));
            }
            if let Some(m) = get("stt", "model") {
                stack.push(("Ears".into(), m));
            }
            if let Some(r) = h.get("register") {
                stack.push(("Register".into(), r.to_string()));
            }
        }
    }
    stack.push((
        "Mic".into(),
        devices.selected_input.clone().unwrap_or_else(|| "system default".into()),
    ));
    stack
}

fn brain_health(base_url: &str) -> Option<serde_json::Value> {
    let url = format!("{}/health", base_url.trim_end_matches('/'));
    std::thread::spawn(move || {
        reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_millis(1500))
            .build()
            .ok()?
            .get(&url)
            .send()
            .ok()?
            .json::<serde_json::Value>()
            .ok()
    })
    .join()
    .ok()
    .flatten()
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

    // Today's journal. Started here rather than at the first reply so that a
    // session which crashes before he says anything still leaves a record of
    // what was running when it did.
    let stack = describe_stack(&provider_config, &pkg, &devices);
    match journal::start(&paths::companion_data_dir(app, &id).join("journal"), &id, stack) {
        Some(p) => tracing::info!("journal: today's log is {}", p.display()),
        None => tracing::warn!("journal: could not be started; today will go unrecorded"),
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
            // Spoken turns never pass through `mind::ask` — the ear talks to
            // the brain itself — so this is the only place they can be
            // journalled, and they are the most interesting entries there are.
            journal::said(journal::Said {
                event: "user_speech".into(),
                heard: h.heard.clone(),
                say: h.say.clone(),
                emotion: h.emotion.clone(),
                glitch: h.glitch,
                action: h.action.clone(),
                spoken: has_audio,
                ..Default::default()
            });
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

    // Movement. The gait comes from the package: a `walk` clip makes him walk,
    // and without one he hops, because sliding a seated sprite across the
    // screen reads as a bug rather than as movement.
    let gait = stroll::Gait::of(
        rt.lock().engine.package().manifest.clips.contains_key("walk"),
    );
    // How tall the walk art actually draws him, so his pace can be derived
    // from it rather than tuned in pixels against one particular display.
    let walker_h = walker_height(&rt);
    let wander = std::env::var("DESKFOLK_WANDER").as_deref() != Ok("off");
    if wander {
        tracing::info!("wander: on, {gait:?} — he will move between your windows");
    }
    let mut walk = stroll::Stroll::new(stroll::REST_MIN.as_millis() as i64);
    let mut roll: u32 = 0;

    loop {
        std::thread::sleep(period);

        let now = Instant::now();
        let dt = now.duration_since(last).as_millis() as i64;
        last = now;

        if now.duration_since(last_raise) > RAISE_EVERY {
            last_raise = now;
            companion.raise();
        }

        if wander {
            roll = roll.wrapping_add(1);
            wander_tick(&companion, &rt, &mut walk, gait, walker_h, dt, roll);
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

/// One tick of wandering: read the desktop, decide, move.
///
/// The engine is not involved in *where* he is — it owns what he is doing, and
/// position is the window's business — but a step plays through it so the hop
/// or the walk cycle animates the same way any other emotion does.
fn wander_tick(
    companion: &Companion,
    rt: &Arc<Mutex<Runtime>>,
    walk: &mut stroll::Stroll,
    gait: stroll::Gait,
    walker_h: u32,
    dt: i64,
    roll: u32,
) {
    // He only wanders when he has nothing better to do. Walking off mid-answer
    // would be worse than standing still.
    let free = {
        let guard = rt.lock();
        !guard.engine.is_asleep()
            && !guard.inputs.busy
            && !guard.inputs.voice_pending
            && !guard.inputs.voice_audible
    };

    let (x, _) = companion.position();
    let (w, _) = companion.size();
    // His height as drawn, which is what every pace in `stroll` is derived
    // from: the same walk has to read the same on a 4K panel as on a 1080p one.
    let height_px = (walker_h as f64 * companion.unit()).round() as i32;
    match walk.tick(dt, gait, height_px, free) {
        stroll::Step::Stay => {}
        stroll::Step::Move { x, y, facing } => {
            companion.move_to(x, y);
            // Hold the walk cycle while he is moving, re-asserted rather than
            // set once: a clip with a hold would otherwise lapse back to idle
            // part-way across the screen. Re-asserting also flips him the
            // instant the direction changes. Hopping has no clip to hold.
            if let Some(e) = gait.emotion(facing) {
                if roll % 20 == 0 {
                    let played = !rt.lock().engine.play_emotion(e, 0, 0).is_empty();
                    tracing::debug!("wander: holding {e:?} (resolved: {played})");
                }
            }
        }
        stroll::Step::Arrived { on } => {
            tracing::debug!("wander: settled on {on:?}");
            journal::did(format!("moved to stand on {on:?}"));
            // The standing pose, not a frozen stride and not his beanbag: he
            // has just walked somewhere and is on his feet there.
            let _ = rt.lock().engine.play_emotion("stand", 0, 0);
            walk.rest(on, rest_for(roll));
            return;
        }
    }

    // Mid-journey, or not yet restless: nothing to decide, and no reason to
    // enumerate every window on the desktop this frame.
    if !free || walk.is_walking() || !walk.restless() {
        if roll % 500 == 0 {
            tracing::debug!(
                "wander: waiting — free={free}, walking={}, restless={}",
                walk.is_walking(),
                walk.restless()
            );
        }
        return;
    }
    // Time to consider somewhere new.
    let ledges = companion.ledges();
    let resting_on = match walk {
        stroll::Stroll::Resting { on, .. } => on.clone(),
        _ => String::new(),
    };
    match stroll::pick(&ledges, x + w / 2, &resting_on, roll) {
        Some(l) => {
            // Land his *feet* on the edge. The stage keeps empty canvas below
            // the anchor, so putting the window's bottom there left him
            // hovering above the ledge by that margin.
            let target_x = l.clamp_x(x + w / 2, w / 2) - w / 2;
            let target_y = l.top - companion.feet_offset();
            tracing::debug!(
                "wander: setting off for {:?} — ledge top {}, feet land there with \
                 his window at y {target_y}",
                l.title,
                l.top,
            );
            walk.walk_to(l.title.clone(), x, target_x, target_y);
        }
        // Nowhere worth going: wait before asking again rather than
        // re-scanning the whole desktop every frame.
        None => walk.rest(resting_on, rest_for(roll)),
    }
}

/// How tall he is drawn, in art pixels, taken from the walk art itself.
///
/// Asked of the package rather than hardcoded, because it is the number every
/// pace is derived from and it changes the moment the sprite sheet does. The
/// stage height is not a substitute — it is the canvas, and he does not fill
/// it.
fn walker_height(rt: &Arc<Mutex<Runtime>>) -> u32 {
    let guard = rt.lock();
    let pkg = guard.engine.package();
    pkg.clip("walk")
        .or_else(|| pkg.clip(pkg.role_clip(deskfolk_package::Role::Idle)))
        .and_then(|c| c.frames.first())
        .and_then(|f| guard.masks.get(&f.img))
        .map(|m| m.height)
        .unwrap_or(pkg.manifest.stage.anchor_y.max(1) as u32)
}

/// A rest somewhere between the two bounds, varied so he is not metronomic.
fn rest_for(roll: u32) -> i64 {
    let lo = stroll::REST_MIN.as_millis() as i64;
    let hi = stroll::REST_MAX.as_millis() as i64;
    lo + (roll as i64 * 7919) % (hi - lo).max(1)
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

    // Play an animation on demand, for looking at one while working on it.
    if let Some(name) = id.strip_prefix(ANIM_PREFIX) {
        if let Some(state) = app.try_state::<AppState>() {
            let mut guard = state.rt.lock();
            // Wake him first: asleep, the sleep clip owns the base and
            // whatever was asked for would never be seen.
            if guard.engine.is_asleep() {
                let _ = guard.engine.wake_up();
            }
            let fx = guard.engine.play_emotion(name, 0, ANIM_HOLD_MS);
            tracing::info!("animation test: {name} ({} effects)", fx.len());
            journal::did(format!("animation {name:?} played from the test menu"));
        }
        return;
    }

    match id {
        "quit" => {
            // Seal the day's journal before the process goes. Everything up to
            // here is already on disk; this is what adds the summary.
            journal::finish();
            app.exit(0)
        }
        "center" => open_control_center(app),
        // Opening it mid-session is useful precisely because the file is
        // written as it happens: what he just said is already in there.
        "journal" => match journal::path() {
            Some(p) => {
                if let Err(e) = tauri_plugin_opener::open_path(&p, None::<&str>) {
                    tracing::warn!("journal: could not open {}: {e}", p.display());
                }
            }
            None => tracing::warn!("journal: nothing to open — none was started"),
        },
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
                journal::did("sent to sleep");
            }
        }
        "wake" => {
            if let Some(state) = app.try_state::<AppState>() {
                let _ = state.rt.lock().engine.wake_up();
                journal::did("woken up");
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn animation_ids_round_trip_through_the_menu() {
        // The dispatch strips this prefix to get the emotion name back, so the
        // two have to agree or every entry silently does nothing.
        let id = format!("{ANIM_PREFIX}hd_walk");
        assert_eq!(id.strip_prefix(ANIM_PREFIX), Some("hd_walk"));
        // And it must not collide with the device menus, which are matched
        // first and would swallow it.
        assert!(!id.starts_with(OUT_PREFIX));
        assert!(!id.starts_with(IN_PREFIX));
    }

    #[test]
    fn animation_labels_are_readable() {
        assert_eq!(pretty("hd_arms_crossed"), "Arms crossed");
        assert_eq!(pretty("hd_walk"), "Walk");
        // Clips that were always there keep their own names.
        assert_eq!(pretty("suspicious"), "Suspicious");
        assert_eq!(pretty(""), "");
    }

    #[test]
    fn a_tested_animation_is_held_long_enough_to_watch() {
        assert!(ANIM_HOLD_MS > 4_000, "a loop should go round more than once");
    }
}
