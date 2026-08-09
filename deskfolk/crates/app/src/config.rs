//! Where the mind's configuration comes from.
//!
//! Until the Control Center can store settings, providers are resolved from
//! the environment plus any `.env` files already on disk. The alpha's
//! `brain/.env` is read deliberately: the user's keys already live there, and
//! asking them to copy secrets into a second file just to try the new build
//! would be a poor trade.
//!
//! Nothing here ever logs a key.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use deskfolk_ai::ProviderConfig;

/// Default OpenRouter model.
///
/// Chosen for this character rather than in general: the alpha ran an
/// uncensored local model on purpose (`mistral-heretic`, `PET_KEEP_IT_REAL=1`)
/// because the persona is blunt and ordinary assistant models kept breaking
/// character. This is the closest hosted equivalent. Override with
/// `DESKFOLK_MODEL`.
pub const DEFAULT_OPENROUTER_MODEL: &str =
    "cognitivecomputations/dolphin-mistral-24b-venice-edition";

const OPENROUTER_BASE: &str = "https://openrouter.ai/api/v1";
const GROQ_BASE: &str = "https://api.groq.com/openai/v1";

/// Resolve the provider from the user's saved settings, falling back to the
/// environment.
///
/// This is the basic/advanced tier: **basic** always runs the bundled local
/// brain (free, private, no key) via the sidecar; **advanced** uses whatever
/// provider the user configured in the Control Center. If advanced is selected
/// but nothing is configured yet, we fall back to environment auto-detection so
/// he still talks.
pub fn provider_from_settings(brain: &crate::settings::Brain) -> (ProviderConfig, String) {
    use crate::settings::Tier;
    match brain.tier {
        Tier::Basic => (
            ProviderConfig::Sidecar { base_url: "http://127.0.0.1:8087".into() },
            "basic tier: bundled local brain".into(),
        ),
        Tier::Advanced => match &brain.provider {
            Some(p) => (p.clone(), "advanced tier: your own provider".into()),
            None => resolve_provider(),
        },
    }
}

/// Resolve the provider, and return a human-readable reason for the choice so
/// startup logs explain themselves.
pub fn resolve_provider() -> (ProviderConfig, String) {
    let env = load_env();
    let get = |k: &str| -> Option<String> {
        env.get(k).map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
    };
    let model_override = get("DESKFOLK_MODEL");

    // An explicit choice always wins over auto-detection.
    if let Some(choice) = get("DESKFOLK_PROVIDER") {
        match choice.to_ascii_lowercase().as_str() {
            "anthropic" => {
                return (
                    ProviderConfig::Anthropic {
                        api_key: get("ANTHROPIC_API_KEY").unwrap_or_default(),
                        model: model_override.unwrap_or_else(deskfolk_ai::anthropic_default_model),
                    },
                    "DESKFOLK_PROVIDER=anthropic".into(),
                );
            }
            "openrouter" => return (openrouter(&get, model_override), "DESKFOLK_PROVIDER=openrouter".into()),
            "groq" => return (groq(&get, model_override), "DESKFOLK_PROVIDER=groq".into()),
            "ollama" => return (ollama(&get, model_override), "DESKFOLK_PROVIDER=ollama".into()),
            "sidecar" => {
                return (
                    ProviderConfig::Sidecar {
                        base_url: get("DESKFOLK_SIDECAR_URL")
                            .unwrap_or_else(|| "http://127.0.0.1:8087".into()),
                    },
                    "DESKFOLK_PROVIDER=sidecar".into(),
                );
            }
            other => tracing::warn!("unknown DESKFOLK_PROVIDER '{other}', auto-detecting instead"),
        }
    }

    // Local first. If the alpha's brain is up it already routes to the local
    // model *and* owns STT and TTS, so preferring it keeps voice working and
    // keeps conversation off the network entirely. Cloud keys are the
    // fallback, not the default.
    if sidecar_is_up(&get) {
        return (
            ProviderConfig::Sidecar { base_url: sidecar_url(&get) },
            "local brain is running on 8087".into(),
        );
    }

    // Auto-detect, cheapest-to-set-up first.
    if let Some(key) = get("ANTHROPIC_API_KEY") {
        return (
            ProviderConfig::Anthropic {
                api_key: key,
                model: model_override.unwrap_or_else(deskfolk_ai::anthropic_default_model),
            },
            "found ANTHROPIC_API_KEY".into(),
        );
    }
    if get("OPENROUTER_API_KEY").is_some() {
        return (openrouter(&get, model_override), "found OPENROUTER_API_KEY".into());
    }
    if get("GROQ_API_KEY").is_some() {
        return (groq(&get, model_override), "found GROQ_API_KEY".into());
    }
    if get("PET_API_BASE").is_some() {
        return (ollama(&get, model_override), "found PET_API_BASE".into());
    }
    (
        ProviderConfig::Sidecar { base_url: "http://127.0.0.1:8087".into() },
        "no API key found, falling back to the local sidecar".into(),
    )
}

fn sidecar_url(get: &impl Fn(&str) -> Option<String>) -> String {
    get("DESKFOLK_SIDECAR_URL").unwrap_or_else(|| "http://127.0.0.1:8087".into())
}

/// Where speech comes from, or `None` if the user turned the voice off.
///
/// Deliberately independent of the provider: the mind can be a cloud model
/// while the voice still comes from the local brain, which is the normal
/// setup here — Orpheus lives behind `brain/.env`, not behind whichever LLM
/// happens to be answering.
pub fn resolve_voice() -> Option<String> {
    let env = load_env();
    let get = |k: &str| -> Option<String> {
        env.get(k).map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
    };
    voice_url(&get)
}

fn voice_url(get: &impl Fn(&str) -> Option<String>) -> Option<String> {
    if let Some(v) = get("DESKFOLK_VOICE") {
        if matches!(v.to_ascii_lowercase().as_str(), "off" | "0" | "false" | "none") {
            return None;
        }
    }
    Some(
        get("DESKFOLK_VOICE_URL")
            // The alpha pointed its desktop pet at the brain with PET_BRAIN;
            // reusing it means an existing setup needs no new configuration.
            .or_else(|| get("PET_BRAIN"))
            .unwrap_or_else(|| sidecar_url(get))
            .trim_end_matches('/')
            .to_string(),
    )
}

/// A quick, blocking liveness probe. Runs once at startup, before the async
/// runtime exists, so a plain TCP connect with a short timeout beats pulling
/// in a blocking HTTP client.
fn sidecar_is_up(get: &impl Fn(&str) -> Option<String>) -> bool {
    use std::net::{TcpStream, ToSocketAddrs};
    use std::time::Duration;

    let url = sidecar_url(get);
    let hostport = url
        .trim_start_matches("http://")
        .trim_start_matches("https://")
        .trim_end_matches('/');
    let Ok(mut addrs) = hostport.to_socket_addrs() else { return false };
    addrs.any(|addr| TcpStream::connect_timeout(&addr, Duration::from_millis(250)).is_ok())
}

fn openrouter(
    get: &impl Fn(&str) -> Option<String>,
    model: Option<String>,
) -> ProviderConfig {
    ProviderConfig::OpenAiCompat {
        base_url: get("OPENROUTER_BASE").unwrap_or_else(|| OPENROUTER_BASE.to_string()),
        api_key: get("OPENROUTER_API_KEY").unwrap_or_default(),
        model: model.unwrap_or_else(|| DEFAULT_OPENROUTER_MODEL.to_string()),
    }
}

fn groq(get: &impl Fn(&str) -> Option<String>, model: Option<String>) -> ProviderConfig {
    ProviderConfig::OpenAiCompat {
        base_url: GROQ_BASE.to_string(),
        api_key: get("GROQ_API_KEY").unwrap_or_default(),
        model: model.unwrap_or_else(|| "llama-3.3-70b-versatile".to_string()),
    }
}

fn ollama(get: &impl Fn(&str) -> Option<String>, model: Option<String>) -> ProviderConfig {
    ProviderConfig::OpenAiCompat {
        // Reuses the alpha's own variables so an existing local setup works
        // with no new configuration at all.
        base_url: get("PET_API_BASE").unwrap_or_else(|| "http://localhost:11434/v1".into()),
        api_key: get("OLLAMA_API_KEY").unwrap_or_default(),
        model: model
            .or_else(|| get("PET_MODEL"))
            .unwrap_or_else(|| "mistral".to_string()),
    }
}

/// Process environment overlaid on any `.env` files we can find. Real env vars
/// win, so a shell override always beats a file.
/// Look up a value from the merged `.env` files (real environment wins), e.g.
/// the shared `OPENROUTER_API_KEY` in `brain/.env`. Dev tools that need a
/// secret at command time use this rather than re-parsing `.env` themselves.
pub fn secret(key: &str) -> Option<String> {
    load_env().get(key).cloned()
}

fn load_env() -> BTreeMap<String, String> {
    let mut merged = BTreeMap::new();
    for path in env_file_candidates() {
        if let Ok(text) = std::fs::read_to_string(&path) {
            let n = merged.len();
            parse_env_into(&text, &mut merged);
            tracing::debug!("read {} entries from {}", merged.len() - n, path.display());
        }
    }
    for (k, v) in std::env::vars() {
        merged.insert(k, v);
    }
    merged
}

fn env_file_candidates() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    vec![
        // The alpha's keys — deliberately shared, not duplicated.
        root.join("../brain/.env"),
        root.join(".env"),
    ]
}

/// Minimal `KEY=value` parser: comments, blank lines, `export` prefixes and
/// surrounding quotes. Enough for the files this reads, and one less
/// dependency in a binary that ships to users.
fn parse_env_into(text: &str, out: &mut BTreeMap<String, String>) {
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((key, value)) = line.split_once('=') else { continue };
        let key = key.trim();
        if key.is_empty() {
            continue;
        }
        let mut value = value.trim();
        // Strip a matched pair of quotes, then any trailing comment.
        if (value.starts_with('"') && value.ends_with('"') && value.len() >= 2)
            || (value.starts_with('\'') && value.ends_with('\'') && value.len() >= 2)
        {
            value = &value[1..value.len() - 1];
        } else if let Some(idx) = value.find(" #") {
            value = value[..idx].trim();
        }
        out.insert(key.to_string(), value.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> BTreeMap<String, String> {
        let mut m = BTreeMap::new();
        parse_env_into(text, &mut m);
        m
    }

    #[test]
    fn parses_plain_assignments() {
        let m = parse("A=1\nB=hello world\n");
        assert_eq!(m["A"], "1");
        assert_eq!(m["B"], "hello world");
    }

    #[test]
    fn ignores_comments_and_blank_lines() {
        let m = parse("# a comment\n\n  \nA=1\n");
        assert_eq!(m.len(), 1);
        assert_eq!(m["A"], "1");
    }

    #[test]
    fn handles_export_prefix_and_quotes() {
        let m = parse("export A=\"quoted value\"\nB='single'\n");
        assert_eq!(m["A"], "quoted value");
        assert_eq!(m["B"], "single");
    }

    #[test]
    fn keeps_values_containing_equals_and_hashes() {
        // API keys and base64 secrets routinely contain '='; a '#' with no
        // preceding space is part of the value, not a comment.
        let m = parse("KEY=sk-or-v1-abc==\nURL=http://h/p#frag\nC=v # trailing\n");
        assert_eq!(m["KEY"], "sk-or-v1-abc==");
        assert_eq!(m["URL"], "http://h/p#frag");
        assert_eq!(m["C"], "v");
    }

    #[test]
    fn skips_malformed_lines_rather_than_panicking() {
        let m = parse("no_equals_here\n=novalue\nA=1\n");
        assert_eq!(m.len(), 1);
        assert_eq!(m["A"], "1");
    }

    fn resolve_with(vars: &[(&str, &str)]) -> (ProviderConfig, String) {
        let map: BTreeMap<String, String> =
            vars.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        let get = |k: &str| -> Option<String> {
            map.get(k).map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
        };
        // Mirrors the auto-detect ladder in resolve_provider().
        if get("ANTHROPIC_API_KEY").is_some() {
            return (
                ProviderConfig::Anthropic {
                    api_key: get("ANTHROPIC_API_KEY").unwrap(),
                    model: deskfolk_ai::anthropic_default_model(),
                },
                "anthropic".into(),
            );
        }
        if get("OPENROUTER_API_KEY").is_some() {
            return (openrouter(&get, get("DESKFOLK_MODEL")), "openrouter".into());
        }
        if get("GROQ_API_KEY").is_some() {
            return (groq(&get, get("DESKFOLK_MODEL")), "groq".into());
        }
        (ProviderConfig::Sidecar { base_url: "http://127.0.0.1:8087".into() }, "sidecar".into())
    }

    fn voice_with(vars: &[(&str, &str)]) -> Option<String> {
        let map: BTreeMap<String, String> =
            vars.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        let get = |k: &str| -> Option<String> {
            map.get(k).map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
        };
        voice_url(&get)
    }

    #[test]
    fn voice_defaults_to_the_local_brain() {
        assert_eq!(voice_with(&[]).as_deref(), Some("http://127.0.0.1:8087"));
    }

    #[test]
    fn voice_reuses_the_alphas_pet_brain_variable() {
        // An existing desktop-pet setup already points PET_BRAIN at the brain;
        // asking the user to set a second variable for the same thing would be
        // a poor trade.
        assert_eq!(
            voice_with(&[("PET_BRAIN", "http://192.168.2.5:8087")]).as_deref(),
            Some("http://192.168.2.5:8087")
        );
    }

    #[test]
    fn an_explicit_voice_url_wins() {
        assert_eq!(
            voice_with(&[("PET_BRAIN", "http://a:1"), ("DESKFOLK_VOICE_URL", "http://b:2")])
                .as_deref(),
            Some("http://b:2")
        );
    }

    #[test]
    fn voice_can_be_turned_off() {
        // Someone who wants a silent companion must be able to have one
        // without the app retrying a dead endpoint on every reply.
        assert!(voice_with(&[("DESKFOLK_VOICE", "off")]).is_none());
        assert!(voice_with(&[("DESKFOLK_VOICE", "0")]).is_none());
        assert!(voice_with(&[("DESKFOLK_VOICE", "FALSE")]).is_none());
    }

    #[test]
    fn a_trailing_slash_is_trimmed_so_urls_never_double_up() {
        assert_eq!(
            voice_with(&[("DESKFOLK_VOICE_URL", "http://host:8087/")]).as_deref(),
            Some("http://host:8087")
        );
    }

    #[test]
    fn openrouter_key_selects_an_openai_compatible_provider() {
        let (cfg, why) = resolve_with(&[("OPENROUTER_API_KEY", "sk-or-test")]);
        assert_eq!(why, "openrouter");
        match cfg {
            ProviderConfig::OpenAiCompat { base_url, model, api_key } => {
                assert!(base_url.contains("openrouter.ai"));
                assert_eq!(model, DEFAULT_OPENROUTER_MODEL);
                assert_eq!(api_key, "sk-or-test");
            }
            other => panic!("expected OpenAiCompat, got {other:?}"),
        }
    }

    #[test]
    fn model_override_beats_the_default() {
        let (cfg, _) = resolve_with(&[
            ("OPENROUTER_API_KEY", "k"),
            ("DESKFOLK_MODEL", "x-ai/grok-4.5"),
        ]);
        match cfg {
            ProviderConfig::OpenAiCompat { model, .. } => assert_eq!(model, "x-ai/grok-4.5"),
            other => panic!("expected OpenAiCompat, got {other:?}"),
        }
    }

    #[test]
    fn falls_back_to_the_sidecar_when_nothing_is_configured() {
        let (cfg, why) = resolve_with(&[]);
        assert_eq!(why, "sidecar");
        assert!(matches!(cfg, ProviderConfig::Sidecar { .. }));
    }

    #[test]
    fn blank_keys_are_treated_as_absent() {
        // An exported-but-empty key must not select a provider that will then
        // fail on every request.
        let (_, why) = resolve_with(&[("OPENROUTER_API_KEY", "   ")]);
        assert_eq!(why, "sidecar");
    }
}
