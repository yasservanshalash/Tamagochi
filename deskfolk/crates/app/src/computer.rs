//! Letting him work the computer itself.
//!
//! The same philosophy as the music: a closed set of deeds, parsed strictly —
//! a model that invents a verb does nothing rather than something surprising.
//! Opening things routes through the default browser (rundll32, which unlike
//! `cmd /C start` does not eat query strings); window deeds go through Win32
//! against windows matched by case-insensitive title substring, using the same
//! "is this a real window" rules the ledges already trust.
//!
//! Deliberately NOT here: arbitrary shell commands, file access, typing into
//! other programs. Every deed is either "show a page", "launch an allowlisted
//! app", or "rearrange windows the way a taskbar click would".

/// Something he can do to the computer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Deed {
    /// Open a URL in the default browser.
    OpenUrl(String),
    /// Search the web for something.
    Search(String),
    /// Search YouTube for something.
    Youtube(String),
    /// Bring the window whose title contains this to the front.
    Focus(String),
    /// Minimize the window whose title contains this.
    Minimize(String),
    /// Ask the window whose title contains this to close (graceful WM_CLOSE —
    /// the app can still prompt to save).
    Close(String),
    /// Launch an allowlisted app.
    OpenApp(App),
    /// Type this text into whatever window is focused — dictation.
    Type(String),
    /// Put this text on the clipboard, ready to paste anywhere.
    Clipboard(String),
    /// Minimize everything — the Win+D reflex.
    ShowDesktop,
    /// Teleport the companion to a region of the screen ("top right").
    GotoScreen(String),
}

/// The only programs he may start. A closed list on purpose: "open <thing>"
/// comes from speech-to-text, and a misheard word must never launch something
/// unexpected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum App {
    Notepad,
    Calculator,
    Explorer,
    Spotify,
    Settings,
    Discord,
}

impl App {
    fn parse(name: &str) -> Option<App> {
        Some(match name.trim().to_ascii_lowercase().as_str() {
            "notepad" => App::Notepad,
            "calculator" | "calc" => App::Calculator,
            "explorer" | "file explorer" | "files" => App::Explorer,
            "spotify" => App::Spotify,
            "settings" | "windows settings" => App::Settings,
            "discord" => App::Discord,
            _ => return None,
        })
    }

    fn label(&self) -> &'static str {
        match self {
            App::Notepad => "notepad",
            App::Calculator => "the calculator",
            App::Explorer => "file explorer",
            App::Spotify => "Spotify",
            App::Settings => "settings",
            App::Discord => "Discord",
        }
    }
}

/// Turn a spoken site name into a URL: known names map directly, anything
/// with a dot is treated as a domain, and everything else is rejected so the
/// caller can fall back to a search. Never returns a non-https scheme.
fn url_for(site: &str) -> Option<String> {
    let s = site.trim().trim_end_matches(['.', '!', '?', ',']).to_ascii_lowercase();
    if s.is_empty() || s.contains(char::is_whitespace) {
        return None;
    }
    let known = match s.as_str() {
        "youtube" => "https://www.youtube.com",
        "google" => "https://www.google.com",
        "gmail" => "https://mail.google.com",
        "github" => "https://github.com",
        "reddit" => "https://www.reddit.com",
        "twitter" | "x" => "https://x.com",
        "instagram" => "https://www.instagram.com",
        "twitch" => "https://www.twitch.tv",
        "netflix" => "https://www.netflix.com",
        "wikipedia" => "https://www.wikipedia.org",
        "chatgpt" => "https://chatgpt.com",
        "claude" => "https://claude.ai",
        _ => "",
    };
    if !known.is_empty() {
        return Some(known.to_string());
    }
    // A bare domain someone read out: "openai.com". Refuse anything that
    // smells like a scheme of its own — https is the only door.
    if s.contains("://") {
        return s.starts_with("https://").then_some(s);
    }
    s.contains('.').then(|| format!("https://{s}"))
}

fn urlenc(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

impl Deed {
    /// Parse what the mind asked for. Unknown verbs are ignored, not guessed.
    pub fn parse(verb: &str, arg: &str) -> Option<Deed> {
        let a = arg.trim();
        Some(match verb.trim().to_ascii_lowercase().as_str() {
            "open_url" | "open" | "goto" | "go_to" => {
                // A known app name said as "open X" is the app, not a site.
                if let Some(app) = App::parse(a) {
                    Deed::OpenApp(app)
                } else if let Some(url) = url_for(a) {
                    Deed::OpenUrl(url)
                } else if !a.is_empty() {
                    Deed::Search(a.to_string())
                } else {
                    return None;
                }
            }
            "search" | "google" if !a.is_empty() => Deed::Search(a.to_string()),
            "youtube" | "youtube_search" if !a.is_empty() => Deed::Youtube(a.to_string()),
            "focus" | "switch" | "switch_to" if !a.is_empty() => Deed::Focus(a.to_string()),
            "minimize" if !a.is_empty() => Deed::Minimize(a.to_string()),
            "close" if !a.is_empty() => Deed::Close(a.to_string()),
            "open_app" => Deed::OpenApp(App::parse(a)?),
            "type" | "dictate" if !a.is_empty() => Deed::Type(a.to_string()),
            "clipboard" | "copy" if !a.is_empty() => Deed::Clipboard(a.to_string()),
            "show_desktop" => Deed::ShowDesktop,
            "goto_screen" if !a.is_empty() => Deed::GotoScreen(a.to_string()),
            _ => return None,
        })
    }

    /// How it reads in the journal.
    pub fn describe(&self) -> String {
        match self {
            Deed::OpenUrl(u) => format!("opened {u}"),
            Deed::Search(q) => format!("searched the web for {q:?}"),
            Deed::Youtube(q) => format!("searched YouTube for {q:?}"),
            Deed::Focus(w) => format!("brought {w:?} to the front"),
            Deed::Minimize(w) => format!("minimized {w:?}"),
            Deed::Close(w) => format!("closed {w:?}"),
            Deed::OpenApp(a) => format!("opened {}", a.label()),
            Deed::Type(t) => format!("typed {} chars into the focused window", t.chars().count()),
            Deed::Clipboard(t) => format!("put {} chars on the clipboard", t.chars().count()),
            Deed::ShowDesktop => "showed the desktop".into(),
            Deed::GotoScreen(c) => format!("teleported to the {c} of the screen"),
        }
    }
}

/// Carry out a deed. `false` means it could not be done — a window that is
/// not there, a platform without the plumbing — and the caller should say so
/// out loud rather than leave a silent nothing.
pub fn grant(deed: &Deed) -> bool {
    match deed {
        Deed::OpenUrl(url) => {
            crate::spotify::open_browser(url);
            true
        }
        Deed::Search(q) => {
            crate::spotify::open_browser(&format!(
                "https://www.google.com/search?q={}",
                urlenc(q)
            ));
            true
        }
        Deed::Youtube(q) => {
            crate::spotify::open_browser(&format!(
                "https://www.youtube.com/results?search_query={}",
                urlenc(q)
            ));
            true
        }
        Deed::Focus(name) => window_deed(name, WindowDeed::Focus),
        Deed::Minimize(name) => window_deed(name, WindowDeed::Minimize),
        Deed::Close(name) => window_deed(name, WindowDeed::Close),
        Deed::OpenApp(app) => {
            let ok = match app {
                App::Notepad => spawn("notepad.exe"),
                App::Calculator => spawn("calc.exe"),
                App::Explorer => spawn("explorer.exe"),
                App::Spotify => {
                    crate::spotify::open_browser("spotify:");
                    true
                }
                App::Settings => {
                    crate::spotify::open_browser("ms-settings:");
                    true
                }
                App::Discord => {
                    // The window first, the app second: "open discord" from
                    // someone who has it running means "show me discord".
                    if window_deed("discord", WindowDeed::Focus) {
                        true
                    } else {
                        crate::spotify::open_browser("discord://");
                        true
                    }
                }
            };
            ok
        }
        Deed::Type(text) => type_text(text),
        Deed::Clipboard(text) => set_clipboard(text),
        Deed::ShowDesktop => {
            chord(&[0x5B, 0x44]); // Win+D
            true
        }
        Deed::GotoScreen(c) => goto_screen(c).is_ok(),
    }
}

/// Teleport the companion window to a named region. `Ok(Some(title))` means
/// he got there but is now sitting on top of that window — worth saying.
pub fn goto_screen(region: &str) -> Result<Option<String>, ()> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::RECT;
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            FindWindowW, GetWindowRect, SetWindowPos, SystemParametersInfoW, HWND_TOPMOST,
            SPI_GETWORKAREA, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER,
        };

        let title: Vec<u16> = "Deskfolk".encode_utf16().chain([0]).collect();
        let hwnd = unsafe { FindWindowW(std::ptr::null(), title.as_ptr()) };
        if hwnd as isize == 0 {
            return Err(());
        }
        let mut me: RECT = unsafe { std::mem::zeroed() };
        let mut work: RECT = unsafe { std::mem::zeroed() };
        unsafe {
            if GetWindowRect(hwnd, &mut me) == 0 {
                return Err(());
            }
            SystemParametersInfoW(SPI_GETWORKAREA, 0, (&mut work as *mut RECT).cast(), 0);
        }
        let (w, h) = (me.right - me.left, me.bottom - me.top);
        let r = region.to_lowercase();
        let x = if r.contains("left") {
            work.left
        } else if r.contains("right") {
            work.right - w
        } else {
            (work.left + work.right - w) / 2
        };
        let y = if r.contains("top") || r.contains("upper") {
            work.top
        } else if r.contains("bottom") || r.contains("lower") {
            work.bottom - h
        } else if r.contains("center") || r.contains("middle") {
            (work.top + work.bottom - h) / 2
        } else {
            // "go to the left" with no vertical: stay grounded.
            work.bottom - h
        };
        unsafe {
            SetWindowPos(hwnd, HWND_TOPMOST, x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
        }
        // Who is he now standing on? The front-most real window whose rect
        // overlaps where he landed.
        Ok(blocking_title(x, y, w, h))
    }
    #[cfg(not(windows))]
    {
        let _ = region;
        Err(())
    }
}

/// The front-most real window overlapping this rect, if any.
#[cfg(windows)]
fn blocking_title(x: i32, y: i32, w: i32, h: i32) -> Option<String> {
    use windows_sys::Win32::Foundation::{HWND, LPARAM, RECT};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowLongPtrW, GetWindowRect, GetWindowTextLengthW, GetWindowTextW,
        IsIconic, IsWindowVisible, GWL_EXSTYLE, WS_EX_TOOLWINDOW,
    };

    struct Hit {
        rect: (i32, i32, i32, i32),
        found: Option<String>,
    }

    unsafe extern "system" fn each(hwnd: HWND, lparam: LPARAM) -> i32 {
        let s = &mut *(lparam as *mut Hit);
        if IsWindowVisible(hwnd) == 0 || IsIconic(hwnd) != 0 {
            return 1;
        }
        if GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW != 0 {
            return 1;
        }
        let len = GetWindowTextLengthW(hwnd);
        if len <= 0 {
            return 1;
        }
        let mut r: RECT = std::mem::zeroed();
        if GetWindowRect(hwnd, &mut r) == 0 {
            return 1;
        }
        let (x, y, x2, y2) = s.rect;
        if r.left < x2 && r.right > x && r.top < y2 && r.bottom > y {
            let mut buf = vec![0u16; len as usize + 1];
            let n = GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
            let t = String::from_utf16_lossy(&buf[..n.max(0) as usize]);
            if t != "Deskfolk" {
                s.found = Some(t);
                return 0;
            }
        }
        1
    }

    let mut hit = Hit { rect: (x, y, x + w, y + h), found: None };
    unsafe {
        EnumWindows(Some(each), (&mut hit as *mut Hit) as LPARAM);
    }
    hit.found
}

/// Press keys down in order, release in reverse — a chord like Win+D.
#[cfg(windows)]
fn chord(vks: &[u16]) {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP,
        KEYBD_EVENT_FLAGS,
    };
    let mut events: Vec<INPUT> = Vec::with_capacity(vks.len() * 2);
    for (vk, flags) in vks
        .iter()
        .map(|vk| (*vk, 0 as KEYBD_EVENT_FLAGS))
        .chain(vks.iter().rev().map(|vk| (*vk, KEYEVENTF_KEYUP)))
    {
        let mut input: INPUT = unsafe { std::mem::zeroed() };
        input.r#type = INPUT_KEYBOARD;
        input.Anonymous = INPUT_0 {
            ki: KEYBDINPUT { wVk: vk, wScan: 0, dwFlags: flags, time: 0, dwExtraInfo: 0 },
        };
        events.push(input);
    }
    unsafe {
        SendInput(events.len() as u32, events.as_ptr(), std::mem::size_of::<INPUT>() as i32);
    }
}

#[cfg(not(windows))]
fn chord(_vks: &[u16]) {}

/// Focus the YouTube tab and press its own play/pause key ('k'). The only
/// transport that is *aimed* at the browser — the media key goes to whichever
/// player registered last.
pub fn youtube_toggle() -> bool {
    if !window_deed("youtube", WindowDeed::Focus) {
        return false;
    }
    // Give the window switch a beat to land before the key does.
    std::thread::sleep(std::time::Duration::from_millis(200));
    crate::music::press(0x4B); // 'K'
    true
}

/// Type text into the focused window, one UTF-16 unit at a time.
#[cfg(windows)]
fn type_text(text: &str) -> bool {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP,
        KEYEVENTF_UNICODE,
    };

    for ch in text.chars() {
        if ch == '\n' {
            crate::music::press(0x0D); // VK_RETURN
            continue;
        }
        let mut units = [0u16; 2];
        for unit in ch.encode_utf16(&mut units) {
            let mut events: [INPUT; 2] = unsafe { std::mem::zeroed() };
            for (i, flags) in [KEYEVENTF_UNICODE, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP]
                .into_iter()
                .enumerate()
            {
                events[i].r#type = INPUT_KEYBOARD;
                events[i].Anonymous = INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: 0,
                        wScan: *unit,
                        dwFlags: flags,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                };
            }
            unsafe {
                SendInput(events.len() as u32, events.as_ptr(), std::mem::size_of::<INPUT>() as i32);
            }
        }
        // A trickle, not a dump: some apps drop input that arrives faster
        // than a human could ever type.
        std::thread::sleep(std::time::Duration::from_millis(3));
    }
    true
}

#[cfg(not(windows))]
fn type_text(_text: &str) -> bool {
    false
}

/// Put text on the Windows clipboard as Unicode.
#[cfg(windows)]
fn set_clipboard(text: &str) -> bool {
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
    };
    use windows_sys::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};

    const CF_UNICODETEXT: u32 = 13;
    let mut wide: Vec<u16> = text.encode_utf16().collect();
    wide.push(0);

    unsafe {
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return false;
        }
        let ok = (|| {
            if EmptyClipboard() == 0 {
                return false;
            }
            let bytes = wide.len() * 2;
            let handle = GlobalAlloc(GMEM_MOVEABLE, bytes);
            if handle.is_null() {
                return false;
            }
            let ptr = GlobalLock(handle);
            if ptr.is_null() {
                return false;
            }
            std::ptr::copy_nonoverlapping(wide.as_ptr(), ptr as *mut u16, wide.len());
            GlobalUnlock(handle);
            // On success the clipboard owns the memory; do not free it.
            !SetClipboardData(CF_UNICODETEXT, handle as _).is_null()
        })();
        CloseClipboard();
        ok
    }
}

#[cfg(not(windows))]
fn set_clipboard(_text: &str) -> bool {
    false
}

fn spawn(exe: &str) -> bool {
    std::process::Command::new(exe).spawn().is_ok()
}

#[derive(Clone, Copy)]
enum WindowDeed {
    Focus,
    Minimize,
    Close,
}

/// Find the front-most real window whose title contains `name` and act on it.
#[cfg(windows)]
fn window_deed(name: &str, deed: WindowDeed) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        PostMessageW, SetForegroundWindow, ShowWindow, SW_MINIMIZE, SW_RESTORE, WM_CLOSE,
    };

    let want = name.trim().to_lowercase();
    if want.is_empty() {
        return false;
    }
    // "close this" targets whatever the user is working in right now.
    let hwnd = if matches!(want.as_str(), "focused" | "this" | "current") {
        use windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
        let h = unsafe { GetForegroundWindow() };
        (h as isize != 0).then_some(h)
    } else if want == "browser" {
        // No browser titles itself "browser": try the ones people run.
        ["chrome", "edge", "firefox", "brave", "opera", "vivaldi"]
            .iter()
            .find_map(|b| find_window(b))
    } else {
        find_window(&want)
    };
    let Some(hwnd) = hwnd else {
        tracing::info!("computer: no window matching {name:?}");
        return false;
    };
    unsafe {
        match deed {
            WindowDeed::Focus => {
                // Restore first: SetForegroundWindow on a minimized window
                // "succeeds" while leaving it in the taskbar.
                ShowWindow(hwnd, SW_RESTORE);
                SetForegroundWindow(hwnd) != 0
            }
            WindowDeed::Minimize => ShowWindow(hwnd, SW_MINIMIZE) != 0,
            WindowDeed::Close => PostMessageW(hwnd, WM_CLOSE, 0, 0) != 0,
        }
    }
}

/// Title-substring window lookup, front-most first, using the same "counts as
/// on the screen" rules as the ledges — except minimized windows are allowed,
/// because "switch to X" should un-minimize X.
#[cfg(windows)]
fn find_window(want_lower: &str) -> Option<windows_sys::Win32::Foundation::HWND> {
    use windows_sys::Win32::Foundation::{HWND, LPARAM};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowLongPtrW, GetWindowTextLengthW, GetWindowTextW,
        IsWindowVisible, GWL_EXSTYLE, WS_EX_TOOLWINDOW,
    };

    struct Search {
        want: String,
        found: Option<HWND>,
    }

    unsafe extern "system" fn each(hwnd: HWND, lparam: LPARAM) -> i32 {
        let s = &mut *(lparam as *mut Search);
        if IsWindowVisible(hwnd) == 0 {
            return 1;
        }
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        if ex & WS_EX_TOOLWINDOW != 0 {
            return 1;
        }
        let len = GetWindowTextLengthW(hwnd);
        if len <= 0 {
            return 1;
        }
        let mut buf = vec![0u16; len as usize + 1];
        let n = GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
        let title = String::from_utf16_lossy(&buf[..n.max(0) as usize]).to_lowercase();
        if title.contains(&s.want) {
            s.found = Some(hwnd);
            return 0; // front-most match wins; stop walking
        }
        1
    }

    let mut search = Search { want: want_lower.to_string(), found: None };
    unsafe {
        EnumWindows(Some(each), (&mut search as *mut Search) as LPARAM);
    }
    search.found
}

#[cfg(not(windows))]
fn window_deed(_name: &str, _deed: WindowDeed) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_site_names_resolve_to_their_real_urls() {
        assert_eq!(Deed::parse("open", "youtube"), Some(Deed::OpenUrl("https://www.youtube.com".into())));
        assert_eq!(Deed::parse("open", "GitHub"), Some(Deed::OpenUrl("https://github.com".into())));
    }

    #[test]
    fn a_bare_domain_gets_https_and_nothing_else_gets_a_scheme() {
        assert_eq!(
            Deed::parse("open", "openai.com"),
            Some(Deed::OpenUrl("https://openai.com".into()))
        );
        // A spoken phrase with no dot is a search, not a guessed domain.
        assert_eq!(
            Deed::parse("open", "that rust tutorial"),
            Some(Deed::Search("that rust tutorial".into()))
        );
    }

    #[test]
    fn hostile_schemes_never_survive() {
        // STT will not produce these, but the model might; neither may open.
        assert_eq!(url_for("javascript:alert(1)"), None, "no dot, no scheme pass");
        assert_eq!(url_for("file://c:/windows"), None);
        assert_eq!(url_for("http://insecure.example"), None, "https only");
        assert_eq!(
            url_for("https://ok.example"),
            Some("https://ok.example".into())
        );
    }

    #[test]
    fn open_a_known_app_name_is_the_app_not_a_site() {
        assert_eq!(Deed::parse("open", "notepad"), Some(Deed::OpenApp(App::Notepad)));
        assert_eq!(Deed::parse("open", "calc"), Some(Deed::OpenApp(App::Calculator)));
    }

    #[test]
    fn the_app_list_is_closed() {
        assert_eq!(App::parse("powershell"), None);
        assert_eq!(App::parse("cmd"), None);
        assert_eq!(App::parse("regedit"), None);
        assert_eq!(Deed::parse("open_app", "powershell"), None);
    }

    #[test]
    fn an_invented_verb_does_nothing_at_all() {
        assert_eq!(Deed::parse("format_disk", "c:"), None);
        assert_eq!(Deed::parse("", ""), None);
        assert_eq!(Deed::parse("close", ""), None, "close with no target is ignored");
    }

    #[test]
    fn window_deeds_carry_their_target() {
        assert_eq!(Deed::parse("focus", "discord"), Some(Deed::Focus("discord".into())));
        assert_eq!(Deed::parse("minimize", "chrome"), Some(Deed::Minimize("chrome".into())));
        assert_eq!(Deed::parse("close", "spotify"), Some(Deed::Close("spotify".into())));
    }

    #[test]
    fn descriptions_read_like_a_journal() {
        assert_eq!(Deed::Search("lofi".into()).describe(), "searched the web for \"lofi\"");
        assert_eq!(Deed::OpenApp(App::Settings).describe(), "opened settings");
    }
}
