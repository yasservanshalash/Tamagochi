//! Anthropic Messages API provider.
//!
//! Raw HTTP rather than an SDK — there is no official Rust SDK, and the
//! Messages API surface we need here is small.

use async_trait::async_trait;
use serde_json::json;

use crate::{
    parse_reply, reply_contract, reply_schema, situation, truncate, AiError, Provider, Role,
    ThinkReply, ThinkRequest,
};

const API_URL: &str = "https://api.anthropic.com/v1/messages";
const API_VERSION: &str = "2023-06-01";

/// Server-side fallback: if a safety classifier declines a request, the API
/// re-runs it on a recommended model instead of returning the refusal. Worth
/// having by default here — this character is deliberately edgy, and a
/// companion that goes silent mid-conversation reads as broken.
const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";

/// Thinking is on by default on this model and `max_tokens` caps thinking
/// *plus* reply text, so a pet-sized budget would truncate mid-sentence.
const MAX_TOKENS: u32 = 2048;

pub fn default_model() -> String {
    "claude-opus-5".to_string()
}

pub struct Anthropic {
    api_key: String,
    model: String,
    http: reqwest::Client,
}

impl Anthropic {
    pub fn new(api_key: String, model: String) -> Self {
        Self {
            api_key,
            model: if model.is_empty() { default_model() } else { model },
            http: reqwest::Client::new(),
        }
    }

    /// Pure body builder, so the request shape is testable without a network.
    fn body(&self, req: &ThinkRequest) -> serde_json::Value {
        let mut messages: Vec<serde_json::Value> = req
            .history
            .iter()
            .map(|t| {
                json!({
                    "role": match t.role { Role::User => "user", Role::Companion => "assistant" },
                    "content": t.text,
                })
            })
            .collect();
        messages.push(json!({ "role": "user", "content": situation(req) }));

        json!({
            "model": self.model,
            "max_tokens": MAX_TOKENS,
            "system": format!("{}\n\n{}", req.system, reply_contract(req)),
            "messages": messages,
            "output_config": {
                // Structured output guarantees a parseable reply and, because
                // the schema's emotion enum comes from the character package,
                // guarantees the mood is one this character can actually play.
                "format": { "type": "json_schema", "schema": reply_schema(&req.emotions) },
                // A desktop companion is a latency-sensitive, low-stakes
                // exchange. Low effort is quick and still well-judged here.
                "effort": "low",
            },
            "fallbacks": "default",
        })
    }
}

#[async_trait]
impl Provider for Anthropic {
    fn name(&self) -> &'static str {
        "anthropic"
    }

    async fn think(&self, req: &ThinkRequest) -> Result<ThinkReply, AiError> {
        if self.api_key.trim().is_empty() {
            return Err(AiError::MissingKey("anthropic"));
        }

        let res = self
            .http
            .post(API_URL)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", API_VERSION)
            .header("anthropic-beta", FALLBACK_BETA)
            .json(&self.body(req))
            .send()
            .await
            .map_err(|source| AiError::Transport { provider: "anthropic", source })?;

        let status = res.status();
        let text = res
            .text()
            .await
            .map_err(|source| AiError::Transport { provider: "anthropic", source })?;
        if !status.is_success() {
            return Err(AiError::Status {
                provider: "anthropic",
                status: status.as_u16(),
                body: truncate(&text, 400),
            });
        }

        let value: serde_json::Value =
            serde_json::from_str(&text).map_err(|e| AiError::Malformed {
                provider: "anthropic",
                detail: e.to_string(),
            })?;

        // Check the stop reason before touching content: a refusal returns a
        // successful 200 with empty or partial content, so indexing straight
        // into content[0] would panic or read a half-sentence.
        if value.get("stop_reason").and_then(|s| s.as_str()) == Some("refusal") {
            return Err(AiError::Malformed {
                provider: "anthropic",
                detail: "request was declined by safety classifiers".into(),
            });
        }

        let said = extract_text(&value).ok_or_else(|| AiError::Malformed {
            provider: "anthropic",
            detail: format!("no text block in: {}", truncate(&text, 300)),
        })?;

        Ok(parse_reply("anthropic", &said)?.sanitize(&req.emotions))
    }
}

/// Concatenate every text block. Ignores thinking and fallback marker blocks,
/// which carry no reply text.
fn extract_text(value: &serde_json::Value) -> Option<String> {
    let blocks = value.get("content")?.as_array()?;
    let joined: String = blocks
        .iter()
        .filter(|b| b.get("type").and_then(|t| t.as_str()) == Some("text"))
        .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
        .collect::<Vec<_>>()
        .join("");
    if joined.trim().is_empty() {
        None
    } else {
        Some(joined)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Turn;

    fn req() -> ThinkRequest {
        ThinkRequest {
            system: "You are a grumpy frog.".into(),
            event: "user_speech".into(),
            text: "hey".into(),
            emotions: vec!["idle".into(), "talk".into(), "happy".into()],
            ..Default::default()
        }
    }

    #[test]
    fn body_has_the_required_message_shape() {
        let p = Anthropic::new("k".into(), String::new());
        let b = p.body(&req());
        assert_eq!(b["model"], "claude-opus-5");
        assert!(b["max_tokens"].as_u64().unwrap() >= 1024, "thinking needs headroom");
        assert!(b["system"].as_str().unwrap().contains("grumpy frog"));
        let msgs = b["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["role"], "user");
    }

    #[test]
    fn body_omits_sampling_parameters() {
        // temperature / top_p / top_k are rejected outright on this model.
        let p = Anthropic::new("k".into(), String::new());
        let b = p.body(&req());
        for banned in ["temperature", "top_p", "top_k"] {
            assert!(b.get(banned).is_none(), "{banned} must not be sent");
        }
    }

    #[test]
    fn body_constrains_emotion_to_the_character() {
        let p = Anthropic::new("k".into(), String::new());
        let b = p.body(&req());
        let e = &b["output_config"]["format"]["schema"]["properties"]["emotion"]["enum"];
        assert_eq!(e.as_array().unwrap().len(), 3);
    }

    #[test]
    fn history_maps_companion_turns_to_assistant() {
        let p = Anthropic::new("k".into(), String::new());
        let mut r = req();
        r.history = vec![
            Turn { role: Role::User, text: "yo".into() },
            Turn { role: Role::Companion, text: "what".into() },
        ];
        let b = p.body(&r);
        let msgs = b["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[0]["role"], "user");
        assert_eq!(msgs[1]["role"], "assistant");
        assert_eq!(msgs[2]["role"], "user", "the new situation goes last");
    }

    #[tokio::test]
    async fn missing_key_fails_fast_without_a_request() {
        let p = Anthropic::new("  ".into(), String::new());
        assert!(matches!(p.think(&req()).await, Err(AiError::MissingKey("anthropic"))));
    }

    #[test]
    fn extracts_and_joins_text_blocks() {
        let v = json!({"content":[
            {"type":"thinking","thinking":""},
            {"type":"text","text":"{\"say\":\"hi\","},
            {"type":"text","text":"\"emotion\":\"talk\",\"glitch\":0,\"action\":\"none\"}"}
        ]});
        let t = extract_text(&v).unwrap();
        let r = parse_reply("anthropic", &t).unwrap();
        assert_eq!(r.say, "hi");
    }

    #[test]
    fn no_text_block_is_reported_rather_than_indexed_into() {
        let v = json!({"content":[{"type":"thinking","thinking":"..."}]});
        assert!(extract_text(&v).is_none());
        assert!(extract_text(&json!({"content": []})).is_none());
        assert!(extract_text(&json!({})).is_none());
    }
}
