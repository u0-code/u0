//! Sending a request to a provider, patiently.
//!
//! Work runs for a long time and providers have bad minutes. A rate limit, an
//! overloaded server or a dropped connection is retried with backoff; an
//! error that retrying cannot fix is returned at once.
//!
//! Answers are read as a stream where the provider offers one. A model that
//! thinks for minutes before it writes sends nothing at all until it is
//! done unless it is streaming, and a connection with nothing on it is one
//! that something between here and there will close.

use std::time::Duration;

use serde_json::Value;

use crate::{Error, Result};

const ATTEMPTS: u32 = 5;
/// The longest one answer may take from the first byte to the last.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(1800);
/// The longest the connection may stay silent. A stream sends something
/// every few seconds, thinking included, so a silence this long is a
/// connection that has stalled and not a model that is busy.
const READ_TIMEOUT: Duration = Duration::from_secs(120);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);

pub(crate) fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .read_timeout(READ_TIMEOUT)
        .connect_timeout(CONNECT_TIMEOUT)
        .build()
        .unwrap_or_default()
}

fn transient(status: u16) -> bool {
    matches!(status, 408 | 409 | 425 | 429) || status >= 500
}

/// Whether an error body is a provider saying the request was larger than
/// the model's context window. Each says it in its own words; these are the
/// ones `OpenAI`, Anthropic, `DeepSeek` and the servers that imitate them use.
fn says_too_long(status: u16, body: &str) -> bool {
    if !matches!(status, 400 | 413 | 422) {
        return false;
    }
    let body = body.to_lowercase();
    body.contains("context_length_exceeded")
        || body.contains("context length")
        || body.contains("context window")
        || body.contains("maximum context")
        || body.contains("prompt is too long")
        || body.contains("input is too long")
        || body.contains("too many tokens")
        || body.contains("exceeds the model") && body.contains("token")
}

/// The error for a response that was not a success.
pub(crate) fn failure(provider: &str, status: u16, body: String) -> Error {
    if says_too_long(status, &body) {
        Error::TooLong {
            provider: provider.to_owned(),
            detail: body.chars().take(300).collect(),
        }
    } else {
        Error::Api {
            provider: provider.to_owned(),
            status,
            body,
        }
    }
}

/// What a provider sent back.
pub(crate) enum Answer {
    /// The events of a stream, in order.
    Events(Vec<Value>),
    /// One JSON body: the provider does not stream, or chose not to.
    Whole(Value, String),
}

/// Split a body of server-sent events into its JSON payloads. Returns them
/// with whether the stream said it was finished (`data: [DONE]`).
fn events(body: &str) -> (Vec<Value>, bool) {
    let mut out = Vec::new();
    let mut done = false;
    for line in body.lines() {
        let Some(data) = line.strip_prefix("data:") else {
            continue;
        };
        let data = data.trim();
        if data == "[DONE]" {
            done = true;
        } else if let Ok(v) = serde_json::from_str::<Value>(data) {
            out.push(v);
        }
    }
    (out, done)
}

/// Send what `build` makes and read the answer to its end, as a stream of
/// events or as one JSON body, whichever the provider sent. `build` is
/// called again for each attempt because a request cannot be reused.
///
/// `finished` says whether the events hold a complete answer. A stream that
/// stops short of one is a dropped connection, and is asked for again.
pub(crate) async fn ask(
    provider: &str,
    build: impl Fn() -> reqwest::RequestBuilder,
    finished: impl Fn(&[Value]) -> bool,
) -> Result<Answer> {
    let mut wait = Duration::from_secs(2);
    let mut attempt = 1;
    loop {
        let last = attempt == ATTEMPTS;
        match build().send().await {
            Ok(mut resp) => {
                let status = resp.status();
                let asked = resp
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse::<u64>().ok())
                    .map(Duration::from_secs);
                // Read whatever comes, and keep what came if the
                // connection breaks: it says how far the answer got.
                let mut bytes = Vec::new();
                let mut broken = None;
                loop {
                    match resp.chunk().await {
                        Ok(Some(chunk)) => {
                            bytes.extend_from_slice(&chunk);
                            // A stream says when it is over. Some servers
                            // then hold the connection open; there is
                            // nothing more to wait for.
                            let tail = &bytes[bytes.len().saturating_sub(64)..];
                            if tail.windows(6).any(|w| w == b"[DONE]") {
                                break;
                            }
                        }
                        Ok(None) => break,
                        Err(e) => {
                            broken = Some(e);
                            break;
                        }
                    }
                }
                let text = String::from_utf8_lossy(&bytes).into_owned();
                let refused = !status.is_success();
                if refused {
                    if !transient(status.as_u16()) || last {
                        return Err(failure(provider, status.as_u16(), text));
                    }
                    tracing::warn!("{provider} returned {status}; attempt {attempt} of {ATTEMPTS}");
                    if let Some(asked) = asked {
                        wait = wait.max(asked.min(Duration::from_secs(90)));
                    }
                } else {
                    let (events, done) = events(&text);
                    if let Some(error) = events.iter().find_map(|e| e.get("error")) {
                        // An error sent down a stream that had already begun.
                        let body = error.to_string();
                        if says_too_long(400, &body) || last {
                            return Err(failure(provider, 400, body));
                        }
                        tracing::warn!(
                            "{provider} failed mid-answer: {body}; attempt {attempt} of {ATTEMPTS}"
                        );
                    } else if !events.is_empty() && (done || finished(&events)) {
                        return Ok(Answer::Events(events));
                    } else if events.is_empty()
                        && let Ok(v) = serde_json::from_str::<Value>(&text)
                    {
                        return Ok(Answer::Whole(v, text));
                    } else if last {
                        return Err(match broken {
                            Some(e) => e.into(),
                            None => Error::Malformed {
                                provider: provider.to_owned(),
                                detail: format!(
                                    "the answer stopped after {} bytes without finishing",
                                    bytes.len()
                                ),
                            },
                        });
                    } else {
                        // Some servers answer 200 with an empty or cut-off
                        // body when they are struggling, and a long answer
                        // can lose its connection. Both are worth another try.
                        tracing::warn!(
                            "{provider}: the answer stopped after {} bytes; attempt {attempt} of {ATTEMPTS}",
                            bytes.len()
                        );
                    }
                }
            }
            Err(e)
                if last || !(e.is_timeout() || e.is_connect() || e.is_request() || e.is_body()) =>
            {
                return Err(e.into());
            }
            Err(e) => tracing::warn!("{provider}: {e}; attempt {attempt} of {ATTEMPTS}"),
        }
        tokio::time::sleep(wait).await;
        wait = (wait * 3).min(Duration::from_secs(60));
        attempt += 1;
    }
}

/// Send what `build` makes and parse the body as JSON, for a provider that
/// is not asked to stream.
pub(crate) async fn send(
    provider: &str,
    build: impl Fn() -> reqwest::RequestBuilder,
) -> Result<(Value, String)> {
    match ask(provider, build, |_| false).await? {
        Answer::Whole(v, text) => Ok((v, text)),
        Answer::Events(_) => Err(Error::Malformed {
            provider: provider.to_owned(),
            detail: "a stream came back where one body was asked for".into(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stream_is_split_into_its_events() {
        let body = ": keep-alive\n\ndata: {\"a\":1}\n\ndata: {\"a\":2}\n\ndata: [DONE]\n\n";
        let (found, done) = events(body);
        assert_eq!(found.len(), 2);
        assert_eq!(found[1]["a"], 2);
        assert!(done);
        // Cut off before the end: what came is kept, and it is not done.
        let (found, done) = events("data: {\"a\":1}\n\ndata: {\"a\"");
        assert_eq!(found.len(), 1);
        assert!(!done);
    }

    #[test]
    fn too_long_is_told_apart_from_other_refusals() {
        let e = failure(
            "openai",
            400,
            r#"{"error":{"message":"This model's maximum context length is 1048576 tokens."}}"#
                .into(),
        );
        assert!(matches!(e, Error::TooLong { .. }));
        let e = failure(
            "anthropic",
            400,
            r#"{"error":{"message":"prompt is too long: 250000 tokens > 200000 maximum"}}"#.into(),
        );
        assert!(matches!(e, Error::TooLong { .. }));
        // Asking for too much output is a different mistake.
        let e = failure(
            "openai",
            400,
            r#"{"error":{"message":"Invalid max_tokens value, the valid range of max_tokens is [1, 8192]"}}"#
                .into(),
        );
        assert!(matches!(e, Error::Api { status: 400, .. }));
        let e = failure("openai", 500, "context length".into());
        assert!(matches!(e, Error::Api { status: 500, .. }));
    }
}
