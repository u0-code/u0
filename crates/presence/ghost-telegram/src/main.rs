//! `ghost-telegram`: carries a ghost on Telegram.
//!
//! Uses the Bot API with long polling, so it needs no public address. The
//! account is a bot: Telegram labels it as one, which is the disclosure the
//! charter asks for, and people have to write to her first.

mod api;
mod convert;

use std::{sync::Arc, time::Duration};

use anyhow::Context;
use async_trait::async_trait;
use clap::Parser;
use ghost_link::{
    presence::{self, Backoff, Field, Link, Platform, Uplink},
    protocol::{Acted, Action, Capability, GhostCard},
    text,
};
use serde_json::{Value, json};
use tracing::{info, warn};

use crate::api::{ApiError, Bot, Update, User};

/// Seconds Telegram holds a `getUpdates` call open when nothing is new.
const LONG_POLL: u64 = 50;
/// The longest short description a bot may have: what its profile shows.
const SHORT_DESCRIPTION: usize = 120;

#[derive(Parser)]
#[command(
    name = "ghost-telegram",
    version,
    about = "Carries a ghost on Telegram.",
    mut_arg("channel", |a| a.default_value("telegram"))
)]
struct Args {
    /// The bot token from `@BotFather`.
    #[arg(long, env = "TELEGRAM_BOT_TOKEN", hide_env_values = true)]
    bot_token: String,
    #[command(flatten)]
    daemon: presence::Daemon,
    /// Base URL of the Bot API, for a self-hosted server.
    #[arg(long, default_value = "https://api.telegram.org")]
    api_base: String,
}

struct Telegram {
    bot: Bot,
    me: User,
}

#[async_trait]
impl Platform for Telegram {
    async fn listen(&self, up: Uplink) -> anyhow::Result<()> {
        let mut offset = 0i64;
        let mut retry = Backoff::new(Duration::from_secs(2), Duration::from_secs(64));
        loop {
            let params = json!({
                "offset": offset,
                "timeout": LONG_POLL,
                "allowed_updates": ["message"],
            });
            match self.bot.call::<Vec<Update>>("getUpdates", &params).await {
                Ok(updates) => {
                    retry.reset();
                    for update in updates {
                        offset = offset.max(update.update_id + 1);
                        if let Some(inbound) = update
                            .message
                            .as_ref()
                            .and_then(|m| convert::inbound(m, &self.me))
                        {
                            up.inbound(inbound).await;
                        }
                    }
                }
                Err(e) => {
                    if let Some(api) = e.downcast_ref::<ApiError>() {
                        match api.code {
                            // The token was revoked. Nothing to retry.
                            401 | 404 => return Err(e),
                            409 => warn!("another process is polling this bot; only one can"),
                            _ => warn!("{api}"),
                        }
                    } else {
                        warn!("{e:#}");
                    }
                    tokio::time::sleep(retry.wait()).await;
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
                let chat = chat_id(&place)?;
                let mut last = None;
                for (i, part) in text::chunks(&text, convert::MESSAGE_LIMIT)
                    .into_iter()
                    .enumerate()
                {
                    let mut params = json!({"chat_id": chat, "text": part});
                    if let (0, Some(id)) =
                        (i, reply_to.as_deref().and_then(|r| r.parse::<i64>().ok()))
                    {
                        params["reply_parameters"] =
                            json!({"message_id": id, "allow_sending_without_reply": true});
                    }
                    let sent: Value = self.bot.call("sendMessage", &params).await?;
                    last = sent["message_id"].as_i64();
                }
                Ok(Acted {
                    id: last.map(|id| id.to_string()),
                    items: Vec::new(),
                    measure: None,
                })
            }
            Action::Typing { place } => {
                let params = json!({"chat_id": chat_id(&place)?, "action": "typing"});
                self.bot.call::<Value>("sendChatAction", &params).await?;
                Ok(Acted::default())
            }
            Action::React { place, to, emoji } => {
                let params = json!({
                    "chat_id": chat_id(&place)?,
                    "message_id": to.parse::<i64>().context("message id")?,
                    "reaction": [{"type": "emoji", "emoji": emoji}],
                });
                // Telegram only allows a fixed set of emoji; anything else is refused.
                self.bot
                    .call::<Value>("setMessageReaction", &params)
                    .await?;
                Ok(Acted::default())
            }
            other => anyhow::bail!("telegram bots cannot {:?}", other.needs()),
        }
    }

    async fn appear(&self, card: &GhostCard) {
        if let Err(e) = self.introduce(card).await {
            warn!("could not set her name and bio on telegram: {e:#}");
        }
        // The Bot API has no call for a bot's own picture.
        presence::set_by_hand(card, &[Field::Avatar], "@BotFather (/setuserpic)");
    }
}

impl Telegram {
    /// Her name, and her bio as the bot's short description, which is what
    /// its profile shows. Each is asked for first and set only if it
    /// differs: Telegram limits how often they may be changed.
    async fn introduce(&self, card: &GhostCard) -> anyhow::Result<()> {
        let now: Value = self.bot.call("getMyName", &json!({})).await?;
        if now["name"].as_str() != Some(card.name.as_str()) {
            self.bot
                .call::<Value>("setMyName", &json!({"name": card.name}))
                .await?;
            info!("her name on telegram is now {}", card.name);
        }
        let Some(bio) = &card.bio else {
            return Ok(());
        };
        if bio.chars().count() > SHORT_DESCRIPTION {
            warn!(
                "her bio is longer than the {SHORT_DESCRIPTION} characters telegram allows a bot's \
                 short description; shorten it in her ghost.toml, or set one by hand with @BotFather"
            );
            return Ok(());
        }
        let now: Value = self.bot.call("getMyShortDescription", &json!({})).await?;
        if now["short_description"].as_str() != Some(bio.as_str()) {
            self.bot
                .call::<Value>("setMyShortDescription", &json!({"short_description": bio}))
                .await?;
            info!("her bio on telegram is set");
        }
        Ok(())
    }
}

fn chat_id(place: &str) -> anyhow::Result<i64> {
    place
        .parse()
        .with_context(|| format!("{place:?} is not a telegram chat id"))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Args = presence::init("info");

    let bot = Bot::new(&args.api_base, &args.bot_token)?;
    let me: User = bot
        .call("getMe", &json!({}))
        .await
        .context("log in to telegram")?;
    info!(
        "on telegram as @{}",
        me.username.as_deref().unwrap_or(&me.first_name)
    );
    let link = Link {
        daemon: args.daemon,
        me: Some(convert::actor(&me)),
        can: vec![Capability::Send, Capability::Typing, Capability::React],
        allows: ghost_link::protocol::Allows::default(),
    };
    presence::run(Arc::new(Telegram { bot, me }), link).await
}
