//! A system-wide key that starts him listening.
//!
//! Clicking him means finding him first: he may be behind a window, on the
//! other monitor, or under whatever you are actually working on. A global
//! hotkey is the difference between "talk to the pet" being a thing you go and
//! do and a thing you just do — press it from inside anything and he opens his
//! ear.
//!
//! `RegisterHotKey` is the right mechanism rather than a keyboard hook: it asks
//! the OS for one specific chord, so nothing else this process does can see
//! your keystrokes. A companion that installs a global key logger to hear
//! "talk to me" would be a poor trade.

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT,
    MOD_SHIFT, MOD_WIN,
};

/// The id we register under. Only meaningful within this window.
pub const HOTKEY_ID: i32 = 0xDF01;

/// Parse a chord like `ctrl+alt+y` into modifiers and a virtual key code.
///
/// Returns `None` for anything unparseable, so a typo in configuration costs
/// the hotkey rather than the companion.
pub fn parse(spec: &str) -> Option<(HOT_KEY_MODIFIERS, u32)> {
    let mut mods: HOT_KEY_MODIFIERS = 0;
    let mut key: Option<u32> = None;

    for part in spec.split('+') {
        let p = part.trim().to_ascii_lowercase();
        if p.is_empty() {
            continue;
        }
        match p.as_str() {
            "ctrl" | "control" => mods |= MOD_CONTROL,
            "alt" => mods |= MOD_ALT,
            "shift" => mods |= MOD_SHIFT,
            "win" | "super" | "meta" => mods |= MOD_WIN,
            other => {
                // Exactly one non-modifier is allowed; a second means the
                // chord is nonsense rather than something to guess at.
                if key.is_some() {
                    return None;
                }
                key = Some(virtual_key(other)?);
            }
        }
    }

    let key = key?;
    // A bare key with no modifier would swallow that key everywhere on the
    // system, which is not a thing to do by accident. Function keys are the
    // conventional exception.
    if mods == 0 && !(0x70..=0x7B).contains(&key) {
        return None;
    }
    // Holding the chord should not open his ear over and over.
    Some((mods | MOD_NOREPEAT, key))
}

fn virtual_key(name: &str) -> Option<u32> {
    let b = name.as_bytes();
    if b.len() == 1 {
        let c = b[0].to_ascii_uppercase();
        if c.is_ascii_alphanumeric() {
            return Some(c as u32);
        }
    }
    Some(match name {
        "space" => 0x20,
        "enter" | "return" => 0x0D,
        "tab" => 0x09,
        "escape" | "esc" => 0x1B,
        "insert" | "ins" => 0x2D,
        "delete" | "del" => 0x2E,
        "home" => 0x24,
        "end" => 0x23,
        "pageup" | "pgup" => 0x21,
        "pagedown" | "pgdn" => 0x22,
        "up" => 0x26,
        "down" => 0x28,
        "left" => 0x25,
        "right" => 0x27,
        "`" | "backtick" | "grave" => 0xC0,
        f if f.starts_with('f') && f.len() <= 3 => {
            let n: u32 = f[1..].parse().ok()?;
            if (1..=12).contains(&n) {
                0x6F + n
            } else {
                return None;
            }
        }
        _ => return None,
    })
}

/// Claim the chord for this window. Returns whether the OS granted it.
///
/// # Safety
/// Must be called on the thread that owns `hwnd`; `WM_HOTKEY` is delivered to
/// that thread's queue.
pub unsafe fn register(hwnd: HWND, spec: &str) -> bool {
    let Some((mods, key)) = parse(spec) else {
        tracing::warn!("hotkey: cannot parse {spec:?}; no shortcut registered");
        return false;
    };
    if RegisterHotKey(hwnd, HOTKEY_ID, mods, key) == 0 {
        // Almost always means another application already owns the chord.
        tracing::warn!("hotkey: {spec} is already taken by something else");
        return false;
    }
    tracing::info!("hotkey: {spec} will start him listening");
    true
}

pub unsafe fn unregister(hwnd: HWND) {
    UnregisterHotKey(hwnd, HOTKEY_ID);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_chord_parses() {
        let (mods, key) = parse("ctrl+alt+y").unwrap();
        assert_eq!(key, b'Y' as u32);
        assert_ne!(mods & MOD_CONTROL, 0);
        assert_ne!(mods & MOD_ALT, 0);
        assert_eq!(mods & MOD_SHIFT, 0);
    }

    #[test]
    fn holding_the_chord_does_not_repeat() {
        // Without NOREPEAT, holding the key reopens his ear every few
        // milliseconds for as long as your finger is down.
        let (mods, _) = parse("ctrl+alt+y").unwrap();
        assert_ne!(mods & MOD_NOREPEAT, 0);
    }

    #[test]
    fn spelling_and_spacing_are_forgiving() {
        assert_eq!(parse("CTRL + ALT + Y"), parse("ctrl+alt+y"));
        assert_eq!(parse("control+alt+y"), parse("ctrl+alt+y"));
        assert_eq!(parse("win+j"), parse("super+j"));
    }

    #[test]
    fn named_keys_parse() {
        assert_eq!(parse("ctrl+shift+space").unwrap().1, 0x20);
        assert_eq!(parse("ctrl+alt+f4").unwrap().1, 0x73);
        assert_eq!(parse("alt+`").unwrap().1, 0xC0);
    }

    #[test]
    fn a_bare_letter_is_refused() {
        // Registering "y" globally would eat the letter y in every
        // application on the machine.
        assert!(parse("y").is_none());
        assert!(parse("space").is_none());
    }

    #[test]
    fn a_bare_function_key_is_allowed() {
        // The conventional exception — nothing types F9.
        assert!(parse("f9").is_some());
        assert_eq!(parse("f9").unwrap().1, 0x78);
    }

    #[test]
    fn nonsense_is_refused_rather_than_guessed_at() {
        assert!(parse("").is_none());
        assert!(parse("ctrl+").is_none());
        assert!(parse("ctrl+alt+nope").is_none());
        assert!(parse("ctrl+a+b").is_none(), "two keys is not a chord");
        assert!(parse("ctrl+f13").is_none());
    }

    #[test]
    fn digits_work_too() {
        assert_eq!(parse("ctrl+alt+1").unwrap().1, b'1' as u32);
    }
}
