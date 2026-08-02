//! The Local Engine pack — the alpha's Python brain, kept rather than rewritten.
//!
//! `brain/think_server.py` already does local Whisper STT, Orpheus/Kokoro TTS
//! and Ollama routing, tuned over a lot of real use. Reimplementing that in
//! Rust would have been weeks of work to arrive back where the alpha already
//! was, so it ships as an optional download and speaks the same [`Provider`]
//! trait as everything else. It is the reason the default installer can be
//! ~15MB instead of a couple of gigabytes.

use async_trait::async_trait;
use serde_json::json;

use crate::{truncate, AiError, Provider, ThinkReply, ThinkRequest};

pub fn default_base() -> String {
    "http://127.0.0.1:8087".to_string()
}

pub struct Sidecar {
    base_url: String,
    http: reqwest::Client,
}

impl Sidecar {
    pub fn new(base_url: String) -> Self {
        let base = if base_url.trim().is_empty() { default_base() } else { base_url };
        Self {
            base_url: base.trim_end_matches('/').to_string(),
            http: reqwest::Client::new(),
        }
    }

    /// Is the local pack installed and running? The Control Center uses this
    /// to show the engine as available rather than letting the user pick a
    /// provider that will fail on first use.
    pub async fn healthy(&self) -> bool {
        matches!(
            self.http.get(format!("{}/health", self.base_url)).send().await,
            Ok(r) if r.status().is_success()
        )
    }

    /// The alpha's `/pet/think` contract, unchanged.
    fn body(&self, req: &ThinkRequest) -> serde_json::Value {
        json!({
            "event": req.event,
            "text": req.text,
            "vitals": { "battery": 100, "charging": true },
            "senses": req.senses,
        })
    }
}

#[async_trait]
impl Provider for Sidecar {
    fn name(&self) -> &'static str {
        "local-sidecar"
    }

    async fn think(&self, req: &ThinkRequest) -> Result<ThinkReply, AiError> {
        let res = self
            .http
            .post(format!("{}/pet/think", self.base_url))
            .json(&self.body(req))
            .send()
            .await
            .map_err(|source| AiError::Transport { provider: "local-sidecar", source })?;

        let status = res.status();
        let text = res
            .text()
            .await
            .map_err(|source| AiError::Transport { provider: "local-sidecar", source })?;
        if !status.is_success() {
            return Err(AiError::Status {
                provider: "local-sidecar",
                status: status.as_u16(),
                body: truncate(&text, 400),
            });
        }

        // The sidecar already returns {say, emotion, glitch, action}.
        let reply: ThinkReply = serde_json::from_str(&text).map_err(|e| AiError::Malformed {
            provider: "local-sidecar",
            detail: format!("{e}: {}", truncate(&text, 200)),
        })?;
        Ok(reply.sanitize(&req.emotions))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_the_alpha_brain_port() {
        assert_eq!(Sidecar::new(String::new()).base_url, "http://127.0.0.1:8087");
    }

    #[test]
    fn trims_a_trailing_slash() {
        assert_eq!(Sidecar::new("http://host:9/".into()).base_url, "http://host:9");
    }

    #[test]
    fn body_matches_the_alpha_think_contract() {
        let s = Sidecar::new(String::new());
        let b = s.body(&ThinkRequest {
            event: "wake_greet".into(),
            text: "hi".into(),
            ..Default::default()
        });
        assert_eq!(b["event"], "wake_greet");
        assert_eq!(b["text"], "hi");
        assert!(b.get("vitals").is_some());
        assert!(b.get("senses").is_some());
    }

    #[test]
    fn a_sidecar_reply_is_still_clamped_to_this_character() {
        // The Python brain predates the package format and can emit emotions a
        // given character does not have.
        let raw = r#"{"say":"yo","emotion":"facepalm","glitch":0,"action":"none"}"#;
        let reply: ThinkReply = serde_json::from_str(raw).unwrap();
        let out = reply.sanitize(&["idle".to_string(), "confused".to_string()]);
        assert_eq!(out.emotion, "confused");
    }
}
