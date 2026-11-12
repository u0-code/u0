//! Anthropic Messages API.

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::{Brain, Completion, Error, Reply, Result, Role};

pub const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";
const VERSION: &str = "2023-06-01";

pub struct Anthropic {
    client: reqwest::Client,
    api_key: String,
    model: String,
    base_url: String,
}

impl Anthropic {
    pub fn new(api_key: String, model: String, base_url: Option<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key,
            model,
            base_url: base_url
                .unwrap_or_else(|| DEFAULT_BASE_URL.to_owned())
                .trim_end_matches('/')
                .to_owned(),
        }
    }
}

#[async_trait]
impl Brain for Anthropic {
    fn label(&self) -> String {
        format!("anthropic/{}", self.model)
    }

    async fn complete(&self, req: &Completion) -> Result<Reply> {
        let messages: Vec<Value> = req
            .messages
            .iter()
            .map(|m| {
                json!({
                    "role": match m.role { Role::User => "user", Role::Assistant => "assistant" },
                    "content": m.text,
                })
            })
            .collect();
        let mut body = json!({
            "model": self.model,
            "max_tokens": req.max_tokens,
            "system": req.system,
            "messages": messages,
        });
        if let Some(t) = req.temperature {
            body["temperature"] = json!(t);
        }

        let resp = self
            .client
            .post(format!("{}/v1/messages", self.base_url))
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", VERSION)
            .json(&body)
            .send()
            .await?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            return Err(Error::Api {
                provider: "anthropic".into(),
                status: status.as_u16(),
                body: text,
            });
        }

        let v: Value = serde_json::from_str(&text)?;
        let out = v["content"]
            .as_array()
            .map(|parts| {
                parts
                    .iter()
                    .filter_map(|p| p["text"].as_str())
                    .collect::<Vec<_>>()
                    .join("")
            })
            .unwrap_or_default();
        if out.trim().is_empty() {
            return Err(Error::Malformed {
                provider: "anthropic".into(),
                detail: format!("no text content in {text}"),
            });
        }
        Ok(Reply {
            text: out.trim().to_owned(),
            model: v["model"].as_str().unwrap_or(&self.model).to_owned(),
            input_tokens: v["usage"]["input_tokens"].as_u64(),
            output_tokens: v["usage"]["output_tokens"].as_u64(),
        })
    }
}
