//! `OpenAI`-compatible `/chat/completions`. Covers `OpenAI` itself and everything
//! that imitates it: `OpenRouter`, `DeepSeek`, Moonshot, Groq, Ollama, vLLM.

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::{
    Brain, Call, Completion, Effort, Error, Item, Reply, Result, Role, Step, Stop, Turn, Usage,
    http::{self, Answer},
};

pub const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";

pub(crate) struct OpenAi {
    client: reqwest::Client,
    api_key: String,
    model: String,
    base_url: String,
    /// `max_completion_tokens` for `OpenAI`'s newer models, `max_tokens` for
    /// most compatible servers. Chosen from the base URL unless overridden.
    max_tokens_param: String,
    /// Send `response_format: json_object` when the caller expects JSON.
    json_mode: bool,
    window: Option<u64>,
    max_output: Option<u32>,
    /// The server takes `reasoning_effort` and `thinking`.
    thinks: bool,
    /// The model takes images.
    sees: bool,
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
            window: crate::known_window(&model),
            max_output: crate::known_max_output(&model),
            thinks: crate::known_to_think(&model),
            model,
            base_url,
            max_tokens_param,
            json_mode,
            sees: false,
        }
    }

    /// Say whether the model takes images. Nothing is guessed from its name:
    /// a server that is sent an image it cannot read refuses the request.
    #[must_use]
    pub fn with_vision(mut self, sees: bool) -> Self {
        self.sees = sees;
        self
    }

    /// Say whether the server can be told how hard to think, when the
    /// model is not one this crate knows.
    #[must_use]
    pub fn with_effort(mut self, thinks: Option<bool>) -> Self {
        self.thinks = thinks.unwrap_or(self.thinks);
        self
    }

    /// Say the most this model writes in one answer, when it is not one
    /// this crate knows.
    #[must_use]
    pub fn with_max_output(mut self, max_output: Option<u32>) -> Self {
        self.max_output = max_output.or(self.max_output);
        self
    }

    /// Say how large this model's context window is, when it is not one
    /// this crate knows.
    #[must_use]
    pub fn with_window(mut self, window: Option<u64>) -> Self {
        self.window = window.or(self.window);
        self
    }

    /// Ask for a completion as a stream and put the answer back together:
    /// the message, why it stopped, what it cost and which model wrote it,
    /// in the shape of an answer that was not streamed.
    async fn post(&self, body: &mut Value, effort: Option<Effort>) -> Result<Value> {
        body["stream"] = json!(true);
        body["stream_options"] = json!({"include_usage": true});
        if self.thinks {
            match effort {
                Some(Effort::Off) => body["thinking"] = json!({"type": "disabled"}),
                Some(Effort::Low) => body["reasoning_effort"] = json!("low"),
                Some(Effort::High) => body["reasoning_effort"] = json!("high"),
                Some(Effort::Max) => body["reasoning_effort"] = json!("max"),
                None => {}
            }
        }
        let body = &*body;
        let answer = http::ask(
            "openai",
            || {
                self.client
                    .post(format!("{}/chat/completions", self.base_url))
                    .bearer_auth(&self.api_key)
                    .json(body)
            },
            // Finished when the model has said why it stopped.
            |events| {
                events
                    .iter()
                    .any(|e| e["choices"][0]["finish_reason"].is_string())
            },
        )
        .await?;
        match answer {
            Answer::Whole(v, _) if v["choices"][0]["message"].is_object() => Ok(v),
            Answer::Whole(_, text) => Err(Error::Malformed {
                provider: "openai".into(),
                detail: format!("no message in {text}"),
            }),
            Answer::Events(events) => Ok(gather(&events)),
        }
    }
}

/// Put the pieces of a streamed completion back together.
fn gather(events: &[Value]) -> Value {
    let mut content = String::new();
    let mut reasoning = String::new();
    // Calls arrive in fragments, each tagged with the index of its call.
    let mut calls: Vec<(String, String, String)> = Vec::new();
    let mut finish = Value::Null;
    let mut usage = Value::Null;
    let mut model = Value::Null;
    for event in events {
        if event["usage"].is_object() {
            usage = event["usage"].clone();
        }
        if model.is_null() && event["model"].is_string() {
            model = event["model"].clone();
        }
        let choice = &event["choices"][0];
        if choice["finish_reason"].is_string() {
            finish = choice["finish_reason"].clone();
        }
        let delta = &choice["delta"];
        if let Some(piece) = delta["content"].as_str() {
            content.push_str(piece);
        }
        if let Some(piece) = delta["reasoning_content"].as_str() {
            reasoning.push_str(piece);
        }
        for piece in delta["tool_calls"].as_array().into_iter().flatten() {
            let at = piece["index"]
                .as_u64()
                .and_then(|i| usize::try_from(i).ok())
                .unwrap_or(calls.len().saturating_sub(1));
            if calls.len() <= at {
                calls.resize(at + 1, Default::default());
            }
            let (id, name, arguments) = &mut calls[at];
            if let Some(piece) = piece["id"].as_str() {
                id.push_str(piece);
            }
            if let Some(piece) = piece["function"]["name"].as_str() {
                name.push_str(piece);
            }
            if let Some(piece) = piece["function"]["arguments"].as_str() {
                arguments.push_str(piece);
            }
        }
    }
    let mut message = json!({"role": "assistant", "content": content});
    if !reasoning.is_empty() {
        message["reasoning_content"] = json!(reasoning);
    }
    if !calls.is_empty() {
        message["tool_calls"] = calls
            .into_iter()
            .map(|(id, name, arguments)| {
                let mut call =
                    json!({"type": "function", "function": {"name": name, "arguments": arguments}});
                if !id.is_empty() {
                    call["id"] = json!(id);
                }
                call
            })
            .collect();
    }
    json!({
        "model": model,
        "choices": [{"message": message, "finish_reason": finish}],
        "usage": usage,
    })
}

/// A completion's messages in the wire format. A message with images,
/// sent to a model that sees, is a list of parts: the text, then each image
/// as a `data:` URL, since a platform's own link may be private or expired.
fn chat(req: &Completion, sees: bool) -> Vec<Value> {
    let mut messages = vec![json!({"role": "system", "content": req.system})];
    messages.extend(req.messages.iter().map(|m| {
        let role = match m.role {
            Role::User => "user",
            Role::Assistant => "assistant",
        };
        if !sees || m.images.is_empty() {
            return json!({"role": role, "content": m.text});
        }
        let mut parts = vec![json!({"type": "text", "text": m.text})];
        parts.extend(
            m.images
                .iter()
                .map(|p| json!({"type": "image_url", "image_url": {"url": p.data_url()}})),
        );
        json!({"role": role, "content": parts})
    }));
    messages
}

/// A working transcript in the wire format.
///
/// A `tool` message takes text only, so pictures a tool returned follow
/// the results of that step in one `user` message: for each call, a line
/// naming it and then its images. It comes after all of the step's results
/// because the API wants every call answered before anything else is said.
/// A model that cannot see is told how many images there were instead.
fn wire(system: &str, items: &[Item], sees: bool) -> Vec<Value> {
    let mut messages = vec![json!({"role": "system", "content": system})];
    let mut shown: Vec<Value> = Vec::new();
    for (at, item) in items.iter().enumerate() {
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
            Item::Result {
                id,
                name,
                output,
                pictures,
                ..
            } => {
                let content = if pictures.is_empty() || sees {
                    output.clone()
                } else {
                    format!("{output}\n{}", crate::not_shown(pictures.len()))
                };
                messages.push(json!({"role": "tool", "tool_call_id": id, "content": content}));
                if sees && !pictures.is_empty() {
                    shown.push(json!({
                        "type": "text",
                        "text": format!("The images returned by the `{name}` call {id}:"),
                    }));
                    shown.extend(
                        pictures.iter().map(
                            |p| json!({"type": "image_url", "image_url": {"url": p.data_url()}}),
                        ),
                    );
                }
                let last_result = !matches!(items.get(at + 1), Some(Item::Result { .. }));
                if last_result && !shown.is_empty() {
                    messages.push(json!({"role": "user", "content": std::mem::take(&mut shown)}));
                }
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
        thought: u["completion_tokens_details"]["reasoning_tokens"]
            .as_u64()
            .unwrap_or(0),
    }
}

#[async_trait]
impl Brain for OpenAi {
    fn label(&self) -> String {
        format!("openai/{}", self.model)
    }

    async fn complete(&self, req: &Completion) -> Result<Reply> {
        let mut body = json!({
            "model": self.model,
            "messages": chat(req, self.sees),
        });
        body[&self.max_tokens_param] = json!(req.max_tokens);
        if req.json && self.json_mode {
            body["response_format"] = json!({"type": "json_object"});
        }
        if let Some(t) = req.temperature {
            body["temperature"] = json!(t);
        }

        let v = self.post(&mut body, req.effort).await?;
        let out = v["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or_default()
            .trim()
            .to_owned();
        if out.is_empty() {
            let spent = usage(&v);
            return Err(Error::Malformed {
                provider: "openai".into(),
                detail: if v["choices"][0]["finish_reason"] == "length" {
                    format!(
                        "the answer ran out at {} tokens, {} of them thinking, before anything was written",
                        spent.output, spent.thought
                    )
                } else {
                    "an empty answer".to_owned()
                },
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

    fn sees(&self) -> bool {
        self.sees
    }

    fn window(&self) -> Option<u64> {
        self.window
    }

    fn max_output(&self) -> Option<u32> {
        self.max_output
    }

    async fn step(&self, turn: &Turn) -> Result<Step> {
        let mut body = json!({
            "model": self.model,
            "messages": wire(&turn.system, &turn.items, self.sees),
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

        let v = self.post(&mut body, turn.effort).await?;
        let choice = &v["choices"][0];
        let message = &choice["message"];
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
    fn a_streamed_answer_is_put_back_together() {
        let events: Vec<Value> = [
            r#"{"model":"m","choices":[{"delta":{"role":"assistant","reasoning_content":"they want "}}]}"#,
            r#"{"choices":[{"delta":{"reasoning_content":"two things"}}]}"#,
            r#"{"choices":[{"delta":{"content":"On it."}}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"c1","function":{"name":"shell","arguments":"{\"comm"}}]}}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":1,"id":"c2","function":{"name":"read","arguments":""}}]}}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"and\":\"ls\"}"}}]}}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":1,"function":{"arguments":"{\"path\":\"a\"}"}}]}}]}"#,
            r#"{"choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#,
            r#"{"choices":[],"usage":{"prompt_tokens":100,"prompt_cache_hit_tokens":80,"completion_tokens":30,"completion_tokens_details":{"reasoning_tokens":12}}}"#,
        ]
        .iter()
        .map(|e| serde_json::from_str(e).unwrap())
        .collect();
        let v = gather(&events);
        let message = &v["choices"][0]["message"];
        assert_eq!(message["content"], "On it.");
        assert_eq!(message["reasoning_content"], "they want two things");
        assert_eq!(message["tool_calls"][0]["id"], "c1");
        assert_eq!(
            message["tool_calls"][0]["function"]["arguments"],
            r#"{"command":"ls"}"#
        );
        assert_eq!(message["tool_calls"][1]["function"]["name"], "read");
        assert_eq!(v["choices"][0]["finish_reason"], "tool_calls");
        assert_eq!(v["model"], "m");
        let spent = usage(&v);
        assert_eq!(
            (spent.input, spent.cached, spent.output, spent.thought),
            (100, 80, 30, 12)
        );
    }

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
            Item::result("c1", "shell", "a.txt", false),
        ];
        let m = wire("sys", &items, false);
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

    fn two_calls_one_with_a_picture() -> Vec<Item> {
        let call = |id: &str, name: &str| Call {
            id: id.into(),
            name: name.into(),
            args: json!({}),
        };
        vec![
            Item::user("look"),
            Item::Assistant {
                text: String::new(),
                calls: vec![call("c1", "fetch"), call("c2", "shell")],
                carry: None,
            },
            Item::Result {
                id: "c1".into(),
                name: "fetch".into(),
                output: "An image.".into(),
                failed: false,
                pictures: vec![crate::Picture {
                    mime: "image/png".into(),
                    bytes: b"\x89PNG".to_vec(),
                }],
            },
            Item::result("c2", "shell", "ok", false),
            Item::user("go on"),
        ]
    }

    #[test]
    fn pictures_a_tool_returned_follow_the_results_in_a_user_message() {
        let m = wire("sys", &two_calls_one_with_a_picture(), true);
        let roles: Vec<&str> = m.iter().map(|m| m["role"].as_str().unwrap()).collect();
        // Every call is answered before the pictures are shown.
        assert_eq!(
            roles,
            [
                "system",
                "user",
                "assistant",
                "tool",
                "tool",
                "user",
                "user"
            ]
        );
        assert_eq!(m[3]["content"], "An image.");
        let parts = m[5]["content"].as_array().unwrap();
        assert_eq!(parts.len(), 2);
        assert!(parts[0]["text"].as_str().unwrap().contains("c1"));
        assert_eq!(
            parts[1]["image_url"]["url"],
            "data:image/png;base64,iVBORw=="
        );
        assert_eq!(m[6]["content"], "go on");
    }

    #[test]
    fn a_model_that_cannot_see_is_told_how_many_images_it_missed() {
        let m = wire("sys", &two_calls_one_with_a_picture(), false);
        assert_eq!(m.len(), 6, "no message of pictures");
        assert_eq!(
            m[3]["content"],
            "An image.\n[1 image, not shown: this model cannot see images]"
        );
        assert!(!m.iter().any(|m| m.to_string().contains("base64")));
    }

    #[test]
    fn an_image_goes_to_a_model_that_sees_as_a_data_url_after_the_text() {
        let picture = crate::Picture {
            mime: "image/png".into(),
            bytes: b"\x89PNG".to_vec(),
        };
        let req = Completion {
            messages: vec![crate::Message::user("what is this?").with_images(vec![picture])],
            ..Completion::ask("look", "", 100)
        };
        let m = chat(&req, true);
        assert_eq!(m[0], json!({"role": "system", "content": "look"}));
        let parts = m[1]["content"].as_array().unwrap();
        assert_eq!(parts[0], json!({"type": "text", "text": "what is this?"}));
        assert_eq!(parts[1]["type"], "image_url");
        assert_eq!(
            parts[1]["image_url"]["url"],
            "data:image/png;base64,iVBORw=="
        );
        // A model that cannot see is sent the words alone.
        assert_eq!(chat(&req, false)[1]["content"], "what is this?");
    }

    #[test]
    fn cache_hits_are_read_from_either_shape() {
        let deepseek = json!({"usage": {"prompt_tokens": 100, "prompt_cache_hit_tokens": 80, "completion_tokens": 5}});
        let openai = json!({"usage": {"prompt_tokens": 100, "prompt_tokens_details": {"cached_tokens": 60}, "completion_tokens": 5}});
        assert_eq!(usage(&deepseek).cached, 80);
        assert_eq!(usage(&openai).cached, 60);
    }
}
