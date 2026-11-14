//! The slice of the Telegram Bot API a presence needs.

use std::time::Duration;

use anyhow::{Context, bail};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;

/// Longer than the long-poll timeout, so the server answers first.
const HTTP_TIMEOUT: Duration = Duration::from_secs(75);

pub struct Bot {
    client: reqwest::Client,
    /// `https://api.telegram.org/bot<token>`
    base: String,
}

/// An error the API reported, as opposed to one reaching it.
#[derive(Debug)]
pub struct ApiError {
    pub code: i64,
    pub description: String,
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "telegram said {}: {}", self.code, self.description)
    }
}

impl std::error::Error for ApiError {}

#[derive(Deserialize)]
struct Envelope {
    ok: bool,
    #[serde(default)]
    result: Value,
    #[serde(default)]
    description: String,
    #[serde(default)]
    error_code: i64,
    #[serde(default)]
    parameters: Option<Parameters>,
}

#[derive(Deserialize)]
struct Parameters {
    #[serde(default)]
    retry_after: Option<u64>,
}

impl Bot {
    pub fn new(api_base: &str, token: &str) -> anyhow::Result<Self> {
        Ok(Self {
            client: ghost_net::builder().timeout(HTTP_TIMEOUT).build()?,
            base: format!("{}/bot{token}", api_base.trim_end_matches('/')),
        })
    }

    /// Call a method. Waits out one flood-control delay if Telegram asks.
    pub async fn call<T: DeserializeOwned>(
        &self,
        method: &str,
        params: &impl Serialize,
    ) -> anyhow::Result<T> {
        for attempt in 0..2 {
            let envelope: Envelope = self
                .client
                .post(format!("{}/{method}", self.base))
                .json(params)
                .send()
                .await
                // The URL holds the token; keep it out of error messages.
                .map_err(reqwest::Error::without_url)
                .with_context(|| format!("reach telegram for {method}"))?
                .json()
                .await
                .map_err(reqwest::Error::without_url)
                .with_context(|| format!("read telegram's answer to {method}"))?;
            if envelope.ok {
                return serde_json::from_value(envelope.result)
                    .with_context(|| format!("understand telegram's answer to {method}"));
            }
            let wait = envelope.parameters.and_then(|p| p.retry_after);
            match wait {
                Some(seconds) if attempt == 0 && seconds <= 60 => {
                    tokio::time::sleep(Duration::from_secs(seconds)).await;
                }
                _ => {
                    return Err(ApiError {
                        code: envelope.error_code,
                        description: envelope.description,
                    }
                    .into());
                }
            }
        }
        bail!("telegram kept asking to wait on {method}")
    }
}

#[derive(Debug, Deserialize)]
pub struct Update {
    pub update_id: i64,
    #[serde(default)]
    pub message: Option<Message>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct User {
    pub id: i64,
    #[serde(default)]
    pub is_bot: bool,
    #[serde(default)]
    pub first_name: String,
    #[serde(default)]
    pub last_name: Option<String>,
    #[serde(default)]
    pub username: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Chat {
    pub id: i64,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub title: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Entity {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub user: Option<User>,
}

#[derive(Debug, Deserialize)]
pub struct Message {
    pub message_id: i64,
    #[serde(default)]
    pub from: Option<User>,
    pub chat: Chat,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub caption: Option<String>,
    #[serde(default)]
    pub entities: Vec<Entity>,
    #[serde(default)]
    pub caption_entities: Vec<Entity>,
    #[serde(default)]
    pub reply_to_message: Option<Box<Message>>,
    // Media is only ever described, so its shape does not matter here.
    #[serde(default)]
    pub photo: Option<Value>,
    #[serde(default)]
    pub sticker: Option<Value>,
    #[serde(default)]
    pub voice: Option<Value>,
    #[serde(default)]
    pub audio: Option<Value>,
    #[serde(default)]
    pub video: Option<Value>,
    #[serde(default)]
    pub video_note: Option<Value>,
    #[serde(default)]
    pub animation: Option<Value>,
    #[serde(default)]
    pub document: Option<Value>,
    #[serde(default)]
    pub location: Option<Value>,
    #[serde(default)]
    pub contact: Option<Value>,
    #[serde(default)]
    pub poll: Option<Value>,
}
