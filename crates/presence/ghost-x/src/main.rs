//! `ghost-x`: carries a ghost on X.
//!
//! X is where she has a public life: mentions reach her like any message,
//! direct messages are private conversations, and her home timeline is
//! something she scrolls when she picks up her phone. She can like, reply,
//! and post.
//!
//! Everything goes through the official API v2, signed with OAuth 1.0a as
//! her account. There is no streaming on most access tiers, so mentions and
//! direct messages are polled.

mod api;
mod convert;
mod oauth;

use std::{path::PathBuf, sync::Arc, time::Duration};

use anyhow::Context;
use async_trait::async_trait;
use clap::Parser;
use ghost_link::{
    presence::{self, Link, Platform, Uplink},
    protocol::{Acted, Action, Actor, Capability, Feed},
    text,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::time::{MissedTickBehavior, interval};
use tracing::{debug, info, warn};

use crate::{
    api::{ApiError, X},
    convert::Target,
    oauth::Credentials,
};

#[derive(Parser)]
#[command(name = "ghost-x", version, about = "Carries a ghost on X.")]
struct Args {
    /// The app's API key (consumer key).
    #[arg(long, env = "X_API_KEY", hide_env_values = true)]
    api_key: String,
    /// The app's API key secret (consumer secret).
    #[arg(long, env = "X_API_SECRET", hide_env_values = true)]
    api_secret: String,
    /// Her account's access token.
    #[arg(long, env = "X_ACCESS_TOKEN", hide_env_values = true)]
    access_token: String,
    /// Her account's access token secret.
    #[arg(long, env = "X_ACCESS_SECRET", hide_env_values = true)]
    access_secret: String,
    /// The daemon's address.
    #[arg(long, default_value_t = ghost_link::default_url())]
    url: String,
    /// The daemon's token, if it has one.
    #[arg(long, env = "GHOSTD_TOKEN", hide_env_values = true)]
    token: Option<String>,
    /// The channel name to register as.
    #[arg(long, default_value = "x")]
    channel: String,
    /// Base URL of the API.
    #[arg(long, default_value = "https://api.x.com")]
    api_base: String,
    /// Seconds between checks for new mentions. 0 turns them off.
    #[arg(long, default_value_t = 90)]
    poll_mentions: u64,
    /// Seconds between checks for new direct messages. 0 turns them off.
    #[arg(long, default_value_t = 180)]
    poll_dms: u64,
    /// A file to remember how far she has read, so a restart does not skip
    /// what arrived while this was not running. Without it, each run starts
    /// from now.
    #[arg(long)]
    state: Option<PathBuf>,
}

/// How far she has read, by X's ids.
#[derive(Debug, Default, Serialize, Deserialize)]
struct Cursor {
    mentions: Option<u64>,
    dms: Option<u64>,
}

struct Account {
    x: X,
    /// Her user id.
    me: String,
    poll_mentions: u64,
    poll_dms: u64,
    state: Option<PathBuf>,
}

impl Account {
    async fn load_cursor(&self) -> Cursor {
        let Some(path) = &self.state else {
            return Cursor::default();
        };
        match tokio::fs::read(path).await {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
            Err(_) => Cursor::default(),
        }
    }

    async fn save_cursor(&self, cursor: &Cursor) {
        let Some(path) = &self.state else { return };
        if let Ok(bytes) = serde_json::to_vec(cursor)
            && let Err(e) = tokio::fs::write(path, bytes).await
        {
            warn!("could not save {}: {e}", path.display());
        }
    }

    /// New mentions since the cursor. The first look only sets the cursor:
    /// she starts from now rather than answering old mentions.
    async fn mentions(&self, cursor: &mut Cursor, up: &Uplink) -> anyhow::Result<()> {
        let mut query: Vec<(&str, String)> = convert::TWEET_FIELDS
            .iter()
            .map(|(k, v)| (*k, (*v).to_owned()))
            .collect();
        query.push(("max_results", "20".to_owned()));
        // Zero marks "looked, and there was nothing yet"; it is not an id.
        if let Some(since) = cursor.mentions.filter(|since| *since > 0) {
            query.push(("since_id", since.to_string()));
        }
        let page = self
            .x
            .get(&format!("/2/users/{}/mentions", self.me), &query)
            .await?;
        let newest = convert::newest(&page);
        if cursor.mentions.is_some() {
            for post in convert::posts(&page, &self.me, true) {
                up.inbound(post).await;
            }
        }
        if let Some(newest) = newest.or(Some(0)).filter(|n| Some(*n) > cursor.mentions) {
            cursor.mentions = Some(newest);
            self.save_cursor(cursor).await;
        }
        Ok(())
    }

    /// New direct messages since the cursor, with the same first-look rule.
    async fn dms(&self, cursor: &mut Cursor, up: &Uplink) -> anyhow::Result<()> {
        let query = [
            ("event_types", "MessageCreate".to_owned()),
            (
                "dm_event.fields",
                "id,text,event_type,sender_id,dm_conversation_id".to_owned(),
            ),
            ("expansions", "sender_id".to_owned()),
            ("user.fields", "username,name".to_owned()),
            ("max_results", "50".to_owned()),
        ];
        let page = self.x.get("/2/dm_events", &query).await?;
        let newest = convert::newest(&page);
        if let Some(seen) = cursor.dms {
            for message in convert::direct_messages(&page, &self.me) {
                if message.id.parse::<u64>().is_ok_and(|id| id > seen) {
                    up.inbound(message).await;
                }
            }
        }
        if let Some(newest) = newest.or(Some(0)).filter(|n| Some(*n) > cursor.dms) {
            cursor.dms = Some(newest);
            self.save_cursor(cursor).await;
        }
        Ok(())
    }

    /// Post `text`, as a reply to `reply_to` if given. Text too long for one
    /// post becomes a thread. Returns the id of the last post.
    async fn post(&self, text: &str, reply_to: Option<&str>) -> anyhow::Result<Option<String>> {
        let mut parent = reply_to.map(str::to_owned);
        for part in text::chunks(text, convert::post_limit(text)) {
            let mut body = json!({"text": part});
            if let Some(parent) = &parent {
                body["reply"] = json!({"in_reply_to_tweet_id": parent});
            }
            let sent = self.x.post("/2/tweets", &body).await?;
            parent = sent["data"]["id"].as_str().map(str::to_owned);
        }
        Ok(parent)
    }
}

/// Report a polling failure once, then keep quiet until it clears.
fn report(what: &str, result: anyhow::Result<()>, failing: &mut bool) {
    match result {
        Ok(()) => *failing = false,
        Err(e) => {
            let hint = match e.downcast_ref::<ApiError>().map(|api| api.status) {
                Some(401) => " (check the four credentials)",
                Some(403) => " (this endpoint may need a higher API access tier)",
                Some(429) => " (rate limited; consider polling less often)",
                _ => "",
            };
            if *failing {
                debug!("{what}: {e:#}");
            } else {
                warn!("{what}: {e:#}{hint}");
            }
            *failing = true;
        }
    }
}

#[async_trait]
impl Platform for Account {
    async fn listen(&self, up: Uplink) -> anyhow::Result<()> {
        let mut cursor = self.load_cursor().await;
        let every = |seconds: u64| {
            // A zero period would panic; an off timer just never matters.
            let mut timer = interval(Duration::from_secs(seconds.max(1)));
            timer.set_missed_tick_behavior(MissedTickBehavior::Delay);
            timer
        };
        let (mut mention_timer, mut dm_timer) = (every(self.poll_mentions), every(self.poll_dms));
        let (mut mentions_failing, mut dms_failing) = (false, false);
        if self.poll_mentions == 0 && self.poll_dms == 0 {
            // Nothing to listen for: she only posts and scrolls here.
            std::future::pending::<()>().await;
        }
        loop {
            tokio::select! {
                _ = mention_timer.tick(), if self.poll_mentions > 0 => {
                    let result = self.mentions(&mut cursor, &up).await;
                    report("mentions", result, &mut mentions_failing);
                }
                _ = dm_timer.tick(), if self.poll_dms > 0 => {
                    let result = self.dms(&mut cursor, &up).await;
                    report("direct messages", result, &mut dms_failing);
                }
            }
        }
    }

    async fn act(&self, action: Action) -> anyhow::Result<Acted> {
        match action {
            Action::Send {
                place,
                text,
                reply_to,
            } => {
                let id = match convert::target(&place)
                    .with_context(|| format!("{place:?} is not somewhere on x"))?
                {
                    // Without a specific post to answer, answer the thread's first.
                    Target::Thread(conversation) => {
                        self.post(&text, Some(reply_to.as_deref().unwrap_or(conversation)))
                            .await?
                    }
                    Target::Dm(conversation) => {
                        let path = format!("/2/dm_conversations/{conversation}/messages");
                        let sent = self.x.post(&path, &json!({"text": text})).await?;
                        sent["data"]["dm_event_id"].as_str().map(str::to_owned)
                    }
                };
                Ok(Acted {
                    id,
                    items: Vec::new(),
                    measure: None,
                })
            }
            // On X a reaction is a like, whatever the emoji.
            Action::React { to, .. } => {
                let path = format!("/2/users/{}/likes", self.me);
                self.x.post(&path, &json!({"tweet_id": to})).await?;
                Ok(Acted::default())
            }
            Action::Post { text } => Ok(Acted {
                id: self.post(&text, None).await?,
                items: Vec::new(),
                measure: None,
            }),
            Action::Fetch { feed, limit } => {
                let mut query: Vec<(&str, String)> = convert::TWEET_FIELDS
                    .iter()
                    .map(|(k, v)| (*k, (*v).to_owned()))
                    .collect();
                query.push(("max_results", limit.clamp(5, 100).to_string()));
                let (path, addressed) = match feed {
                    Feed::Timeline => {
                        query.push(("exclude", "replies,retweets".to_owned()));
                        (
                            format!("/2/users/{}/timelines/reverse_chronological", self.me),
                            false,
                        )
                    }
                    Feed::Mentions => (format!("/2/users/{}/mentions", self.me), true),
                };
                let page: Value = self.x.get(&path, &query).await?;
                Ok(Acted {
                    id: None,
                    items: convert::posts(&page, &self.me, addressed),
                    measure: None,
                })
            }
            Action::Measure => {
                // Her own account, then her own recent posts. Both are
                // reads of what she owns, the cheapest kind on X.
                let me: Value = self
                    .x
                    .get(
                        &format!("/2/users/{}", self.me),
                        &[("user.fields", "public_metrics".to_owned())],
                    )
                    .await?;
                let posts: Value = self
                    .x
                    .get(
                        &format!("/2/users/{}/tweets", self.me),
                        &[
                            ("tweet.fields", "public_metrics".to_owned()),
                            ("exclude", "retweets".to_owned()),
                            ("max_results", "20".to_owned()),
                        ],
                    )
                    .await?;
                Ok(Acted {
                    measure: Some(convert::measure(&me, &posts)),
                    ..Acted::default()
                })
            }
            other => anyhow::bail!("this adapter cannot {:?} on x", other.needs()),
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Keys and settings may sit in an ignored `.env` file in the directory
    // this runs from or one above it. Anything already set in the
    // environment wins.
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with_target(false)
        .with_writer(std::io::stderr)
        .init();
    let args = Args::parse();

    let x = X::new(
        &args.api_base,
        Credentials {
            consumer_key: args.api_key,
            consumer_secret: args.api_secret,
            token: args.access_token,
            token_secret: args.access_secret,
        },
    )?;
    let who = x
        .get(
            "/2/users/me",
            &[("user.fields", "username,name".to_owned())],
        )
        .await
        .context("log in to x")?;
    let me = who["data"]["id"]
        .as_str()
        .context("x did not say who we are")?
        .to_owned();
    let handle = who["data"]["username"].as_str().map(str::to_owned);
    info!("on x as @{}", handle.as_deref().unwrap_or(&me));

    let link = Link {
        url: args.url,
        token: args.token,
        channel: args.channel,
        me: Some(Actor {
            id: me.clone(),
            name: who["data"]["name"].as_str().unwrap_or(&me).to_owned(),
            handle,
            automated: true,
        }),
        can: vec![
            Capability::Send,
            Capability::React,
            Capability::Post,
            Capability::Feed,
            Capability::Metrics,
        ],
    };
    let account = Account {
        x,
        me,
        poll_mentions: args.poll_mentions,
        poll_dms: args.poll_dms,
        state: args.state,
    };
    presence::run(Arc::new(account), link).await
}
