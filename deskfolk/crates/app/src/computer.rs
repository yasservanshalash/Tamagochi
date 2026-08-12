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
}

impl App {
    fn parse(name: &str) -> Option<App> {
        Some(match name.trim().to_ascii_lowercase().as_str() {
            "notepad" => App::Notepad,
            "calculator" | "calc" => App::Calculator,
            "explorer" | "file explorer" | "files" => App::Explorer,
            "spotify" => App::Spotify,
            "settings" | "windows settings" => App::Settings,
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
            };
            ok
        }
    }
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
    let Some(hwnd) = find_window(&want) else {
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
