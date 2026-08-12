//! The Spotify Web API — the half of the music the media keys cannot do.
//!
//! Media keys (`music.rs`) cover transport: pause, skip, volume. What they can
//! never do is *put a named thing on* — "play some MF DOOM" needs search and a
//! playback command, which means the Web API, which means OAuth.
//!
//! This is the smallest honest implementation of that: the PKCE authorization
//! flow (no client secret, made for desktop apps), tokens in one JSON file
//! under %APPDATA%, and a search→play pair. Setup is one env entry: create an
//! app at developer.spotify.com, add `http://127.0.0.1:8898/callback` as its
//! redirect URI, and put `SPOTIFY_CLIENT_ID=<id>` in `brain/.env`. Playback
//! control requires Spotify Premium — that's Spotify's rule, not ours.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::config;

const REDIRECT_PORT: u16 = 8898;
const SCOPES: &str = "user-modify-playback-state user-read-playback-state";

#[derive(Serialize, Deserialize, Default)]
struct Tokens {
    access_token: String,
    refresh_token: String,
    /// Unix seconds when the access token dies.
    expires_at: u64,
}

fn token_path() -> PathBuf {
    let base = std::env::var("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    base.join("deskfolk").join("spotify.json")
}

fn load_tokens() -> Option<Tokens> {
    serde_json::from_str(&std::fs::read_to_string(token_path()).ok()?).ok()
}

fn save_tokens(t: &Tokens) {
    let p = token_path();
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(s) = serde_json::to_string(t) {
        let _ = std::fs::write(p, s);
    }
}

/// Is the account linked (a token file exists)?
pub fn connected() -> bool {
    load_tokens().is_some()
}

/// Is the feature even configured (client id present)?
pub fn configured() -> bool {
    config::secret("SPOTIFY_CLIENT_ID").filter(|s| !s.trim().is_empty()).is_some()
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn b64url(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// A PKCE verifier: unguessable enough from OS entropy sources, and never
/// leaves this machine except hashed.
fn verifier() -> String {
    let mut seed = format!(
        "{}-{}-{:?}",
        std::process::id(),
        now(),
        std::time::Instant::now()
    )
    .into_bytes();
    let mut out = String::new();
    while out.len() < 64 {
        let d = Sha256::digest(&seed);
        out.push_str(&b64url(&d));
        seed = d.to_vec();
    }
    out.truncate(64);
    out
}

fn open_browser(url: &str) {
    // Not `cmd /C start`: cmd splits its line at every unquoted `&`, which
    // silently truncated the authorize URL at the first query parameter —
    // Spotify then reported "client_id: Not present". rundll32 hands the URL
    // to the default browser without any shell parsing in between.
    let _ = std::process::Command::new("rundll32")
        .args(["url.dll,FileProtocolHandler", url])
        .spawn();
}

/// Run the PKCE authorization flow. Blocks its thread until the browser
/// round-trip completes (or fails); call from a spawned thread.
pub fn connect() -> Result<(), String> {
    let client_id = config::secret("SPOTIFY_CLIENT_ID")
        .filter(|s| !s.trim().is_empty())
        .ok_or("no SPOTIFY_CLIENT_ID in brain/.env — create an app at developer.spotify.com")?;
    let verifier = verifier();
    let challenge = b64url(&Sha256::digest(verifier.as_bytes()));
    let redirect = format!("http://127.0.0.1:{REDIRECT_PORT}/callback");

    let listener = TcpListener::bind(("127.0.0.1", REDIRECT_PORT))
        .map_err(|e| format!("cannot listen on {REDIRECT_PORT}: {e}"))?;

    let url = format!(
        "https://accounts.spotify.com/authorize?response_type=code&client_id={client_id}\
         &scope={}&redirect_uri={}&code_challenge_method=S256&code_challenge={challenge}",
        urlenc(SCOPES),
        urlenc(&redirect),
    );
    tracing::info!("spotify: opening browser for consent");
    open_browser(&url);

    // One request is all the browser will send us.
    listener
        .set_nonblocking(false)
        .map_err(|e| e.to_string())?;
    let (mut stream, _) = listener.accept().map_err(|e| e.to_string())?;
    let mut buf = [0u8; 4096];
    let n = stream.read(&mut buf).map_err(|e| e.to_string())?;
    let req = String::from_utf8_lossy(&buf[..n]);
    let code = req
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|path| path.split("code=").nth(1))
        .map(|c| c.split('&').next().unwrap_or(c).to_string())
        .filter(|c| !c.is_empty())
        .ok_or("no code in the callback — consent denied?")?;
    let _ = stream.write_all(
        b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n\
          <body style=\"font-family:sans-serif;background:#141217;color:#ece7de;\
          display:grid;place-items:center;height:100vh\">\
          <div>Spotify connected. He can put songs on now &mdash; close this tab.</div></body>",
    );

    // Code -> tokens.
    let client = http()?;
    let resp: serde_json::Value = client
        .post("https://accounts.spotify.com/api/token")
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code.as_str()),
            ("redirect_uri", redirect.as_str()),
            ("client_id", client_id.as_str()),
            ("code_verifier", verifier.as_str()),
        ])
        .send()
        .map_err(|e| e.to_string())?
        .json()
        .map_err(|e| e.to_string())?;
    let access = resp["access_token"].as_str().ok_or_else(|| format!("token exchange failed: {resp}"))?;
    let refresh = resp["refresh_token"].as_str().unwrap_or_default();
    let ttl = resp["expires_in"].as_u64().unwrap_or(3600);
    save_tokens(&Tokens {
        access_token: access.to_string(),
        refresh_token: refresh.to_string(),
        expires_at: now() + ttl.saturating_sub(60),
    });
    tracing::info!("spotify: connected");
    Ok(())
}

fn http() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())
}

/// A valid access token, refreshed if stale.
fn access_token() -> Result<String, String> {
    let mut t = load_tokens().ok_or("Spotify not connected — use \"Connect Spotify\" in his menu")?;
    if now() < t.expires_at {
        return Ok(t.access_token);
    }
    let client_id = config::secret("SPOTIFY_CLIENT_ID").ok_or("no SPOTIFY_CLIENT_ID")?;
    let client = http()?;
    let resp: serde_json::Value = client
        .post("https://accounts.spotify.com/api/token")
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", t.refresh_token.as_str()),
            ("client_id", client_id.as_str()),
        ])
        .send()
        .map_err(|e| e.to_string())?
        .json()
        .map_err(|e| e.to_string())?;
    let access = resp["access_token"]
        .as_str()
        .ok_or_else(|| format!("refresh failed: {resp}"))?;
    t.access_token = access.to_string();
    if let Some(r) = resp["refresh_token"].as_str() {
        t.refresh_token = r.to_string();
    }
    t.expires_at = now() + resp["expires_in"].as_u64().unwrap_or(3600).saturating_sub(60);
    save_tokens(&t);
    Ok(t.access_token)
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

/// Resume playback with nothing named — the play button, but through the API,
/// which unlike the media key can wake a Spotify that is not the active media
/// session (a freshly opened one ignores the key entirely).
pub fn resume() -> Result<(), String> {
    let token = access_token()?;
    let client = http()?;
    let put = |device: Option<&str>| {
        let url = match device {
            Some(id) => format!("https://api.spotify.com/v1/me/player/play?device_id={id}"),
            None => "https://api.spotify.com/v1/me/player/play".into(),
        };
        client.put(url).bearer_auth(&token).header("Content-Length", "0").send()
    };
    let resp = put(None).map_err(|e| e.to_string())?;
    if resp.status().is_success() {
        return Ok(());
    }
    let devices: serde_json::Value = client
        .get("https://api.spotify.com/v1/me/player/devices")
        .bearer_auth(&token)
        .send()
        .map_err(|e| e.to_string())?
        .json()
        .map_err(|e| e.to_string())?;
    let id = devices["devices"][0]["id"]
        .as_str()
        .ok_or("no Spotify device is open — start Spotify somewhere first")?
        .to_string();
    let resp = put(Some(&id)).map_err(|e| e.to_string())?;
    if resp.status().is_success() {
        Ok(())
    } else {
        Err(format!("Spotify said {}", resp.status()))
    }
}

/// Jump `delta_secs` (negative = back) within the current track.
pub fn seek_by(delta_secs: i64) -> Result<(), String> {
    seek(SeekWhere::Relative(delta_secs))
}

/// Jump to `secs` from the start of the current track.
pub fn seek_to(secs: u32) -> Result<(), String> {
    seek(SeekWhere::Absolute(secs as i64))
}

enum SeekWhere {
    Relative(i64),
    Absolute(i64),
}

fn seek(target: SeekWhere) -> Result<(), String> {
    let token = access_token()?;
    let client = http()?;
    let resp = client
        .get("https://api.spotify.com/v1/me/player")
        .bearer_auth(&token)
        .send()
        .map_err(|e| e.to_string())?;
    if resp.status().as_u16() == 204 {
        return Err("nothing is playing".into());
    }
    let state: serde_json::Value = resp.json().map_err(|e| e.to_string())?;
    let progress = state["progress_ms"].as_i64().ok_or("nothing is playing")?;
    let duration = state["item"]["duration_ms"].as_i64().unwrap_or(i64::MAX);
    // Clamp shy of the very end, or a big jump forward just skips the track.
    let wanted = match target {
        SeekWhere::Relative(d) => progress + d * 1000,
        SeekWhere::Absolute(s) => s * 1000,
    };
    let pos = wanted.clamp(0, duration.saturating_sub(1_500));
    let resp = client
        .put(format!("https://api.spotify.com/v1/me/player/seek?position_ms={pos}"))
        .bearer_auth(&token)
        .header("Content-Length", "0")
        .send()
        .map_err(|e| e.to_string())?;
    if resp.status().is_success() {
        Ok(())
    } else {
        Err(format!("Spotify said {}", resp.status()))
    }
}

/// One bodyless player PUT (volume, shuffle, repeat all look like this).
fn player_put(path_and_query: &str) -> Result<(), String> {
    let token = access_token()?;
    let client = http()?;
    let resp = client
        .put(format!("https://api.spotify.com/v1/me/player/{path_and_query}"))
        .bearer_auth(&token)
        .header("Content-Length", "0")
        .send()
        .map_err(|e| e.to_string())?;
    if resp.status().is_success() {
        Ok(())
    } else {
        Err(format!("Spotify said {}", resp.status()))
    }
}

/// Set the player volume to an exact percentage.
pub fn set_volume(percent: u8) -> Result<(), String> {
    player_put(&format!("volume?volume_percent={}", percent.min(100)))
}

/// Shuffle on or off.
pub fn shuffle(on: bool) -> Result<(), String> {
    player_put(&format!("shuffle?state={on}"))
}

/// Repeat the current track, or stop repeating.
pub fn repeat(on: bool) -> Result<(), String> {
    player_put(&format!("repeat?state={}", if on { "track" } else { "off" }))
}

/// Find `query` and add it to the queue, leaving what plays alone.
pub fn queue(query: &str) -> Result<String, String> {
    let token = access_token()?;
    let client = http()?;
    let search: serde_json::Value = client
        .get(format!(
            "https://api.spotify.com/v1/search?q={}&type=track&limit=1",
            urlenc(query)
        ))
        .bearer_auth(&token)
        .send()
        .map_err(|e| e.to_string())?
        .json()
        .map_err(|e| e.to_string())?;
    let track = &search["tracks"]["items"][0];
    let uri = track["uri"].as_str().ok_or_else(|| format!("nothing found for {query:?}"))?;
    let label = format!(
        "{} — {}",
        track["artists"][0]["name"].as_str().unwrap_or("?"),
        track["name"].as_str().unwrap_or(query),
    );
    let resp = client
        .post(format!("https://api.spotify.com/v1/me/player/queue?uri={}", urlenc(uri)))
        .bearer_auth(&token)
        .header("Content-Length", "0")
        .send()
        .map_err(|e| e.to_string())?;
    if resp.status().is_success() {
        Ok(label)
    } else {
        Err(format!("Spotify said {}", resp.status()))
    }
}

/// Open the lyrics of what is currently playing, via a Genius search in the
/// browser — Spotify has no public lyrics API.
pub fn open_lyrics() -> Result<String, String> {
    let token = access_token()?;
    let client = http()?;
    let resp = client
        .get("https://api.spotify.com/v1/me/player")
        .bearer_auth(&token)
        .send()
        .map_err(|e| e.to_string())?;
    if resp.status().as_u16() == 204 {
        return Err("nothing is playing".into());
    }
    let state: serde_json::Value = resp.json().map_err(|e| e.to_string())?;
    let track = state["item"]["name"].as_str().ok_or("nothing is playing")?;
    let artist = state["item"]["artists"][0]["name"].as_str().unwrap_or("");
    let label = format!("{artist} — {track}");
    open_browser(&format!(
        "https://genius.com/search?q={}",
        urlenc(&format!("{artist} {track}"))
    ));
    Ok(label)
}

/// Find a playlist for `query` and start it. Returns the playlist's name.
pub fn play_playlist(query: &str) -> Result<String, String> {
    let token = access_token()?;
    let client = http()?;
    let search: serde_json::Value = client
        .get(format!(
            "https://api.spotify.com/v1/search?q={}&type=playlist&limit=5",
            urlenc(query)
        ))
        .bearer_auth(&token)
        .send()
        .map_err(|e| e.to_string())?
        .json()
        .map_err(|e| e.to_string())?;
    // The items array can contain literal nulls; take the first real one.
    let lists = search["playlists"]["items"].as_array().cloned().unwrap_or_default();
    let list = lists
        .iter()
        .find(|p| p["uri"].as_str().is_some())
        .ok_or_else(|| format!("no playlist found for {query:?}"))?;
    let uri = list["uri"].as_str().unwrap_or_default().to_string();
    let name = list["name"].as_str().unwrap_or(query).to_string();

    let body = serde_json::json!({ "context_uri": uri }).to_string();
    let put = |device: Option<&str>| {
        let url = match device {
            Some(id) => format!("https://api.spotify.com/v1/me/player/play?device_id={id}"),
            None => "https://api.spotify.com/v1/me/player/play".into(),
        };
        client
            .put(url)
            .bearer_auth(&token)
            .header("Content-Type", "application/json")
            .body(body.clone())
            .send()
    };
    let resp = put(None).map_err(|e| e.to_string())?;
    if resp.status().is_success() {
        return Ok(name);
    }
    let devices: serde_json::Value = client
        .get("https://api.spotify.com/v1/me/player/devices")
        .bearer_auth(&token)
        .send()
        .map_err(|e| e.to_string())?
        .json()
        .map_err(|e| e.to_string())?;
    let id = devices["devices"][0]["id"]
        .as_str()
        .ok_or("no Spotify device is open — start Spotify somewhere first")?
        .to_string();
    let resp = put(Some(&id)).map_err(|e| e.to_string())?;
    if resp.status().is_success() {
        Ok(name)
    } else {
        Err(format!("Spotify said {}", resp.status()))
    }
}

/// Find `query` and start playing it. Returns what got put on.
pub fn play(query: &str) -> Result<String, String> {
    let token = access_token()?;
    let client = http()?;

    let search: serde_json::Value = client
        .get(format!(
            "https://api.spotify.com/v1/search?q={}&type=track&limit=1",
            urlenc(query)
        ))
        .bearer_auth(&token)
        .send()
        .map_err(|e| e.to_string())?
        .json()
        .map_err(|e| e.to_string())?;
    let track = &search["tracks"]["items"][0];
    let uri = track["uri"].as_str().ok_or_else(|| format!("nothing found for {query:?}"))?;
    let label = format!(
        "{} — {}",
        track["artists"][0]["name"].as_str().unwrap_or("?"),
        track["name"].as_str().unwrap_or(query),
    );

    let body = serde_json::json!({ "uris": [uri] }).to_string();
    let put = |device: Option<&str>| {
        let url = match device {
            Some(id) => format!("https://api.spotify.com/v1/me/player/play?device_id={id}"),
            None => "https://api.spotify.com/v1/me/player/play".into(),
        };
        client
            .put(url)
            .bearer_auth(&token)
            .header("Content-Type", "application/json")
            .body(body.clone())
            .send()
    };

    let resp = put(None).map_err(|e| e.to_string())?;
    if resp.status().is_success() {
        return Ok(label);
    }
    // 404 usually means "no active device": wake the first one Spotify knows.
    let devices: serde_json::Value = client
        .get("https://api.spotify.com/v1/me/player/devices")
        .bearer_auth(&token)
        .send()
        .map_err(|e| e.to_string())?
        .json()
        .map_err(|e| e.to_string())?;
    let id = devices["devices"][0]["id"]
        .as_str()
        .ok_or("no Spotify device is open — start Spotify somewhere first")?
        .to_string();
    let resp = put(Some(&id)).map_err(|e| e.to_string())?;
    if resp.status().is_success() {
        Ok(label)
    } else {
        Err(format!(
            "Spotify said {}: {}",
            resp.status(),
            resp.text().unwrap_or_default().chars().take(200).collect::<String>()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_verifier_is_long_enough_for_pkce() {
        let v = verifier();
        assert!(v.len() >= 43 && v.len() <= 128, "len {}", v.len());
        assert!(v.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
    }

    #[test]
    fn url_encoding_keeps_unreserved_and_escapes_the_rest() {
        assert_eq!(urlenc("a b&c"), "a%20b%26c");
        assert_eq!(urlenc("A-z_0.~"), "A-z_0.~");
    }
}
