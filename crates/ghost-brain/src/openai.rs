//! `OpenAI`-compatible `/chat/completions`. Covers `OpenAI` itself and everything
//! that imitates it: `OpenRouter`, `DeepSeek`, Moonshot, Groq, Ollama, vLLM.

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::{Brain, Completion, Error, Reply, Result, Role};

pub const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";

pub struct OpenAi {
    client: reqwest::Client,
    api_key: String,
    model: String,
    base_url: String,
    /// `max_completion_tokens` for `OpenAI`'s newer models, `max_tokens` for
    /// most compatible servers. Chosen from the base URL unless overridden.
    max_tokens_param: String,
}

impl OpenAi {
    pub fn new(
        api_key: String,
        model: String,
        base_url: Option<String>,
        max_tokens_param: Option<String>,
    ) -> Self {
        let base_url = base_url
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_owned())
            .trim_end_matches('/')
            .to_owned();
        let max_tokens_param = max_tokens_param.unwrap_or_else(|| {
            if base_url.contains("api.openai.com") {
                "max_completion_tokens".to_owned()
            } else {
                "max_tokens".to_owned()
            }
        });
        Self {
            client: reqwest::Client::new(),
            api_key,
            model,
            base_url,
            max_tokens_param,
        }
    }
}

#[async_trait]
impl Brain for OpenAi {
    fn label(&self) -> String {
        format!("openai/{}", self.model)
    }

    async fn complete(&self, req: &Completion) -> Result<Reply> {
        let mut messages = vec![json!({"role": "system", "content": req.system})];
        messages.extend(req.messages.iter().map(|m| {
            json!({
                "role": match m.role { Role::User => "user", Role::Assistant => "assistant" },
                "content": m.text,
            })
        }));
        let mut body = json!({
            "model": self.model,
            "messages": messages,
        });
        body[&self.max_tokens_param] = json!(req.max_tokens);
        if let Some(t) = req.temperature {
            body["temperature"] = json!(t);
        }

        let resp = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            return Err(Error::Api {
                provider: "openai".into(),
                status: status.as_u16(),
                body: text,
            });
        }

        let v: Value = serde_json::from_str(&text)?;
        let out = v["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or_default()
            .trim()
            .to_owned();
        if out.is_empty() {
            return Err(Error::Malformed {
                provider: "openai".into(),
                detail: format!("no message content in {text}"),
            });
        }
        Ok(Reply {
            text: out,
            model: v["model"].as_str().unwrap_or(&self.model).to_owned(),
            input_tokens: v["usage"]["prompt_tokens"].as_u64(),
            output_tokens: v["usage"]["completion_tokens"].as_u64(),
        })
    }
}
