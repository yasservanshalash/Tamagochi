//! The bridge the webview UI calls into.
//!
//! The onboarding wizard and the Control Center are web pages; this is every
//! `#[tauri::command]` they invoke to read and write the app's state. Keeping
//! them in one module means the `invoke_handler` list in `run()` has a single
//! obvious home, and the UI's contract with Rust is legible in one place.
//!
//! Commands are deliberately thin: they marshal to and from the real modules
//! (`settings`, `paths`, the package crate) and never hold a lock across the
//! network. Nothing here logs a key — `Settings` carries a `ProviderConfig`
//! whose only log-safe printer lives on the type itself.

use base64::Engine;
use tauri::{AppHandle, Manager};

use crate::{audio, paths, settings};

/// The current, persisted settings — the wizard and Control Center read this to
/// populate their controls.
#[tauri::command]
pub fn get_settings(app: AppHandle) -> settings::Settings {
    settings::load(&app)
}

/// Persist settings wholesale. The UI edits a copy and saves it back.
#[tauri::command]
pub fn save_settings(app: AppHandle, settings: settings::Settings) {
    self::settings::save(&app, &settings);
}

/// Mark onboarding complete, persist the wizard's choices in one shot, and boot
/// the companion with them — "that's it, I'm moving in". The wizard closes
/// itself afterwards.
#[tauri::command]
pub fn finish_onboarding(app: AppHandle, mut settings: settings::Settings) {
    settings.first_run_complete = true;
    self::settings::save(&app, &settings);
    tracing::info!("onboarding complete: character={}", settings.character);
    // Close the wizard from Rust — reliably, and *before* the boot is queued —
    // so its close is processed first and the window vanishes instantly. Relying
    // on the webview to close itself was flaky in the bundled build, and doing it
    // after queueing the (heavy) boot left it hanging behind the spin-up.
    if let Some(w) = app.get_webview_window("wizard") {
        let _ = w.close();
    }
    // Then boot him, on the main thread, a beat later.
    let app2 = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Err(e) = crate::boot_companion(&app2) {
            tracing::error!("could not boot the companion after onboarding: {e:#}");
        }
    });
}

/// Every character package discovered under the characters dir — the picker.
#[tauri::command]
pub fn list_characters(app: AppHandle) -> Vec<deskfolk_package::PackageSummary> {
    deskfolk_package::scan(paths::characters_dir(&app))
}

/// A character's avatar as a `data:` URL (his talking face), so the UI can show
/// it with no asset-protocol or CSP fuss. `dir` is the folder id from
/// `list_characters`.
#[tauri::command]
pub fn character_portrait(app: AppHandle, dir: String) -> Option<String> {
    let root = paths::characters_dir(&app).join(&dir);
    let pkg = deskfolk_package::CharacterPackage::load_dir(&root).ok()?;
    let path = pkg.portrait_path()?;
    let bytes = std::fs::read(&path).ok()?;
    let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
    Some(format!("data:image/png;base64,{b64}"))
}

/// Microphones and speakers the OS reports, plus which are selected/default —
/// for the Voice & Ears step and the Settings tab.
#[tauri::command]
pub fn list_audio_devices(app: AppHandle) -> audio::DeviceList {
    let saved = audio::load_settings(&app);
    audio::list_devices(&saved)
}
