//! `ghost-qqbot`: carries a ghost on QQ through Tencent's official QQ Bot
//! API: a bot account on the QQ open platform, events over its WebSocket
//! gateway, messages sent over HTTPS.

mod api;
mod convert;
mod gateway;
mod replies;

use std::{
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, Instant},
};

use anyhow::Context;
use async_trait::async_trait;
use clap::Parser;
use ghost_link::{
    presence::{self, Link, Platform, Uplink},
    protocol::{Acted, Action, Actor, Capability},
    text,
};
use reqwest::Method;
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::{api::Api, convert::Me, gateway::Event, replies::Replies};

/// How long to wait for the gateway to accept the bot before giving up.
const READY_WITHIN: Duration = Duration::from_secs(60);

#[derive(Parser)]
#[command(
    name = "ghost-qqbot",
    version,
    about = "Carries a ghost on QQ through the official QQ Bot API."
)]
struct Args {
    /// The bot's `AppID`, from the QQ open platform.
    #[arg(long, env = "QQBOT_APPID")]
    app_id: String,
    /// The bot's `AppSecret`, from the QQ open platform.
    #[arg(long, env = "QQBOT_SECRET", hide_env_values = true)]
    app_secret: String,
    /// The daemon's address.
    #[arg(long, default_value_t = ghost_link::default_url())]
    url: String,
    /// The daemon's token, if it has one.
    #[arg(long, env = "GHOSTD_TOKEN", hide_env_values = true)]
    token: Option<String>,
    /// The channel name to register as.
    #[arg(long, default_value = "qq")]
    channel: String,
    /// Base URL of the open API. The sandbox is at
    /// `https://sandbox.api.sgroup.qq.com`.
    #[arg(long, default_value = "https://api.sgroup.qq.com")]
    api_base: String,
    /// Base URL access tokens are fetched from.
    #[arg(long, default_value = "https://bots.qq.com")]
    token_base: String,
}

struct QqBot {
    api: Arc<Api>,
    replies: Mutex<Replies>,
    /// Taken by `listen`, once.
    events: tokio::sync::Mutex<Option<mpsc::Receiver<Event>>>,
}

impl QqBot {
    fn replies(&self) -> std::sync::MutexGuard<'_, Replies> {
        self.replies.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[async_trait]
impl Platform for QqBot {
    async fn listen(&self, up: Uplink) -> anyhow::Result<()> {
        let mut events = self
            .events
            .lock()
            .await
            .take()
            .context("already listening")?;
        while let Some(event) = events.recv().await {
            let Event::Dispatch { kind, data } = event else {
                continue;
            };
            if let Some(message) = convert::inbound(&kind, &data) {
                // Known before she can answer it.
                self.replies()
                    .received(&message.place.id, &message.id, Instant::now());
                up.inbound(message).await;
            }
        }
        anyhow::bail!("the qq gateway task ended")
    }

    async fn act(&self, action: Action) -> anyhow::Result<Acted> {
        let Action::Send {
            place,
            text,
            reply_to,
        } = action
        else {
            anyhow::bail!("qq bots cannot {:?} here", action.needs());
        };
        let path =
            convert::target(&place).with_context(|| format!("{place:?} is not somewhere on qq"))?;
        let mut last = None;
        for part in text::chunks(&text, convert::MESSAGE_LIMIT) {
            let passive = self
                .replies()
                .answer(&place, reply_to.as_deref(), Instant::now());
            let body = convert::outgoing(&part, passive.as_ref());
            let sent = self
                .api
                .call(Method::POST, &path, Some(&body))
                .await
                .with_context(|| {
                    if passive.is_some() {
                        "send a reply on qq".to_owned()
                    } else {
                        // Most refusals come from here: QQ allows a bot to
                        // speak first only rarely.
                        "send on qq with nothing recent here to answer, which qq allows only rarely"
                            .to_owned()
                    }
                })?;
            last = sent["id"].as_str().map(str::to_owned);
        }
        Ok(Acted {
            id: last,
            items: Vec::new(),
            measure: None,
        })
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with_target(false)
        .with_writer(std::io::stderr)
        .init();
    // The gateway's TLS needs a crypto provider chosen once for the process.
    // Another may already be installed; either will do.
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    let args = Args::parse();

    let api = Arc::new(Api::new(
        &args.api_base,
        &args.token_base,
        &args.app_id,
        &args.app_secret,
    )?);
    // Wrong credentials are said once, now, rather than retried forever.
    api.token(false).await.context("log in to qq")?;

    let (tx, mut events) = mpsc::channel(1024);
    let gateway = tokio::spawn({
        let api = api.clone();
        gateway::run(
            move |fresh| {
                let api = api.clone();
                async move { api.gateway(fresh).await }
            },
            tx,
        )
    });
    let me = tokio::time::timeout(READY_WITHIN, async {
        while let Some(event) = events.recv().await {
            if let Event::Ready(user) = event {
                return Me::from_ready(&user);
            }
        }
        None
    })
    .await
    .ok()
    .flatten();
    let Some(me) = me else {
        if gateway.is_finished() {
            gateway.await??;
        }
        anyhow::bail!("the qq gateway did not accept the bot within a minute");
    };
    info!("on qq as {} ({})", me.name, me.id);
    tokio::spawn(async move {
        match gateway.await {
            Ok(Err(e)) => warn!("the qq gateway gave up: {e:#}"),
            Err(e) => warn!("the qq gateway task failed: {e}"),
            Ok(Ok(())) => {}
        }
    });

    let link = Link {
        url: args.url,
        token: args.token,
        channel: args.channel,
        me: Some(Actor {
            id: me.id,
            name: me.name,
            handle: None,
            automated: true,
        }),
        can: vec![Capability::Send],
    };
    let bot = QqBot {
        api,
        replies: Mutex::new(Replies::default()),
        events: tokio::sync::Mutex::new(Some(events)),
    };
    presence::run(Arc::new(bot), link).await
}
