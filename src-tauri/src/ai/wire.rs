//! Request bodies, reply parsing and error mapping for each provider's HTTP API (pure; the call
//! itself lives in `ai::chat_json`).
use serde_json::{json, Value};

use super::{ChatOpts, Provider};
use crate::error::{AppError, AppResult};

pub const ANTHROPIC_URL: &str = "https://api.anthropic.com/v1/messages";
pub const ANTHROPIC_VERSION: &str = "2023-06-01";
pub const OPENAI_URL: &str = "https://api.openai.com/v1/chat/completions";
pub const GEMINI_URL: &str = "https://generativelanguage.googleapis.com/v1beta/interactions";
/// Thinking/reasoning tokens count toward the output limit on current cloud models.
pub const THINKING_HEADROOM: u32 = 4096;

pub struct Target<'a> {
    pub provider: Provider,
    pub model: &'a str,
    pub effort: Option<&'static str>,
    pub base_url: Option<&'a str>,
    pub key: Option<&'a str>,
}

pub struct Request {
    pub url: String,
    pub headers: Vec<(&'static str, String)>,
    pub body: Value,
}

fn messages(system: &str, user: &str) -> Value {
    json!([
        { "role": "system", "content": system },
        { "role": "user", "content": user },
    ])
}

fn json_schema_format(opts: &ChatOpts, schema: &Value) -> Value {
    json!({
        "type": "json_schema",
        "json_schema": { "name": opts.name, "strict": true, "schema": schema },
    })
}

/// llama-server (model None), Ollama, LM Studio: the current llama-server body.
pub fn chat_body(model: Option<&str>, opts: &ChatOpts, system: &str, user: &str, schema: &Value) -> Value {
    let mut body = json!({
        "messages": messages(system, user),
        "temperature": 0.2,
        "max_tokens": opts.max_tokens,
        "response_format": json_schema_format(opts, schema),
    });
    if let Some(m) = model {
        body["model"] = json!(m);
    }
    body
}

/// The HTTP request for `t`; `schema` is already converted with `schema::for_provider`.
/// An empty or absent effort omits the effort parameter.
pub fn request(t: &Target<'_>, opts: &ChatOpts, system: &str, user: &str, schema: &Value) -> Request {
    let key = t.key.unwrap_or_default().to_owned();
    let effort = t.effort.filter(|e| !e.is_empty());
    let max = opts.max_tokens + THINKING_HEADROOM;
    match t.provider {
        Provider::Local | Provider::Ollama | Provider::LmStudio => Request {
            url: format!("{}/v1/chat/completions", t.base_url.unwrap_or_default()),
            headers: Vec::new(),
            body: chat_body(Some(t.model), opts, system, user, schema),
        },
        Provider::OpenAi => {
            let mut body = json!({
                "model": t.model,
                "messages": messages(system, user),
                "max_completion_tokens": max,
                "store": false,
                "response_format": json_schema_format(opts, schema),
            });
            if let Some(e) = effort {
                body["reasoning_effort"] = json!(e);
            }
            Request { url: OPENAI_URL.into(), headers: vec![("Authorization", format!("Bearer {key}"))], body }
        }
        Provider::Anthropic => {
            let mut output = json!({ "format": { "type": "json_schema", "schema": schema } });
            if let Some(e) = effort {
                output["effort"] = json!(e);
            }
            Request {
                url: ANTHROPIC_URL.into(),
                headers: vec![("x-api-key", key), ("anthropic-version", ANTHROPIC_VERSION.into())],
                body: json!({
                    "model": t.model,
                    "max_tokens": max,
                    "system": system,
                    "messages": [{ "role": "user", "content": user }],
                    "output_config": output,
                }),
            }
        }
        Provider::Gemini => {
            let mut generation = json!({ "max_output_tokens": max });
            if let Some(e) = effort {
                generation["thinking_level"] = json!(e);
            }
            Request {
                url: GEMINI_URL.into(),
                headers: vec![("x-goog-api-key", key)],
                body: json!({
                    "model": t.model,
                    "system_instruction": system,
                    "input": user,
                    "store": false,
                    "response_format": { "type": "text", "mime_type": "application/json", "schema": schema },
                    "generation_config": generation,
                }),
            }
        }
    }
}

/// The JSON text the model wrote, from a 2xx reply; refusal / truncation / no text → AppError::Msg.
pub fn reply_text(p: Provider, reply: &Value) -> AppResult<String> {
    let name = p.as_str();
    match p {
        Provider::Local | Provider::OpenAi | Provider::Ollama | Provider::LmStudio => {
            let choice = &reply["choices"][0];
            if let Some(r) = choice["message"]["refusal"].as_str() {
                return Err(AppError::msg(format!("{name} refused: {r}")));
            }
            if choice["finish_reason"] == "length" {
                return Err(AppError::msg(format!("{name} reply was cut off at the token limit")));
            }
            choice["message"]["content"]
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| AppError::msg(format!("{name} reply has no content")))
        }
        Provider::Anthropic => {
            match reply["stop_reason"].as_str() {
                Some("refusal") => return Err(AppError::msg("anthropic refused")),
                Some("max_tokens") => return Err(AppError::msg("anthropic reply was cut off at the token limit")),
                _ => {}
            }
            reply["content"]
                .as_array()
                .and_then(|blocks| blocks.iter().find(|b| b["type"] == "text"))
                .and_then(|b| b["text"].as_str())
                .map(str::to_owned)
                .ok_or_else(|| AppError::msg("anthropic reply has no text"))
        }
        Provider::Gemini => {
            if reply["status"] != "completed" {
                let status = reply["status"].as_str().unwrap_or("unknown");
                return Err(AppError::msg(format!("gemini interaction ended with status {status}")));
            }
            let text: String = reply["steps"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|s| s["type"] == "model_output")
                .flat_map(|s| s["content"].as_array().into_iter().flatten())
                .filter(|c| c["type"] == "text")
                .filter_map(|c| c["text"].as_str())
                .collect();
            if text.is_empty() {
                return Err(AppError::msg("gemini reply has no text"));
            }
            Ok(text)
        }
    }
}

/// App error code for a non-2xx reply.
pub fn status_code(p: Provider, status: u16, body: &str) -> &'static str {
    match status {
        401 | 403 => "unauthorized",
        400 if p == Provider::Gemini && body.contains("API_KEY_INVALID") => "unauthorized",
        404 => "aiModel",
        429 => "rateLimited",
        500..=599 => "aiUnavailable",
        _ => "aiRequest",
    }
}

/// App error code for a request that never got a reply.
pub fn transport_code(p: Provider) -> &'static str {
    if p.is_server() { "serverUnreachable" } else { "network" }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    const OPTS: ChatOpts = ChatOpts { name: "triage", max_tokens: 400, timeout: Duration::from_secs(120) };

    fn schema() -> Value {
        json!({ "type": "object", "additionalProperties": false, "required": ["ok"], "properties": { "ok": { "type": "boolean" } } })
    }

    fn target<'a>(provider: Provider, model: &'a str, effort: Option<&'static str>) -> Target<'a> {
        Target { provider, model, effort, base_url: None, key: Some("sk-test") }
    }

    #[test]
    fn server_body_matches_llama_server() {
        let s = schema();
        let old = json!({
            "messages": [
                { "role": "system", "content": "sys" },
                { "role": "user", "content": "usr" },
            ],
            "temperature": 0.2,
            "max_tokens": OPTS.max_tokens,
            "response_format": {
                "type": "json_schema",
                "json_schema": { "name": OPTS.name, "strict": true, "schema": s },
            },
        });
        assert_eq!(chat_body(None, &OPTS, "sys", "usr", &s), old);
        let mut with_model = old.clone();
        with_model["model"] = json!("qwen3:4b");
        assert_eq!(chat_body(Some("qwen3:4b"), &OPTS, "sys", "usr", &s), with_model);

        let t = Target { provider: Provider::Ollama, model: "qwen3:4b", effort: None, base_url: Some("http://localhost:11434"), key: None };
        let r = request(&t, &OPTS, "sys", "usr", &s);
        assert_eq!(r.url, "http://localhost:11434/v1/chat/completions");
        assert!(r.headers.is_empty());
        assert_eq!(r.body, with_model);
    }

    #[test]
    fn openai_request_shape() {
        let r = request(&target(Provider::OpenAi, "gpt-6-luna", Some("none")), &OPTS, "sys", "usr", &schema());
        assert_eq!(r.url, OPENAI_URL);
        assert_eq!(r.headers, vec![("Authorization", "Bearer sk-test".to_owned())]);
        let b = &r.body;
        assert_eq!(b["model"], "gpt-6-luna");
        assert_eq!(b["messages"], messages("sys", "usr"));
        assert_eq!(b["max_completion_tokens"], json!(400 + 4096));
        assert_eq!(b["reasoning_effort"], "none");
        assert_eq!(b["store"], json!(false));
        assert_eq!(b["response_format"]["type"], "json_schema");
        assert_eq!(b["response_format"]["json_schema"]["strict"], json!(true));
        assert_eq!(b["response_format"]["json_schema"]["schema"], schema());
        assert!(b.get("temperature").is_none() && b.get("max_tokens").is_none());
        // An empty effort is omitted.
        let r = request(&target(Provider::OpenAi, "gpt-6-luna", Some("")), &OPTS, "sys", "usr", &schema());
        assert!(r.body.get("reasoning_effort").is_none());
    }

    #[test]
    fn anthropic_request_shape() {
        let r = request(&target(Provider::Anthropic, "claude-haiku-5-5", Some("low")), &OPTS, "sys", "usr", &schema());
        assert_eq!(r.url, ANTHROPIC_URL);
        assert_eq!(
            r.headers,
            vec![("x-api-key", "sk-test".to_owned()), ("anthropic-version", "2023-06-01".to_owned())]
        );
        let b = &r.body;
        assert_eq!(b["max_tokens"], json!(400 + 4096));
        assert_eq!(b["system"], "sys");
        assert_eq!(b["messages"], json!([{ "role": "user", "content": "usr" }]));
        assert_eq!(b["output_config"]["format"]["type"], "json_schema");
        assert_eq!(b["output_config"]["format"]["schema"], schema());
        assert_eq!(b["output_config"]["effort"], "low");
        assert!(b.get("temperature").is_none() && b.get("thinking").is_none());
    }

    #[test]
    fn gemini_request_shape() {
        let r = request(&target(Provider::Gemini, "gemini-3.5-flash-lite", Some("minimal")), &OPTS, "sys", "usr", &schema());
        assert_eq!(r.url, GEMINI_URL);
        assert_eq!(r.headers, vec![("x-goog-api-key", "sk-test".to_owned())]);
        let b = &r.body;
        assert_eq!(b["model"], "gemini-3.5-flash-lite");
        assert_eq!(b["system_instruction"], "sys");
        assert_eq!(b["input"], "usr");
        assert_eq!(b["store"], json!(false));
        assert_eq!(b["response_format"], json!({ "type": "text", "mime_type": "application/json", "schema": schema() }));
        assert_eq!(b["generation_config"], json!({ "max_output_tokens": 4496, "thinking_level": "minimal" }));
    }

    #[test]
    fn parse_openai_refusal_and_length_are_errors() {
        let ok = json!({ "choices": [{ "message": { "content": "{\"ok\":true}", "refusal": null }, "finish_reason": "stop" }] });
        assert_eq!(reply_text(Provider::OpenAi, &ok).unwrap(), "{\"ok\":true}");
        assert_eq!(reply_text(Provider::Local, &ok).unwrap(), "{\"ok\":true}");
        let refused = json!({ "choices": [{ "message": { "content": null, "refusal": "I can't help." }, "finish_reason": "stop" }] });
        assert!(matches!(reply_text(Provider::OpenAi, &refused), Err(AppError::Msg(_))));
        let cut = json!({ "choices": [{ "message": { "content": "{\"ok\":" }, "finish_reason": "length" }] });
        assert!(matches!(reply_text(Provider::Ollama, &cut), Err(AppError::Msg(_))));
        assert!(matches!(reply_text(Provider::LmStudio, &json!({})), Err(AppError::Msg(_))));
    }

    #[test]
    fn parse_anthropic_takes_first_text_block() {
        let reply: Value = serde_json::from_str(
            r#"{"content":[{"type":"thinking","thinking":""},{"type":"text","text":"{\"ok\":true}"}],"stop_reason":"end_turn"}"#,
        )
        .unwrap();
        assert_eq!(reply_text(Provider::Anthropic, &reply).unwrap(), "{\"ok\":true}");
        assert!(matches!(reply_text(Provider::Anthropic, &json!({ "content": [], "stop_reason": "end_turn" })), Err(AppError::Msg(_))));
    }

    #[test]
    fn parse_anthropic_refusal_is_error() {
        for stop in ["refusal", "max_tokens"] {
            let reply = json!({ "content": [{ "type": "text", "text": "{}" }], "stop_reason": stop });
            assert!(matches!(reply_text(Provider::Anthropic, &reply), Err(AppError::Msg(_))), "{stop}");
        }
    }

    #[test]
    fn parse_gemini_concatenates_model_output_text() {
        let reply = json!({
            "status": "completed",
            "steps": [
                { "type": "thought", "content": [{ "type": "text", "text": "hmm" }] },
                { "type": "model_output", "content": [{ "type": "text", "text": "{\"ok\":" }, { "type": "image" }] },
                { "type": "model_output", "content": [{ "type": "text", "text": "true}" }] },
            ],
        });
        assert_eq!(reply_text(Provider::Gemini, &reply).unwrap(), "{\"ok\":true}");
    }

    #[test]
    fn parse_gemini_incomplete_is_error() {
        let reply = json!({
            "status": "incomplete",
            "steps": [{ "type": "model_output", "content": [{ "type": "text", "text": "{\"ok\":" }] }],
        });
        assert!(matches!(reply_text(Provider::Gemini, &reply), Err(AppError::Msg(_))));
        assert!(matches!(reply_text(Provider::Gemini, &json!({ "status": "completed", "steps": [] })), Err(AppError::Msg(_))));
    }

    #[test]
    fn status_codes_map_to_app_codes() {
        for p in std::iter::once(Provider::Local).chain(Provider::CLOUD) {
            assert_eq!(status_code(p, 401, ""), "unauthorized");
            assert_eq!(status_code(p, 403, ""), "unauthorized");
            assert_eq!(status_code(p, 404, ""), "aiModel");
            assert_eq!(status_code(p, 429, ""), "rateLimited");
            assert_eq!(status_code(p, 500, ""), "aiUnavailable");
            assert_eq!(status_code(p, 503, ""), "aiUnavailable");
            assert_eq!(status_code(p, 599, ""), "aiUnavailable");
            assert_eq!(status_code(p, 400, "bad request"), "aiRequest");
            assert_eq!(status_code(p, 422, ""), "aiRequest");
        }
        assert_eq!(status_code(Provider::Anthropic, 529, "overloaded_error"), "aiUnavailable");
        let invalid_key = r#"{"error":{"code":400,"status":"INVALID_ARGUMENT","details":[{"reason":"API_KEY_INVALID"}]}}"#;
        assert_eq!(status_code(Provider::Gemini, 400, invalid_key), "unauthorized");
        assert_eq!(status_code(Provider::OpenAi, 400, invalid_key), "aiRequest");
        assert_eq!(transport_code(Provider::Ollama), "serverUnreachable");
        assert_eq!(transport_code(Provider::LmStudio), "serverUnreachable");
        assert_eq!(transport_code(Provider::OpenAi), "network");
        assert_eq!(transport_code(Provider::Local), "network");
    }
}
