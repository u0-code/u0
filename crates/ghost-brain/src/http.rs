//! Sending a request to a provider, patiently.
//!
//! Work runs for a long time and providers have bad minutes. A rate limit, an
//! overloaded server or a dropped connection is retried with backoff; an
//! error that retrying cannot fix is returned at once.

use std::time::Duration;

use serde_json::Value;

use crate::{Error, Result};

const ATTEMPTS: u32 = 5;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(600);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);

pub(crate) fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .connect_timeout(CONNECT_TIMEOUT)
        .build()
        .unwrap_or_default()
}

fn transient(status: u16) -> bool {
    matches!(status, 408 | 409 | 425 | 429) || status >= 500
}

/// Send what `build` makes and parse the body as JSON. `build` is called
/// again for each attempt because a request cannot be reused.
pub(crate) async fn send(
    provider: &str,
    build: impl Fn() -> reqwest::RequestBuilder,
) -> Result<(Value, String)> {
    let mut wait = Duration::from_secs(2);
    let mut attempt = 1;
    loop {
        let last = attempt == ATTEMPTS;
        match build().send().await {
            Ok(resp) => {
                let status = resp.status();
                let asked = resp
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse::<u64>().ok())
                    .map(Duration::from_secs);
                let text = resp.text().await?;
                if status.is_success() {
                    // Some servers answer 200 with an empty or cut-off body
                    // when they are struggling. That is worth another try.
                    match serde_json::from_str::<Value>(&text) {
                        Ok(v) => return Ok((v, text)),
                        Err(e) if last => return Err(e.into()),
                        Err(_) => {}
                    }
                } else if !transient(status.as_u16()) || last {
                    return Err(Error::Api {
                        provider: provider.to_owned(),
                        status: status.as_u16(),
                        body: text,
                    });
                } else {
                    tracing::warn!("{provider} returned {status}; attempt {attempt} of {ATTEMPTS}");
                    if let Some(asked) = asked {
                        wait = wait.max(asked.min(Duration::from_secs(90)));
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
