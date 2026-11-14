//! Anthropic Messages API.

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::{
    Brain, Call, Completion, Error, Item, Reply, Result, Role, Step, Stop, Turn, Usage, http,
};

pub const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";
const VERSION: &str = "2023-06-01";

pub struct Anthropic {
    client: reqwest::Client,
    api_key: String,
    model: String,
    base_url: String,
    window: Option<u64>,
    /// The model takes images.
    sees: bool,
}

impl Anthropic {
    pub fn new(api_key: String, model: String, base_url: Option<String>) -> Self {
        Self {
            client: http::client(),
            api_key,
            window: crate::known_window(&model),
            model,
            base_url: base_url
                .unwrap_or_else(|| DEFAULT_BASE_URL.to_owned())
                .trim_end_matches('/')
                .to_owned(),
            sees: false,
        }
    }

    /// Say whether the model takes images.
    #[must_use]
    pub fn with_vision(mut self, sees: bool) -> Self {
        self.sees = sees;
        self
    }

    /// Say how large this model's context window is, when it is not one
    /// this crate knows.
    #[must_use]
    pub fn with_window(mut self, window: Option<u64>) -> Self {
        self.window = window.or(self.window);
        self
    }

    async fn post(&self, body: &Value) -> Result<(Value, String)> {
        http::send("anthropic", || {
            self.client
                .post(format!("{}/v1/messages", self.base_url))
                .header("x-api-key", &self.api_key)
                .header("anthropic-version", VERSION)
                .json(body)
        })
        .await
    }
}

/// A completion's messages in the wire format. A message with images, sent
/// to a model that sees, is a list of blocks: each image, base64, then the
/// text, the order Anthropic's guidance gives.
fn chat(req: &Completion, sees: bool) -> Vec<Value> {
    req.messages
        .iter()
        .map(|m| {
            let role = match m.role {
                Role::User => "user",
                Role::Assistant => "assistant",
            };
            if !sees || m.images.is_empty() {
                return json!({"role": role, "content": m.text});
            }
            let mut blocks: Vec<Value> = m.images.iter().map(image).collect();
            blocks.push(json!({"type": "text", "text": m.text}));
            json!({"role": role, "content": blocks})
        })
        .collect()
}

/// A working transcript in the wire format. Tool results are user turns, and
/// neighbouring turns of the same role are merged, as the API requires.
fn wire(items: &[Item], sees: bool) -> Vec<Value> {
    let mut messages: Vec<Value> = Vec::new();
    let mut push = |role: &str, blocks: Vec<Value>| {
        if let Some(last) = messages.last_mut()
            && last["role"] == role
            && let Some(content) = last["content"].as_array_mut()
        {
            content.extend(blocks);
            return;
        }
        messages.push(json!({"role": role, "content": blocks}));
    };
    for item in items {
        match item {
            Item::User { text } => push("user", vec![json!({"type": "text", "text": text})]),
            Item::Assistant { text, calls, carry } => {
                let mut blocks = Vec::new();
                // Thinking blocks come first and go back exactly as received.
                if let Some(Value::Array(kept)) = carry {
                    blocks.extend(kept.iter().cloned());
                }
                if !text.is_empty() {
                    blocks.push(json!({"type": "text", "text": text}));
                }
                for c in calls {
                    let input = if c.args.is_object() {
                        c.args.clone()
                    } else {
                        json!({})
                    };
                    blocks.push(
                        json!({"type": "tool_use", "id": c.id, "name": c.name, "input": input}),
                    );
                }
                if blocks.is_empty() {
                    blocks.push(json!({"type": "text", "text": "(nothing)"}));
                }
                push("assistant", blocks);
            }
            // Pictures go inside the result they came with, after its text.
            // A model that cannot see is told how many there were instead.
            Item::Result {
                id,
                output,
                failed,
                pictures,
                ..
            } => {
                let content = if pictures.is_empty() {
                    json!(output)
                } else if !sees {
                    json!(format!("{output}\n{}", crate::not_shown(pictures.len())))
                } else {
                    let mut blocks = vec![json!({"type": "text", "text": output})];
                    blocks.extend(pictures.iter().map(image));
                    Value::Array(blocks)
                };
                push(
                    "user",
                    vec![json!({
                        "type": "tool_result",
                        "tool_use_id": id,
                        "content": content,
                        "is_error": failed,
                    })],
                );
            }
        }
    }
    messages
}

fn image(p: &crate::Picture) -> Value {
    json!({
        "type": "image",
        "source": {"type": "base64", "media_type": p.mime, "data": p.base64()},
    })
}

#[async_trait]
impl Brain for Anthropic {
    fn label(&self) -> String {
        format!("anthropic/{}", self.model)
    }

    async fn complete(&self, req: &Completion) -> Result<Reply> {
        let mut body = json!({
            "model": self.model,
            "max_tokens": req.max_tokens,
            "system": req.system,
            "messages": chat(req, self.sees),
        });
        if let Some(t) = req.temperature {
            body["temperature"] = json!(t);
        }

        let (v, text) = self.post(&body).await?;
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

    fn works(&self) -> bool {
        true
    }

    fn sees(&self) -> bool {
        self.sees
    }

    fn window(&self) -> Option<u64> {
        self.window
    }

    fn max_output(&self) -> Option<u32> {
        crate::known_max_output(&self.model)
    }

    async fn step(&self, turn: &Turn) -> Result<Step> {
        let mut messages = wire(&turn.items, self.sees);
        // Cache everything up to the newest message: a loop resends the same
        // prefix every step, and this is what makes that affordable.
        if let Some(block) = messages
            .last_mut()
            .and_then(|m| m["content"].as_array_mut())
            .and_then(|c| c.last_mut())
        {
            block["cache_control"] = json!({"type": "ephemeral"});
        }
        let mut body = json!({
            "model": self.model,
            "max_tokens": turn.max_tokens,
            "system": [{"type": "text", "text": turn.system, "cache_control": {"type": "ephemeral"}}],
            "messages": messages,
        });
        if !turn.tools.is_empty() {
            body["tools"] = turn
                .tools
                .iter()
                .map(|t| json!({"name": t.name, "description": t.description, "input_schema": t.parameters}))
                .collect();
        }
        if let Some(t) = turn.temperature {
            body["temperature"] = json!(t);
        }

        let (v, text) = self.post(&body).await?;
        let Some(blocks) = v["content"].as_array() else {
            return Err(Error::Malformed {
                provider: "anthropic".into(),
                detail: format!("no content in {text}"),
            });
        };
        let mut said = String::new();
        let mut calls = Vec::new();
        let mut kept = Vec::new();
        for block in blocks {
            match block["type"].as_str() {
                Some("text") => said.push_str(block["text"].as_str().unwrap_or_default()),
                Some("tool_use") => calls.push(Call {
                    id: block["id"].as_str().unwrap_or_default().to_owned(),
                    name: block["name"].as_str().unwrap_or_default().to_owned(),
                    args: block["input"].clone(),
                }),
                Some("thinking" | "redacted_thinking") => kept.push(block.clone()),
                _ => {}
            }
        }
        let stop = match v["stop_reason"].as_str() {
            _ if !calls.is_empty() => Stop::Calls,
            Some("max_tokens") => Stop::Length,
            _ => Stop::End,
        };
        let u = &v["usage"];
        let cached = u["cache_read_input_tokens"].as_u64().unwrap_or(0);
        Ok(Step {
            text: said.trim().to_owned(),
            calls,
            stop,
            usage: Usage {
                // The API counts fresh, cache-written and cache-read input
                // separately; together they are what the model read.
                input: u["input_tokens"].as_u64().unwrap_or(0)
                    + u["cache_creation_input_tokens"].as_u64().unwrap_or(0)
                    + cached,
                cached,
                output: u["output_tokens"].as_u64().unwrap_or(0),
                thought: 0,
            },
            model: v["model"].as_str().unwrap_or(&self.model).to_owned(),
            carry: (!kept.is_empty()).then_some(Value::Array(kept)),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_image_goes_to_a_model_that_sees_as_a_base64_block() {
        let req = Completion {
            messages: vec![crate::Message::user("what is this?").with_images(vec![
                crate::Picture {
                    mime: "image/jpeg".into(),
                    bytes: vec![1, 2, 3],
                },
            ])],
            ..Completion::ask("look", "", 100)
        };
        let m = chat(&req, true);
        let blocks = m[0]["content"].as_array().unwrap();
        assert_eq!(
            blocks[0],
            json!({"type": "image", "source": {"type": "base64", "media_type": "image/jpeg", "data": "AQID"}})
        );
        assert_eq!(blocks[1], json!({"type": "text", "text": "what is this?"}));
        assert_eq!(chat(&req, false)[0]["content"], "what is this?");
    }

    #[test]
    fn results_merge_into_one_user_turn() {
        let call = |id: &str| Call {
            id: id.into(),
            name: "shell".into(),
            args: json!({"command": "ls"}),
        };
        let result = |id: &str| Item::result(id, "shell", "ok", false);
        let items = vec![
            Item::user("go"),
            Item::Assistant {
                text: "two things".into(),
                calls: vec![call("a"), call("b")],
                carry: None,
            },
            result("a"),
            result("b"),
            Item::user("and now?"),
        ];
        let m = wire(&items, true);
        assert_eq!(m.len(), 3, "user, assistant, user");
        assert_eq!(m[1]["content"].as_array().unwrap().len(), 3);
        let last = m[2]["content"].as_array().unwrap();
        assert_eq!(last.len(), 3, "two results and the follow-up share a turn");
        assert_eq!(last[0]["type"], "tool_result");
        assert_eq!(last[2]["type"], "text");
    }

    #[test]
    fn pictures_a_tool_returned_go_inside_its_result_for_a_model_that_sees() {
        let items = vec![
            Item::user("look"),
            Item::Assistant {
                text: String::new(),
                calls: vec![Call {
                    id: "a".into(),
                    name: "fetch".into(),
                    args: json!({}),
                }],
                carry: None,
            },
            Item::Result {
                id: "a".into(),
                name: "fetch".into(),
                output: "An image.".into(),
                failed: false,
                pictures: vec![crate::Picture {
                    mime: "image/gif".into(),
                    bytes: vec![1, 2, 3],
                }],
            },
        ];
        let seen = wire(&items, true);
        let content = seen[2]["content"][0]["content"].as_array().unwrap();
        assert_eq!(content[0], json!({"type": "text", "text": "An image."}));
        assert_eq!(content[1]["source"]["media_type"], "image/gif");
        assert_eq!(content[1]["source"]["data"], "AQID");

        let blind = wire(&items, false);
        assert_eq!(
            blind[2]["content"][0]["content"],
            "An image.\n[1 image, not shown: this model cannot see images]"
        );
    }
}
