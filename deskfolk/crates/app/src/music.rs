//! Letting him work the music.
//!
//! "Skip this" and "turn it up" are the things a person actually says to
//! someone sitting by the speakers, and they need no account, no token and no
//! setup: Windows routes the media keys to whatever is playing, so this works
//! with Spotify, a browser tab, or anything else that registers for them.
//!
//! Deliberately *not* the Spotify Web API for these. That needs an OAuth app,
//! a refresh token, Premium, and an "active device" — four ways to be broken
//! on the day you want to skip a track. The Web API earns its complexity only
//! for the one thing keys cannot do, which is *choosing* a song; that is
//! [`Wish::Play`], and it is the only variant that needs credentials.
//!
//! Volume is the system volume rather than Spotify's own mixer slider. That is
//! what "turn it up" means to a person in a room, and per-application volume
//! would need an audio-session walk to find the right process.

/// Something he can do to the music.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Wish {
    Pause,
    Resume,
    /// Toggle, for when he has not been told which way.
    PlayPause,
    Next,
    Previous,
    Louder,
    Quieter,
    Mute,
    /// Find and play something. Needs the Web API; the keys cannot search.
    Play(String),
    /// Jump ahead this many seconds in the current track. Web API only.
    Forward(u32),
    /// Jump back this many seconds. Web API only.
    Back(u32),
    /// Jump to an absolute position, in seconds from the start. Web API only.
    SeekTo(u32),
    /// Put on a playlist matching a vibe ("egyptian", "thug"). Web API only.
    Playlist(String),
}

impl Wish {
    /// Parse what the mind asked for. Unknown verbs are ignored rather than
    /// guessed at — a model that invents "shuffle" should do nothing, not
    /// skip a track.
    pub fn parse(verb: &str, query: &str) -> Option<Wish> {
        let q = query.trim();
        Some(match verb.trim().to_ascii_lowercase().as_str() {
            "pause" | "stop" => Wish::Pause,
            "resume" | "unpause" => Wish::Resume,
            "play" if q.is_empty() => Wish::PlayPause,
            "play" => Wish::Play(q.to_string()),
            "play_pause" | "toggle" => Wish::PlayPause,
            "next" | "skip" => Wish::Next,
            // "back" with a number is a seek; bare "back" is the prev button.
            "previous" | "prev" | "back" => match q.parse::<u32>() {
                Ok(n) if n > 0 => Wish::Back(n.min(600)),
                _ => Wish::Previous,
            },
            "forward" | "ahead" => Wish::Forward(q.parse::<u32>().ok().filter(|n| *n > 0)?.min(600)),
            "seek_to" | "goto" => Wish::SeekTo(q.parse::<u32>().ok()?.min(3600)),
            "playlist" if !q.is_empty() => Wish::Playlist(q.to_string()),
            "louder" | "volume_up" | "up" => Wish::Louder,
            "quieter" | "volume_down" | "down" => Wish::Quieter,
            "mute" | "unmute" => Wish::Mute,
            _ => return None,
        })
    }

    /// How it reads in the journal.
    pub fn describe(&self) -> String {
        match self {
            Wish::Pause => "paused the music".into(),
            Wish::Resume => "started the music".into(),
            Wish::PlayPause => "hit play/pause".into(),
            Wish::Next => "skipped a track".into(),
            Wish::Previous => "went back a track".into(),
            Wish::Louder => "turned it up".into(),
            Wish::Quieter => "turned it down".into(),
            Wish::Mute => "muted it".into(),
            Wish::Play(q) => format!("put on {q:?}"),
            Wish::Forward(n) => format!("jumped ahead {n}s"),
            Wish::Back(n) => format!("jumped back {n}s"),
            Wish::SeekTo(n) => format!("jumped to {}:{:02}", n / 60, n % 60),
            Wish::Playlist(q) => format!("put on a {q} playlist"),
        }
    }

    /// The media key this maps to, and how many times to press it.
    ///
    /// Volume moves two percent per press on Windows, which is not enough to
    /// notice; a handful of presses is what "turn it up" means.
    fn key(&self) -> Option<(u16, usize)> {
        Some(match self {
            // There is no separate play and pause key — the hardware only ever
            // had a toggle — so all three land on the same one. Asking him to
            // pause what is already paused will start it, which is the same
            // thing that happens when a person hits the button.
            Wish::Pause | Wish::Resume | Wish::PlayPause => (VK_MEDIA_PLAY_PAUSE, 1),
            Wish::Next => (VK_MEDIA_NEXT_TRACK, 1),
            Wish::Previous => (VK_MEDIA_PREV_TRACK, 1),
            Wish::Louder => (VK_VOLUME_UP, 5),
            Wish::Quieter => (VK_VOLUME_DOWN, 5),
            Wish::Mute => (VK_VOLUME_MUTE, 1),
            Wish::Play(_) | Wish::Forward(_) | Wish::Back(_) | Wish::SeekTo(_)
            | Wish::Playlist(_) => return None,
        })
    }
}

const VK_VOLUME_MUTE: u16 = 0xAD;
const VK_VOLUME_DOWN: u16 = 0xAE;
const VK_VOLUME_UP: u16 = 0xAF;
const VK_MEDIA_NEXT_TRACK: u16 = 0xB0;
const VK_MEDIA_PREV_TRACK: u16 = 0xB1;
const VK_MEDIA_PLAY_PAUSE: u16 = 0xB3;

/// Carry out a wish. `false` means it needs something we do not have — today,
/// only searching for a track.
pub fn grant(wish: &Wish) -> bool {
    // "start the music" through the media key only works if a player is
    // already the active media session; a Spotify that has not played since it
    // opened ignores the key outright. With an account linked, resume goes
    // through the Web API, which can wake the device — the key stays as the
    // fallback if the API is having a day.
    if matches!(wish, Wish::Resume | Wish::PlayPause) && crate::spotify::connected() {
        std::thread::spawn(|| match crate::spotify::resume() {
            Ok(()) => tracing::info!("music: started playback via the Web API"),
            Err(e) => {
                tracing::warn!("music: API resume failed ({e}); pressing the key instead");
                press(VK_MEDIA_PLAY_PAUSE);
            }
        });
        tracing::info!("music: {}", wish.describe());
        return true;
    }
    match wish.key() {
        Some((vk, times)) => {
            for _ in 0..times {
                press(vk);
            }
            tracing::info!("music: {}", wish.describe());
            true
        }
        None => {
            // Wishes media keys cannot grant: searching, and seeking within a
            // track. With a linked account they go to the Web API — off the
            // caller's thread, because each is network round-trips.
            if crate::spotify::connected() {
                match wish {
                    Wish::Play(q) => {
                        let q = q.clone();
                        std::thread::spawn(move || match crate::spotify::play(&q) {
                            Ok(what) => tracing::info!("music: put on {what}"),
                            Err(e) => tracing::warn!("music: could not put on {q:?}: {e}"),
                        });
                        return true;
                    }
                    Wish::Playlist(q) => {
                        let q = q.clone();
                        std::thread::spawn(move || match crate::spotify::play_playlist(&q) {
                            Ok(what) => tracing::info!("music: put on playlist {what}"),
                            Err(e) => {
                                tracing::warn!("music: no {q:?} playlist ({e}); trying a track");
                                match crate::spotify::play(&q) {
                                    Ok(what) => tracing::info!("music: put on {what}"),
                                    Err(e) => tracing::warn!("music: could not put on {q:?}: {e}"),
                                }
                            }
                        });
                        return true;
                    }
                    Wish::Forward(n) | Wish::Back(n) => {
                        let delta = if matches!(wish, Wish::Forward(_)) {
                            *n as i64
                        } else {
                            -(*n as i64)
                        };
                        let what = wish.describe();
                        std::thread::spawn(move || match crate::spotify::seek_by(delta) {
                            Ok(()) => tracing::info!("music: {what}"),
                            Err(e) => tracing::warn!("music: could not seek: {e}"),
                        });
                        return true;
                    }
                    Wish::SeekTo(n) => {
                        let (n, what) = (*n, wish.describe());
                        std::thread::spawn(move || match crate::spotify::seek_to(n) {
                            Ok(()) => tracing::info!("music: {what}"),
                            Err(e) => tracing::warn!("music: could not seek: {e}"),
                        });
                        return true;
                    }
                    _ => {}
                }
            }
            tracing::info!("music: {} needs Spotify connected (his menu)", wish.describe());
            false
        }
    }
}

#[cfg(windows)]
fn press(vk: u16) {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, KEYBD_EVENT_FLAGS,
    };
    // Down then up. A key that is never released leaves the volume climbing
    // for as long as the process lives.
    let mut events: [INPUT; 2] = unsafe { std::mem::zeroed() };
    for (i, flags) in [0 as KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP].into_iter().enumerate() {
        events[i].r#type = INPUT_KEYBOARD;
        events[i].Anonymous = INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        };
    }
    unsafe {
        SendInput(
            events.len() as u32,
            events.as_ptr(),
            std::mem::size_of::<INPUT>() as i32,
        );
    }
}

#[cfg(not(windows))]
fn press(_vk: u16) {}

/// What is playing, as far as we can tell without an account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NowPlaying {
    pub artist: String,
    pub track: String,
}

/// Spotify writes `Artist - Track` into its window title while playing, and
/// falls back to a bare `Spotify` or `Spotify Premium` when it is not.
///
/// Not elegant, and the reason it is here anyway: the alternative is the Web
/// API, which needs an OAuth app, a refresh token and Premium before it can
/// tell you the name of a song. This needs nothing and is right often enough
/// to put on a widget.
pub fn parse_title(title: &str) -> Option<NowPlaying> {
    let t = title.trim();
    if t.is_empty() || t.eq_ignore_ascii_case("spotify") {
        return None;
    }
    // "Spotify Premium" / "Spotify Free" are the idle titles, not a band.
    if t.len() < 32 && t.to_ascii_lowercase().starts_with("spotify") {
        return None;
    }
    // Advertisements report themselves, and are not worth showing as music.
    if t.eq_ignore_ascii_case("advertisement") {
        return None;
    }
    // The separator is a plain hyphen surrounded by spaces. Split once, from
    // the left: plenty of track names contain a dash, far fewer artists do.
    let (artist, track) = t.split_once(" - ")?;
    let (artist, track) = (artist.trim(), track.trim());
    if artist.is_empty() || track.is_empty() {
        return None;
    }
    Some(NowPlaying { artist: artist.into(), track: track.into() })
}

/// Read the Spotify desktop window's title, if it is running.
#[cfg(windows)]
pub fn now_playing() -> Option<NowPlaying> {
    use windows_sys::Win32::Foundation::{HWND, LPARAM};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetClassNameW, GetWindowTextLengthW, GetWindowTextW, IsWindowVisible,
    };

    unsafe extern "system" fn each(hwnd: HWND, lparam: LPARAM) -> i32 {
        let out = &mut *(lparam as *mut Option<String>);
        if IsWindowVisible(hwnd) == 0 {
            return 1;
        }
        // Spotify's main window class. Checked rather than matching on the
        // title, which is exactly the thing that changes every track.
        let mut class = [0u16; 64];
        let n = GetClassNameW(hwnd, class.as_mut_ptr(), class.len() as i32);
        if String::from_utf16_lossy(&class[..n.max(0) as usize]) != "Chrome_WidgetWin_0" {
            return 1;
        }
        let len = GetWindowTextLengthW(hwnd);
        if len <= 0 {
            return 1;
        }
        let mut buf = vec![0u16; len as usize + 1];
        let got = GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
        let title = String::from_utf16_lossy(&buf[..got.max(0) as usize]);
        if !title.trim().is_empty() {
            *out = Some(title);
            return 0;
        }
        1
    }

    let mut title: Option<String> = None;
    unsafe {
        EnumWindows(Some(each), (&mut title as *mut Option<String>) as LPARAM);
    }
    parse_title(&title?)
}

#[cfg(not(windows))]
pub fn now_playing() -> Option<NowPlaying> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_words_a_person_would_use_all_land_somewhere() {
        for (verb, want) in [
            ("skip", Wish::Next),
            ("next", Wish::Next),
            ("back", Wish::Previous),
            ("louder", Wish::Louder),
            ("volume_up", Wish::Louder),
            ("quieter", Wish::Quieter),
            ("mute", Wish::Mute),
            ("pause", Wish::Pause),
            ("stop", Wish::Pause),
            ("resume", Wish::Resume),
        ] {
            assert_eq!(Wish::parse(verb, ""), Some(want), "{verb}");
        }
    }

    #[test]
    fn play_means_two_different_things() {
        // Bare "play" is the button on a remote; "play something" is a search.
        assert_eq!(Wish::parse("play", ""), Some(Wish::PlayPause));
        assert_eq!(Wish::parse("play", "   "), Some(Wish::PlayPause));
        assert_eq!(
            Wish::parse("play", "Madvillainy"),
            Some(Wish::Play("Madvillainy".into()))
        );
    }

    #[test]
    fn an_invented_verb_does_nothing_at_all() {
        // Guessing is worse than ignoring: a model that says "shuffle" must
        // not have that resolve to skipping the track he is enjoying.
        assert_eq!(Wish::parse("shuffle", ""), None);
        assert_eq!(Wish::parse("", ""), None);
        assert_eq!(Wish::parse("delete_everything", ""), None);
    }

    #[test]
    fn case_and_spacing_do_not_matter() {
        assert_eq!(Wish::parse("  NEXT  ", ""), Some(Wish::Next));
        assert_eq!(Wish::parse("Volume_Up", ""), Some(Wish::Louder));
    }

    #[test]
    fn volume_moves_by_enough_to_notice() {
        // One press is two percent on Windows, which nobody would hear.
        let (_, times) = Wish::Louder.key().expect("a key");
        assert!(times >= 4, "only {times} presses");
        assert_eq!(Wish::Next.key().expect("a key").1, 1, "skipping is once");
    }

    #[test]
    fn choosing_a_song_is_the_only_thing_keys_cannot_do() {
        assert!(Wish::Play("anything".into()).key().is_none());
        for w in [Wish::Next, Wish::Previous, Wish::Louder, Wish::Quieter, Wish::Mute,
                  Wish::Pause, Wish::Resume, Wish::PlayPause] {
            assert!(w.key().is_some(), "{w:?} should work with no account");
        }
    }
}

#[cfg(test)]
mod title_tests {
    use super::*;

    fn np(a: &str, t: &str) -> Option<NowPlaying> {
        Some(NowPlaying { artist: a.into(), track: t.into() })
    }

    #[test]
    fn a_playing_title_gives_artist_and_track() {
        assert_eq!(parse_title("Nujabes - Feather"), np("Nujabes", "Feather"));
        assert_eq!(parse_title("  MF DOOM - Doomsday  "), np("MF DOOM", "Doomsday"));
    }

    #[test]
    fn the_idle_titles_are_not_a_band_called_spotify() {
        for t in ["Spotify", "spotify", "Spotify Premium", "Spotify Free", "  "] {
            assert_eq!(parse_title(t), None, "{t:?} is not a song");
        }
    }

    #[test]
    fn a_dash_in_the_track_name_survives() {
        // Split from the left: track names carry dashes far more often than
        // artist names do, so the first separator is the real one.
        assert_eq!(
            parse_title("Radiohead - Paranoid Android - Remastered"),
            np("Radiohead", "Paranoid Android - Remastered")
        );
    }

    #[test]
    fn a_band_whose_name_starts_with_spotify_still_plays() {
        // The idle-title guard is length-limited precisely so it cannot eat a
        // real track by a long-named artist.
        assert_eq!(
            parse_title("Spotify Sessions Orchestra - A Very Long Song Title"),
            np("Spotify Sessions Orchestra", "A Very Long Song Title")
        );
    }

    #[test]
    fn adverts_and_malformed_titles_show_nothing() {
        assert_eq!(parse_title("Advertisement"), None);
        assert_eq!(parse_title("no separator here"), None);
        assert_eq!(parse_title(" - Feather"), None, "no artist");
        assert_eq!(parse_title("Nujabes - "), None, "no track");
    }
}
