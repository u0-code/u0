//! The slice of the X API v2 a presence needs, signed as one account.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::Context;
use serde_json::Value;

use crate::oauth::{self, Credentials};

pub struct X {
    client: reqwest::Client,
    base: String,
    credentials: Credentials,
}

/// An error the API reported, as opposed to one reaching it.
#[derive(Debug)]
pub struct ApiError {
    pub status: u16,
    pub detail: String,
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "x said {}: {}", self.status, self.detail)
    }
}

impl std::error::Error for ApiError {}

impl X {
    pub fn new(base: &str, credentials: Credentials) -> anyhow::Result<Self> {
        Ok(Self {
            client: ghost_net::builder()
                .timeout(Duration::from_secs(30))
                .build()?,
            base: base.trim_end_matches('/').to_owned(),
            credentials,
        })
    }

    pub async fn get(&self, path: &str, query: &[(&str, String)]) -> anyhow::Result<Value> {
        let url = format!("{}{path}", self.base);
        let params: Vec<(&str, &str)> = query.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let request = self
            .client
            .get(&url)
            .query(query)
            .header("Authorization", self.sign("GET", &url, &params));
        Self::finish(request, path).await
    }

    pub async fn post(&self, path: &str, body: &Value) -> anyhow::Result<Value> {
        let url = format!("{}{path}", self.base);
        // A JSON body is not part of an OAuth 1.0a signature.
        let request = self
            .client
            .post(&url)
            .json(body)
            .header("Authorization", self.sign("POST", &url, &[]));
        Self::finish(request, path).await
    }

    fn sign(&self, method: &str, url: &str, params: &[(&str, &str)]) -> String {
        let nonce: String = std::iter::repeat_with(fastrand::alphanumeric)
            .take(32)
            .collect();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        oauth::authorization(&self.credentials, method, url, params, &nonce, now)
    }

    async fn finish(request: reqwest::RequestBuilder, path: &str) -> anyhow::Result<Value> {
        let response = request
            .send()
            .await
            .with_context(|| format!("reach x for {path}"))?;
        let status = response.status();
        let body: Value = response.json().await.unwrap_or(Value::Null);
        if status.is_success() {
            return Ok(body);
        }
        let detail = body["detail"]
            .as_str()
            .or_else(|| body["errors"][0]["message"].as_str())
            .or_else(|| body["title"].as_str())
            .unwrap_or("no detail given")
            .to_owned();
        Err(ApiError {
            status: status.as_u16(),
            detail,
        }
        .into())
    }
}
