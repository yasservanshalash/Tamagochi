//! The mind.
//!
//! The engine decides *when* a companion speaks; this crate decides *what* it
//! says, and it is deliberately replaceable. Every provider implements one
//! trait, so swapping a cloud model for a local one — or adding a provider
//! that does not exist yet — never touches the engine or a character package.
//!
//! Cloud providers work out of the box in the default install. Local models
//! (Ollama, whisper, Kokoro) arrive as an optional "Local Engine" pack that
//! speaks the same trait through [`Sidecar`], which is the alpha's Python
//! brain kept alive rather than rewritten.

use std::collections::BTreeMap;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

mod anthropic;
mod openai_compat;
mod sidecar;

pub use anthropic::default_model as anthropic_default_model;
pub use anthropic::Anthropic;
pub use openai_compat::OpenAiCompat;
pub use sidecar::Sidecar;

#[derive(Debug, thiserror::Error)]
pub enum AiError {
    #[error("no API key configured for {0}")]
    MissingKey(&'static str),
    #[error("{provider} request failed: {source}")]
    Transport { provider: &'static str, source: reqwest::Error },
    #[error("{provider} returned {status}: {body}")]
    Status { provider: &'static str, status: u16, body: String },
    #[error("could not understand the {provider} response: {detail}")]
    Malformed { provider: &'static str, detail: String },
}

/// What the mind is being asked about. Mirrors the alpha's brain contract so
/// the existing Python server remains a drop-in provider.
#[derive(Debug, Clone, Default)]
pub struct ThinkRequest {
    /// Character brief, assembled from the package's personality block.
    pub system: String,
    /// Prior conversation, oldest first.
    pub history: Vec<Turn>,
    /// `wake_greet`, `self_talk`, `user_speech`, …
    pub event: String,
    /// What the user actually said, when there is one.
    pub text: String,
    /// Emotion names this character supports. The reply is constrained to
    /// these, so a character can never be asked to perform a face it lacks.
    pub emotions: Vec<String>,
    /// Ambient facts the character can react to (hour, battery, focused app).
    pub senses: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Turn {
    pub role: Role,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Companion,
}

/// The mind's answer. Identical in shape to the alpha's JSON reply.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ThinkReply {
    pub say: String,
    #[serde(default)]
    pub emotion: String,
    #[serde(default)]
    pub glitch: u8,
    #[serde(default)]
    pub action: String,
    /// Something to do to whatever is playing. Separate from `action` because
    /// it happens *alongside* what he says and does — he can skip a track and
    /// still be listening to you — where `action` is what his body does next.
    #[serde(default)]
    pub music: Option<MusicWish>,
}

/// A request to the music, as the mind words it. Validated by the app, which
/// owns the small set of things that can actually be done.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct MusicWish {
    /// `next`, `louder`, `pause`, `play`, …
    #[serde(default, alias = "action", alias = "cmd")]
    pub r#do: String,
    /// What to put on, when the verb is `play`.
    #[serde(default, alias = "track", alias = "q")]
    pub query: String,
}

impl ThinkReply {
    /// Clamp a reply to what this character can actually perform. A model that
    /// invents an emotion or a verb must never wedge the engine.
    pub fn sanitize(mut self, emotions: &[String]) -> Self {
        if !emotions.iter().any(|e| e == &self.emotion) {
            self.emotion = if emotions.iter().any(|e| e == "confused") {
                "confused".into()
            } else {
                emotions.first().cloned().unwrap_or_default()
            };
        }
        if !matches!(
            self.action.as_str(),
            "listen" | "sleep" | "walk" | "walk_left" | "walk_right"
        ) {
            self.action = "none".into();
        }
        self.glitch = self.glitch.min(100);
        self.say = self.say.trim().to_string();
        self
    }
}

#[async_trait]
pub trait Provider: Send + Sync {
    /// Stable name, for logs and the Control Center.
    fn name(&self) -> &'static str;
    async fn think(&self, req: &ThinkRequest) -> Result<ThinkReply, AiError>;
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Which mind to use. Serialized into settings, so adding a provider does not
/// invalidate an existing config.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "provider", rename_all = "kebab-case")]
pub enum ProviderConfig {
    Anthropic {
        #[serde(default)]
        api_key: String,
        #[serde(default = "anthropic::default_model")]
        model: String,
    },
    /// Anything speaking the OpenAI chat-completions shape: OpenAI itself,
    /// OpenRouter, Groq, Ollama, LM Studio, vLLM.
    OpenAiCompat {
        base_url: String,
        #[serde(default)]
        api_key: String,
        model: String,
    },
    /// The optional Local Engine pack — the alpha's Python brain.
    Sidecar {
        #[serde(default = "sidecar::default_base")]
        base_url: String,
    },
}

impl Default for ProviderConfig {
    fn default() -> Self {
        // Cloud-first so a fresh install talks without a 2GB download. The key
        // comes from the environment until the Control Center can store it.
        ProviderConfig::Anthropic {
            api_key: std::env::var("ANTHROPIC_API_KEY").unwrap_or_default(),
            model: anthropic::default_model(),
        }
    }
}

impl ProviderConfig {
    /// Log-safe summary. The struct derives `Debug`, which would print the
    /// API key verbatim, so anything user-visible must go through this.
    pub fn describe(&self) -> String {
        fn keyed(present: bool) -> &'static str {
            if present {
                "key set"
            } else {
                "no key"
            }
        }
        match self {
            ProviderConfig::Anthropic { api_key, model } => {
                format!("anthropic model={model} ({})", keyed(!api_key.trim().is_empty()))
            }
            ProviderConfig::OpenAiCompat { base_url, api_key, model } => format!(
                "openai-compat base={base_url} model={model} ({})",
                keyed(!api_key.trim().is_empty())
            ),
            ProviderConfig::Sidecar { base_url } => format!("local sidecar at {base_url}"),
        }
    }

    pub fn build(&self) -> Box<dyn Provider> {
        match self {
            ProviderConfig::Anthropic { api_key, model } => {
                Box::new(Anthropic::new(api_key.clone(), model.clone()))
            }
            ProviderConfig::OpenAiCompat { base_url, api_key, model } => Box::new(
                OpenAiCompat::new(base_url.clone(), api_key.clone(), model.clone()),
            ),
            ProviderConfig::Sidecar { base_url } => Box::new(Sidecar::new(base_url.clone())),
        }
    }
}

// ---------------------------------------------------------------------------
// Prompt assembly — shared by the model-backed providers
// ---------------------------------------------------------------------------

/// Build the instruction block appended to the character's own brief.
///
/// The character's voice comes entirely from its package; this only explains
/// the reply contract. Keeping the two separate is what stops engine changes
/// from quietly rewriting someone's character.
pub(crate) fn reply_contract(req: &ThinkRequest) -> String {
    let emotions = req.emotions.join(", ");
    let mut s = format!(
        "You are speaking as a character who lives on this person's desktop.\n\
         Reply with a single short line — one or two sentences, spoken aloud. \
         No narration, no stage directions, no markdown.\n\
         Pick the emotion that best fits your reply from exactly this list: {emotions}.\n\
         Set \"action\" to \"listen\" only if you asked a question and want an \
         answer, \"sleep\" if you are settling down, otherwise \"none\"."
    );

    // Without this a companion left alone loops on one idea all evening —
    // every idle line a variation of the last. The alpha fought the same
    // thing and rerolled repeats server-side; saying it plainly is cheaper.
    let said_before: Vec<&str> = req
        .history
        .iter()
        .filter(|t| t.role == Role::Companion)
        .map(|t| t.text.as_str())
        .collect();
    if !said_before.is_empty() {
        s.push_str(
            "\n\nYou have already said the lines below. Do not repeat them, \
             reuse their imagery, or restate the same observation — say \
             something about a different subject entirely:\n",
        );
        for line in said_before.iter().rev().take(6) {
            s.push_str(&format!("- {line}\n"));
        }
    }
    s
}

/// Render the situation as a user turn, so providers only differ in transport.
pub(crate) fn situation(req: &ThinkRequest) -> String {
    let mut s = String::new();
    if !req.senses.is_empty() {
        let senses: Vec<String> =
            req.senses.iter().map(|(k, v)| format!("{k}: {v}")).collect();
        s.push_str(&format!("[{}]\n", senses.join(", ")));
    }
    match req.event.as_str() {
        "user_speech" => s.push_str(&req.text),
        "wake_greet" => s.push_str("(you just woke up — say something)"),
        // Each idle prompt names a different angle, because "say whatever is
        // on your mind" asked repeatedly gets the same answer repeatedly.
        // Thoughts are meant to stack. Asking for a subject he has *not*
        // raised recently produced the opposite of an inner life: a stream of
        // unrelated trivia, each line arriving from nowhere. His own musings
        // are in the history, so continuing one is simply a matter of asking.
        "self_talk" => s.push_str(
            "(nobody said anything. Say one unprompted thought. Look at what \
             you last mused about: if that thread is still live, continue it — \
             take it further, land it, or change your mind about it. Start a \
             new one only when the old is finished.)",
        ),
        "long_session" => s.push_str(&format!(
            "(they have been at this machine for about {} hours without a real              break. Say something about it in your own voice - concerned,              teasing, whatever fits you. Do not lecture and do not list health              advice.)",
            if req.text.is_empty() { "several" } else { &req.text }
        )),
        "user_poke" => s.push_str(
            "(they just clicked on you to get your attention — react to being \
             poked, don't narrate the room)",
        ),
        other => {
            if req.text.is_empty() {
                s.push_str(&format!("({other})"));
            } else {
                s.push_str(&format!("({other}) {}", req.text));
            }
        }
    }
    s
}

/// JSON Schema for the reply, with `emotion` constrained to the character's
/// own list. Providers that support structured output enforce this; the
/// others get it as a prompt and are re-checked by [`ThinkReply::sanitize`].
pub(crate) fn reply_schema(emotions: &[String]) -> serde_json::Value {
    let emotions = if emotions.is_empty() {
        vec!["confused".to_string()]
    } else {
        emotions.to_vec()
    };
    serde_json::json!({
        "type": "object",
        "properties": {
            "say": { "type": "string", "description": "What the character says out loud." },
            "emotion": { "type": "string", "enum": emotions },
            "glitch": { "type": "integer", "description": "0-100 instability." },
            "action": { "type": "string", "enum": ["none", "listen", "sleep"] }
        },
        "required": ["say", "emotion", "glitch", "action"],
        "additionalProperties": false
    })
}

/// Pull a reply out of model text that may be wrapped in prose or a code
/// fence. Local models in particular rarely return bare JSON.
pub(crate) fn parse_reply(provider: &'static str, raw: &str) -> Result<ThinkReply, AiError> {
    let trimmed = raw.trim();
    if let Ok(r) = serde_json::from_str::<ThinkReply>(trimmed) {
        return Ok(r);
    }
    // Fall back to the outermost {...} span.
    if let (Some(start), Some(end)) = (trimmed.find('{'), trimmed.rfind('}')) {
        if start < end {
            if let Ok(r) = serde_json::from_str::<ThinkReply>(&trimmed[start..=end]) {
                return Ok(r);
            }
        }
    }
    // Last resort: treat the whole thing as speech. A character that says
    // something slightly off is better than one that goes mute.
    if !trimmed.is_empty() && !trimmed.starts_with('{') {
        return Ok(ThinkReply {
            say: trimmed.to_string(),
            emotion: "talk".into(),
            ..Default::default()
        });
    }
    Err(AiError::Malformed {
        provider,
        detail: format!("no JSON object in: {}", truncate(trimmed, 200)),
    })
}

pub(crate) fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        s.chars().take(n).collect::<String>() + "…"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn emos() -> Vec<String> {
        ["idle", "talk", "happy", "confused", "sad"]
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    #[test]
    fn parses_bare_json() {
        let r = parse_reply("t", r#"{"say":"yo","emotion":"happy","glitch":0,"action":"none"}"#)
            .unwrap();
        assert_eq!(r.say, "yo");
        assert_eq!(r.emotion, "happy");
    }

    #[test]
    fn parses_json_inside_a_code_fence() {
        // Local models do this constantly.
        let raw = "Sure!\n```json\n{\"say\":\"hey\",\"emotion\":\"talk\",\"glitch\":0,\"action\":\"none\"}\n```";
        let r = parse_reply("t", raw).unwrap();
        assert_eq!(r.say, "hey");
    }

    #[test]
    fn falls_back_to_treating_plain_prose_as_speech() {
        // Going mute is a worse failure than a slightly off reply.
        let r = parse_reply("t", "man, I dunno about that").unwrap();
        assert_eq!(r.say, "man, I dunno about that");
        assert_eq!(r.emotion, "talk");
    }

    #[test]
    fn empty_output_is_an_error_not_a_silent_blank() {
        assert!(parse_reply("t", "   ").is_err());
    }

    #[test]
    fn sanitize_replaces_an_invented_emotion() {
        let r = ThinkReply { emotion: "smouldering".into(), ..Default::default() }.sanitize(&emos());
        assert_eq!(r.emotion, "confused");
    }

    #[test]
    fn sanitize_falls_back_to_the_first_emotion_when_confused_is_absent() {
        let emos = vec!["blink".to_string(), "wave".to_string()];
        let r = ThinkReply { emotion: "nope".into(), ..Default::default() }.sanitize(&emos);
        assert_eq!(r.emotion, "blink");
    }

    #[test]
    fn sanitize_rejects_unknown_actions_and_clamps_glitch() {
        let r = ThinkReply {
            emotion: "talk".into(),
            action: "launch_missiles".into(),
            glitch: 250,
            ..Default::default()
        }
        .sanitize(&emos());
        assert_eq!(r.action, "none");
        assert_eq!(r.glitch, 100);
    }

    #[test]
    fn sanitize_keeps_valid_actions() {
        // The agent vocabulary his body can perform: the two the engine owns
        // plus the movement verbs the app grants.
        for a in ["listen", "sleep", "walk", "walk_left", "walk_right"] {
            let r = ThinkReply { emotion: "talk".into(), action: a.into(), ..Default::default() }
                .sanitize(&emos());
            assert_eq!(r.action, a);
        }
    }

    #[test]
    fn schema_constrains_emotion_to_this_character() {
        let s = reply_schema(&emos());
        let list = s["properties"]["emotion"]["enum"].as_array().unwrap();
        assert_eq!(list.len(), 5);
        assert!(list.iter().any(|v| v == "happy"));
        // A character without a "dance" clip can never be asked to dance.
        assert!(!list.iter().any(|v| v == "dance"));
    }

    #[test]
    fn schema_never_emits_an_empty_enum() {
        // An empty enum is invalid JSON Schema and would 400 the request.
        let s = reply_schema(&[]);
        assert!(!s["properties"]["emotion"]["enum"].as_array().unwrap().is_empty());
    }

    #[test]
    fn situation_renders_speech_and_ambient_context() {
        let mut senses = BTreeMap::new();
        senses.insert("hour".to_string(), "23".to_string());
        let req = ThinkRequest {
            event: "user_speech".into(),
            text: "you up?".into(),
            senses,
            ..Default::default()
        };
        let s = situation(&req);
        assert!(s.contains("hour: 23"));
        assert!(s.ends_with("you up?"));
    }

    #[test]
    fn self_talk_has_no_user_text_to_echo() {
        let req = ThinkRequest { event: "self_talk".into(), ..Default::default() };
        assert!(situation(&req).contains("nobody said anything"));
    }

    #[test]
    fn a_poke_asks_for_a_reaction_not_a_room_description() {
        // Before this, "user_poke" fell through to a bare "(user_poke)" and
        // he narrated the empty desktop every single time.
        let req = ThinkRequest { event: "user_poke".into(), ..Default::default() };
        let s = situation(&req);
        assert!(s.contains("clicked on you"), "got: {s}");
        assert!(!s.contains("(user_poke)"), "the raw event name leaked into the prompt");
    }

    #[test]
    fn contract_lists_prior_lines_so_he_stops_repeating_himself() {
        let req = ThinkRequest {
            emotions: emos(),
            history: vec![
                Turn { role: Role::User, text: "hey".into() },
                Turn { role: Role::Companion, text: "where'd all the pixels go".into() },
            ],
            ..Default::default()
        };
        let c = reply_contract(&req);
        assert!(c.contains("Do not repeat"));
        assert!(c.contains("where'd all the pixels go"));
        assert!(!c.contains("- hey"), "only his own lines should be listed back");
    }

    #[test]
    fn contract_stays_clean_on_the_first_exchange() {
        let req = ThinkRequest { emotions: emos(), ..Default::default() };
        assert!(!reply_contract(&req).contains("already said"));
    }

    #[test]
    fn contract_caps_how_many_prior_lines_it_replays() {
        // Otherwise a long session ships his whole transcript twice.
        let req = ThinkRequest {
            emotions: emos(),
            history: (0..30)
                .map(|i| Turn { role: Role::Companion, text: format!("line {i}") })
                .collect(),
            ..Default::default()
        };
        let c = reply_contract(&req);
        assert_eq!(c.matches("\n- ").count(), 6);
        assert!(c.contains("line 29"), "the most recent lines are the ones that matter");
    }

    #[test]
    fn truncate_is_char_safe_on_multibyte_input() {
        // Byte slicing here would panic on emoji or accented text.
        let s = "héllo 🌍 wörld";
        assert_eq!(truncate(s, 100), s);
        assert!(truncate(s, 3).chars().count() <= 4);
    }
}
