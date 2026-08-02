//! One provider for every OpenAI-shaped chat API.
//!
//! OpenAI, OpenRouter, Groq, Ollama, LM Studio and vLLM all speak
//! `/chat/completions` with the same request body. Rather than a class per
//! vendor, this is a base URL plus a model name — which also means a provider
//! that does not exist yet usually works the day it ships.

use async_trait::async_trait;
use serde_json::json;

use crate::{
    parse_reply, reply_contract, reply_schema, situation, truncate, AiError, Provider, Role,
    ThinkReply, ThinkRequest,
};

pub struct OpenAiCompat {
    base_url: String,
    api_key: String,
    model: String,
    http: reqwest::Client,
}

impl OpenAiCompat {
    pub fn new(base_url: String, api_key: String, model: String) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key,
            model,
            http: reqwest::Client::new(),
        }
    }

    fn endpoint(&self) -> String {
        format!("{}/chat/completions", self.base_url)
    }

    fn body(&self, req: &ThinkRequest, mode: SchemaMode) -> serde_json::Value {
        let mut messages = vec![json!({
            "role": "system",
            "content": format!("{}\n\n{}", req.system, reply_contract(req)),
        })];
        for t in &req.history {
            messages.push(json!({
                "role": match t.role { Role::User => "user", Role::Companion => "assistant" },
                "content": t.text,
            }));
        }
        messages.push(json!({ "role": "user", "content": situation(req) }));

        let mut body = json!({
            "model": self.model,
            "messages": messages,
            "max_tokens": 300,
            // Local models repeat themselves badly without these; the alpha
            // hit this constantly with Mistral through Ollama.
            "temperature": 0.9,
            "frequency_penalty": 0.4,
            "presence_penalty": 0.3,
        });

        match mode {
            SchemaMode::JsonSchema => {
                body["response_format"] = json!({
                    "type": "json_schema",
                    "json_schema": {
                        "name": "companion_reply",
                        "strict": true,
                        "schema": reply_schema(&req.emotions),
                    },
                });
            }
            SchemaMode::JsonObject => {
                // Plain JSON mode: the shape now rides on the prompt, and
                // ThinkReply::sanitize re-checks whatever comes back.
                body["response_format"] = json!({ "type": "json_object" });
            }
            SchemaMode::Freeform => {}
        }
        body
    }

    async fn attempt(
        &self,
        req: &ThinkRequest,
        mode: SchemaMode,
    ) -> Result<ThinkReply, AiError> {
        let mut r = self.http.post(self.endpoint()).json(&self.body(req, mode));
        // Local servers (Ollama, LM Studio) accept and ignore any key, so an
        // empty one simply means "don't send the header".
        if !self.api_key.trim().is_empty() {
            r = r.bearer_auth(&self.api_key);
        }
        // OpenRouter uses these for attribution and ranking; harmless elsewhere.
        if self.base_url.contains("openrouter.ai") {
            r = r
                .header("HTTP-Referer", "https://deskfolk.app")
                .header("X-Title", "Deskfolk");
        }

        let res = r
            .send()
            .await
            .map_err(|source| AiError::Transport { provider: "openai-compat", source })?;
        let status = res.status();
        let text = res
            .text()
            .await
            .map_err(|source| AiError::Transport { provider: "openai-compat", source })?;
        if !status.is_success() {
            return Err(AiError::Status {
                provider: "openai-compat",
                status: status.as_u16(),
                body: truncate(&text, 400),
            });
        }

        let value: serde_json::Value =
            serde_json::from_str(&text).map_err(|e| AiError::Malformed {
                provider: "openai-compat",
                detail: e.to_string(),
            })?;

        // Some gateways return a 200 carrying an error object instead.
        if let Some(msg) = value.pointer("/error/message").and_then(|m| m.as_str()) {
            return Err(AiError::Status {
                provider: "openai-compat",
                status: value
                    .pointer("/error/code")
                    .and_then(|c| c.as_u64())
                    .unwrap_or(200) as u16,
                body: truncate(msg, 400),
            });
        }

        let said = value
            .pointer("/choices/0/message/content")
            .and_then(|c| c.as_str())
            .ok_or_else(|| AiError::Malformed {
                provider: "openai-compat",
                detail: format!("no message content in: {}", truncate(&text, 300)),
            })?;

        Ok(parse_reply("openai-compat", said)?.sanitize(&req.emotions))
    }
}

/// How hard to constrain the reply shape.
///
/// "OpenAI-compatible" is a spectrum: some servers enforce a full JSON schema,
/// some only know `json_object`, and some reject `response_format` outright.
/// Notably the uncensored models that suit an edgy character are often in the
/// second or third group, so the provider walks down the ladder rather than
/// giving up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SchemaMode {
    JsonSchema,
    JsonObject,
    Freeform,
}

/// Does this failure look like the server objecting to `response_format`
/// rather than to the request as a whole?
fn is_schema_rejection(e: &AiError) -> bool {
    match e {
        AiError::Status { status, body, .. } => {
            let b = body.to_ascii_lowercase();
            (*status == 400 || *status == 404 || *status == 422 || *status == 500)
                && (b.contains("response_format")
                    || b.contains("json_schema")
                    || b.contains("structured output")
                    || b.contains("schema"))
        }
        _ => false,
    }
}

#[async_trait]
impl Provider for OpenAiCompat {
    fn name(&self) -> &'static str {
        "openai-compat"
    }

    async fn think(&self, req: &ThinkRequest) -> Result<ThinkReply, AiError> {
        let mut last = match self.attempt(req, SchemaMode::JsonSchema).await {
            Ok(reply) => return Ok(reply),
            Err(e) => e,
        };
        for mode in [SchemaMode::JsonObject, SchemaMode::Freeform] {
            if !is_schema_rejection(&last) {
                break;
            }
            tracing::debug!("{} rejected the reply schema, retrying as {mode:?}", self.model);
            match self.attempt(req, mode).await {
                Ok(reply) => return Ok(reply),
                Err(e) => last = e,
            }
        }
        Err(last)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req() -> ThinkRequest {
        ThinkRequest {
            system: "You are a pirate.".into(),
            event: "self_talk".into(),
            emotions: vec!["idle".into(), "talk".into()],
            ..Default::default()
        }
    }

    #[test]
    fn endpoint_tolerates_a_trailing_slash() {
        let a = OpenAiCompat::new("http://localhost:11434/v1/".into(), String::new(), "m".into());
        let b = OpenAiCompat::new("http://localhost:11434/v1".into(), String::new(), "m".into());
        assert_eq!(a.endpoint(), b.endpoint());
        assert_eq!(a.endpoint(), "http://localhost:11434/v1/chat/completions");
    }

    #[test]
    fn system_prompt_is_the_first_message() {
        let p = OpenAiCompat::new("http://x/v1".into(), String::new(), "m".into());
        let b = p.body(&req(), SchemaMode::JsonSchema);
        let msgs = b["messages"].as_array().unwrap();
        assert_eq!(msgs[0]["role"], "system");
        assert!(msgs[0]["content"].as_str().unwrap().contains("pirate"));
        assert_eq!(msgs.last().unwrap()["role"], "user");
    }

    #[test]
    fn carries_anti_repeat_penalties_for_local_models() {
        let p = OpenAiCompat::new("http://x/v1".into(), String::new(), "m".into());
        let b = p.body(&req(), SchemaMode::JsonSchema);
        assert!(b["frequency_penalty"].as_f64().unwrap() > 0.0);
        assert!(b["presence_penalty"].as_f64().unwrap() > 0.0);
    }

    #[test]
    fn requests_a_schema_constrained_reply() {
        let p = OpenAiCompat::new("http://x/v1".into(), String::new(), "m".into());
        let b = p.body(&req(), SchemaMode::JsonSchema);
        assert_eq!(b["response_format"]["type"], "json_schema");
        let e = &b["response_format"]["json_schema"]["schema"]["properties"]["emotion"]["enum"];
        assert_eq!(e.as_array().unwrap().len(), 2);
    }

    #[test]
    fn degrades_to_plain_json_mode_then_to_freeform() {
        let p = OpenAiCompat::new("http://x/v1".into(), String::new(), "m".into());
        assert_eq!(
            p.body(&req(), SchemaMode::JsonObject)["response_format"]["type"],
            "json_object"
        );
        assert!(
            p.body(&req(), SchemaMode::Freeform).get("response_format").is_none(),
            "freeform must not send response_format at all"
        );
    }

    #[test]
    fn recognises_a_schema_rejection() {
        // The uncensored models that suit an edgy character often support
        // response_format but not json_schema, and say so in the 400 body.
        let e = AiError::Status {
            provider: "openai-compat",
            status: 400,
            body: "json_schema is not supported by this model".into(),
        };
        assert!(is_schema_rejection(&e));
    }

    #[test]
    fn does_not_retry_on_unrelated_failures() {
        // Retrying an auth or rate-limit failure three times is just noise.
        for (status, body) in [(401u16, "invalid api key"), (429, "rate limited")] {
            let e = AiError::Status { provider: "openai-compat", status, body: body.into() };
            assert!(!is_schema_rejection(&e), "{status} should not retry");
        }
        assert!(!is_schema_rejection(&AiError::MissingKey("openai-compat")));
    }
}
