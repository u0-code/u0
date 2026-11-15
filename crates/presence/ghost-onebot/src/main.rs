//! `ghost-onebot`: carries a ghost on QQ through OneBot v11.
//!
//! This adapter speaks the OneBot protocol to whatever implementation you
//! point it at (a forward WebSocket). It knows nothing about how that
//! implementation is logged in to QQ. Read the README before choosing one.

mod convert;
mod wire;

use std::{sync::Arc, time::Duration};

use anyhow::Context;
use async_trait::async_trait;
use clap::Parser;
use ghost_link::{
    presence::{self, Field, Link, Platform, Uplink},
    protocol::{Acted, Action, Actor, Capability, GhostCard},
    text,
};
use serde_json::{Value, json};
use tokio::sync::{Mutex, mpsc};
use tracing::{debug, info};

use crate::{convert::Me, wire::Wire};

#[derive(Parser)]
#[command(
    name = "ghost-onebot",
    version,
    about = "Carries a ghost on QQ through OneBot v11."
)]
struct Args {
    /// The OneBot implementation's forward WebSocket address.
    #[arg(long, env = "ONEBOT_ENDPOINT", default_value = "ws://127.0.0.1:3001")]
    endpoint: String,
    /// The OneBot access token, if the implementation requires one.
    #[arg(long, env = "ONEBOT_ACCESS_TOKEN", hide_env_values = true)]
    access_token: Option<String>,
    /// The daemon's address.
    #[arg(long, default_value_t = ghost_link::default_url())]
    url: String,
    /// The daemon's token, if it has one.
    #[arg(long, env = "GHOSTD_TOKEN", hide_env_values = true)]
    token: Option<String>,
    /// The character this account is: the name of her directory. Needed
    /// when the daemon runs more than one.
    #[arg(long, env = "GHOST_NAME")]
    ghost: Option<String>,
    /// The channel name to register as.
    #[arg(long, default_value = "qq")]
    channel: String,
}

struct OneBot {
    wire: Wire,
    me: Me,
    /// Taken by `listen`, once.
    events: Mutex<Option<mpsc::Receiver<Value>>>,
}

#[async_trait]
impl Platform for OneBot {
    async fn listen(&self, up: Uplink) -> anyhow::Result<()> {
        let mut events = self
            .events
            .lock()
            .await
            .take()
            .context("already listening")?;
        while let Some(event) = events.recv().await {
            if let Some(message) = convert::inbound(&event, &self.me) {
                up.inbound(message).await;
            } else if let Some(place) = convert::typing(&event) {
                up.typing(place);
            }
        }
        anyhow::bail!("the onebot connection task ended")
    }

    async fn act(&self, action: Action) -> anyhow::Result<Acted> {
        match action {
            Action::Send {
                place,
                text,
                reply_to,
            } => {
                let (action, key, id) = convert::target(&place)
                    .with_context(|| format!("{place:?} is not somewhere on qq"))?;
                let mut last = None;
                for (i, part) in text::chunks(&text, convert::MESSAGE_LIMIT)
                    .iter()
                    .enumerate()
                {
                    let quoted = if i == 0 { reply_to.as_deref() } else { None };
                    let params = json!({key: id, "message": convert::outgoing(part, quoted)});
                    let sent = self.wire.call(action, params).await?;
                    last = convert::id(&sent["message_id"]);
                }
                Ok(Acted {
                    id: last,
                    items: Vec::new(),
                    measure: None,
                })
            }
            Action::Typing { place } => {
                // An extension only some implementations have, and only for
                // private chats. Where it is missing, she simply does not
                // show as typing.
                if let Some(("send_private_msg", _, user)) = convert::target(&place) {
                    let params = json!({"user_id": user, "event_type": 1});
                    if let Err(e) = self.wire.call("set_input_status", params).await {
                        debug!("no typing indicator here: {e:#}");
                    }
                }
                Ok(Acted::default())
            }
            other => anyhow::bail!("this adapter cannot {:?} on qq", other.needs()),
        }
    }

    /// OneBot v11 has no call for an account's name, signature or picture.
    /// Some implementations add their own, each differently, and none is
    /// relied on here.
    async fn appear(&self, card: &GhostCard) {
        presence::set_by_hand(
            card,
            &[Field::Name, Field::Bio, Field::Avatar],
            "the QQ client signed in to the account OneBot runs",
        );
    }
}

/// Ask who we are logged in as, waiting for the socket to come up.
async fn login(wire: &Wire) -> anyhow::Result<Me> {
    let mut last = None;
    for _ in 0..15 {
        match wire.call("get_login_info", json!({})).await {
            Ok(info) => {
                let id = convert::id(&info["user_id"]).context("onebot did not say who we are")?;
                let name = info["nickname"].as_str().unwrap_or(&id).to_owned();
                return Ok(Me { id, name });
            }
            Err(e) => last = Some(e),
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    Err(last.unwrap_or_else(|| anyhow::anyhow!("onebot never answered")))
        .context("log in through onebot")
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

    let (wire, events) = Wire::open(args.endpoint, args.access_token);
    let me = login(&wire).await?;
    info!("on qq as {} ({})", me.name, me.id);
    let link = Link {
        url: args.url,
        token: args.token,
        ghost: args.ghost,
        channel: args.channel,
        me: Some(Actor {
            id: me.id.clone(),
            name: me.name.clone(),
            handle: None,
            automated: true,
        }),
        can: vec![Capability::Send, Capability::Typing],
    };
    let onebot = OneBot {
        wire,
        me,
        events: Mutex::new(Some(events)),
    };
    presence::run(Arc::new(onebot), link).await
}
