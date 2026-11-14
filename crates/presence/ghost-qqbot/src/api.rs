//! The slice of the QQ Bot open API a presence needs: an access token, the
//! gateway's address, and sending a message.

use std::time::{Duration, Instant};

use anyhow::{Context, bail};
use reqwest::Method;
use serde_json::{Value, json};
use tokio::sync::Mutex;

const HTTP_TIMEOUT: Duration = Duration::from_secs(30);
/// A token is fetched again this long before it runs out, so that a request
/// made just before the end does not arrive just after it.
const REFRESH_AHEAD: Duration = Duration::from_secs(5 * 60);
/// What a token lasts when QQ does not say.
const TOKEN_LIFE: u64 = 7200;

pub struct Api {
    client: reqwest::Client,
    api_base: String,
    token_base: String,
    app_id: String,
    secret: String,
    token: Mutex<Option<Token>>,
}

struct Token {
    value: String,
    until: Instant,
}

/// An error the API reported, as opposed to one reaching it.
#[derive(Debug)]
pub struct ApiError {
    pub status: u16,
    pub code: Option<i64>,
    pub message: String,
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.code {
            Some(code) => write!(f, "qq said {} ({code}): {}", self.status, self.message),
            None => write!(f, "qq said {}: {}", self.status, self.message),
        }
    }
}

impl std::error::Error for ApiError {}

impl Api {
    pub fn new(
        api_base: &str,
        token_base: &str,
        app_id: &str,
        secret: &str,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            client: ghost_net::builder().timeout(HTTP_TIMEOUT).build()?,
            api_base: api_base.trim_end_matches('/').to_owned(),
            token_base: token_base.trim_end_matches('/').to_owned(),
            app_id: app_id.trim().to_owned(),
            secret: secret.trim().to_owned(),
            token: Mutex::new(None),
        })
    }

    /// The access token, fetched again if it is close to running out or if
    /// `fresh` says the last one was refused.
    pub async fn token(&self, fresh: bool) -> anyhow::Result<String> {
        let mut held = self.token.lock().await;
        if !fresh
            && let Some(token) = held.as_ref()
            && Instant::now() + REFRESH_AHEAD < token.until
        {
            return Ok(token.value.clone());
        }
        let response = self
            .client
            .post(format!("{}/app/getAppAccessToken", self.token_base))
            .json(&json!({"appId": self.app_id, "clientSecret": self.secret}))
            .send()
            .await
            .context("reach qq for an access token")?;
        let status = response.status();
        let answer: Value = response
            .json()
            .await
            .context("read qq's answer to the access token request")?;
        let Some(value) = answer["access_token"].as_str().filter(|t| !t.is_empty()) else {
            bail!(
                "qq gave no access token ({status}): {}; check QQBOT_APPID and QQBOT_SECRET",
                answer["message"].as_str().unwrap_or("no reason given")
            );
        };
        // Written as a number by some versions of the API and as a string by others.
        let life = answer["expires_in"]
            .as_u64()
            .or_else(|| answer["expires_in"].as_str()?.parse().ok())
            .unwrap_or(TOKEN_LIFE);
        *held = Some(Token {
            value: value.to_owned(),
            until: Instant::now() + Duration::from_secs(life),
        });
        Ok(value.to_owned())
    }

    /// Call the open API. A refused token is fetched again, once.
    pub async fn call(
        &self,
        method: Method,
        path: &str,
        body: Option<&Value>,
    ) -> anyhow::Result<Value> {
        for attempt in 0..2 {
            let token = self.token(attempt > 0).await?;
            let mut request = self
                .client
                .request(method.clone(), format!("{}{path}", self.api_base))
                .header("Authorization", format!("QQBot {token}"));
            if let Some(body) = body {
                request = request.json(body);
            }
            let response = request
                .send()
                .await
                .with_context(|| format!("reach qq for {path}"))?;
            let status = response.status();
            let text = response
                .text()
                .await
                .with_context(|| format!("read qq's answer to {path}"))?;
            if status.is_success() {
                return Ok(serde_json::from_str(&text).unwrap_or(Value::Null));
            }
            if status == reqwest::StatusCode::UNAUTHORIZED && attempt == 0 {
                continue;
            }
            let answer: Value = serde_json::from_str(&text).unwrap_or_default();
            return Err(ApiError {
                status: status.as_u16(),
                code: answer["code"]
                    .as_i64()
                    .or_else(|| answer["err_code"].as_i64()),
                message: answer["message"]
                    .as_str()
                    .map_or_else(|| text.chars().take(200).collect(), str::to_owned),
            }
            .into());
        }
        bail!("qq kept refusing a fresh access token for {path}")
    }

    /// The token and gateway address to connect with.
    pub async fn gateway(&self, fresh: bool) -> anyhow::Result<(String, String)> {
        if fresh {
            self.token(true).await?;
        }
        let answer = self.call(Method::GET, "/gateway", None).await?;
        let url = answer["url"]
            .as_str()
            .filter(|u| !u.is_empty())
            .context("qq did not say where its gateway is")?
            .to_owned();
        Ok((self.token(false).await?, url))
    }
}
