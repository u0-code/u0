//! `OpenAI`-compatible `/chat/completions`. Covers `OpenAI` itself and everything
//! that imitates it: `OpenRouter`, `DeepSeek`, Moonshot, Groq, Ollama, vLLM.

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::{
    Brain, Call, Completion, Error, Item, Reply, Result, Role, Step, Stop, Turn, Usage, http,
};

pub const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";

pub struct OpenAi {
    client: reqwest::Client,
    api_key: String,
    model: String,
    base_url: String,
    /// `max_completion_tokens` for `OpenAI`'s newer models, `max_tokens` for
    /// most compatible servers. Chosen from the base URL unless overridden.
    max_tokens_param: String,
    /// Send `response_format: json_object` when the caller expects JSON.
    json_mode: bool,
}

impl OpenAi {
    pub fn new(
        api_key: String,
        model: String,
        base_url: Option<String>,
        max_tokens_param: Option<String>,
        json_mode: bool,
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
            client: http::client(),
            api_key,
            model,
            base_url,
            max_tokens_param,
            json_mode,
        }
    }

    async fn post(&self, body: &Value) -> Result<(Value, String)> {
        http::send("openai", || {
            self.client
                .post(format!("{}/chat/completions", self.base_url))
                .bearer_auth(&self.api_key)
                .json(body)
        })
        .await
    }
}

/// A working transcript in the wire format.
fn wire(system: &str, items: &[Item]) -> Vec<Value> {
    let mut messages = vec![json!({"role": "system", "content": system})];
    for item in items {
        match item {
            Item::User { text } => messages.push(json!({"role": "user", "content": text})),
            Item::Assistant { text, calls, carry } => {
                let mut m = json!({"role": "assistant", "content": text});
                if !calls.is_empty() {
                    m["tool_calls"] = calls
                        .iter()
                        .map(|c| {
                            // A call whose arguments were not JSON goes back
                            // as the text the model wrote.
                            let arguments = match &c.args {
                                Value::String(raw) => raw.clone(),
                                other => other.to_string(),
                            };
                            json!({
                                "id": c.id,
                                "type": "function",
                                "function": {"name": c.name, "arguments": arguments},
                            })
                        })
                        .collect();
                }
                // Servers that think out loud want their thinking handed
                // back while a run of tool calls is in progress.
                if let Some(Value::Object(extra)) = carry {
                    for (k, v) in extra {
                        m[k] = v.clone();
                    }
                }
                messages.push(m);
            }
            Item::Result { id, output, .. } => {
                messages.push(json!({"role": "tool", "tool_call_id": id, "content": output}));
            }
        }
    }
    messages
}

fn usage(v: &Value) -> Usage {
    let u = &v["usage"];
    Usage {
        input: u["prompt_tokens"].as_u64().unwrap_or(0),
        // DeepSeek reports cache hits at the top level; OpenAI nests them.
        cached: u["prompt_cache_hit_tokens"]
            .as_u64()
            .or_else(|| u["prompt_tokens_details"]["cached_tokens"].as_u64())
            .unwrap_or(0),
        output: u["completion_tokens"].as_u64().unwrap_or(0),
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
        if req.json && self.json_mode {
            body["response_format"] = json!({"type": "json_object"});
        }
        if let Some(t) = req.temperature {
            body["temperature"] = json!(t);
        }

        let (v, text) = self.post(&body).await?;
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

    fn works(&self) -> bool {
        true
    }

    async fn step(&self, turn: &Turn) -> Result<Step> {
        let mut body = json!({
            "model": self.model,
            "messages": wire(&turn.system, &turn.items),
        });
        body[&self.max_tokens_param] = json!(turn.max_tokens);
        if !turn.tools.is_empty() {
            body["tools"] = turn
                .tools
                .iter()
                .map(|t| {
                    json!({
                        "type": "function",
                        "function": {
                            "name": t.name,
                            "description": t.description,
                            "parameters": t.parameters,
                        },
                    })
                })
                .collect();
        }
        if let Some(t) = turn.temperature {
            body["temperature"] = json!(t);
        }

        let (v, text) = self.post(&body).await?;
        let choice = &v["choices"][0];
        let message = &choice["message"];
        if !message.is_object() {
            return Err(Error::Malformed {
                provider: "openai".into(),
                detail: format!("no message in {text}"),
            });
        }
        let calls: Vec<Call> = message["tool_calls"]
            .as_array()
            .map(|calls| {
                calls
                    .iter()
                    .enumerate()
                    .map(|(i, c)| {
                        let raw = c["function"]["arguments"].as_str().unwrap_or("{}");
                        let raw = if raw.trim().is_empty() { "{}" } else { raw };
                        Call {
                            id: c["id"]
                                .as_str()
                                .map_or_else(|| format!("call_{i}"), str::to_owned),
                            name: c["function"]["name"]
                                .as_str()
                                .unwrap_or_default()
                                .to_owned(),
                            args: serde_json::from_str(raw)
                                .unwrap_or_else(|_| Value::String(raw.to_owned())),
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        let stop = if !calls.is_empty() {
            Stop::Calls
        } else if choice["finish_reason"].as_str() == Some("length") {
            Stop::Length
        } else {
            Stop::End
        };
        let carry = message
            .get("reasoning_content")
            .filter(|r| r.as_str().is_some_and(|s| !s.is_empty()))
            .map(|r| json!({"reasoning_content": r}));
        Ok(Step {
            text: message["content"]
                .as_str()
                .unwrap_or_default()
                .trim()
                .to_owned(),
            calls,
            stop,
            usage: usage(&v),
            model: v["model"].as_str().unwrap_or(&self.model).to_owned(),
            carry,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_transcript_goes_on_the_wire_in_order() {
        let items = vec![
            Item::user("list the files"),
            Item::Assistant {
                text: String::new(),
                calls: vec![Call {
                    id: "c1".into(),
                    name: "shell".into(),
                    args: json!({"command": "ls"}),
                }],
                carry: Some(json!({"reasoning_content": "they want a listing"})),
            },
            Item::Result {
                id: "c1".into(),
                name: "shell".into(),
                output: "a.txt".into(),
                failed: false,
            },
        ];
        let m = wire("sys", &items);
        assert_eq!(m.len(), 4);
        assert_eq!(m[0]["role"], "system");
        assert_eq!(
            m[2]["tool_calls"][0]["function"]["arguments"],
            r#"{"command":"ls"}"#
        );
        assert_eq!(m[2]["reasoning_content"], "they want a listing");
        assert_eq!(m[3]["role"], "tool");
        assert_eq!(m[3]["tool_call_id"], "c1");
    }

    #[test]
    fn cache_hits_are_read_from_either_shape() {
        let deepseek = json!({"usage": {"prompt_tokens": 100, "prompt_cache_hit_tokens": 80, "completion_tokens": 5}});
        let openai = json!({"usage": {"prompt_tokens": 100, "prompt_tokens_details": {"cached_tokens": 60}, "completion_tokens": 5}});
        assert_eq!(usage(&deepseek).cached, 80);
        assert_eq!(usage(&openai).cached, 60);
    }
}
