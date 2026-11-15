//! `ghost-discord`: carries a ghost on Discord.
//!
//! She is a bot account, connected through the gateway. Discord marks bots as
//! such on every message, which is the disclosure the charter asks for.
//! Automating a normal user account is against Discord's terms and is not
//! something this adapter does.

mod convert;

use std::{collections::HashMap, sync::Arc};

use anyhow::Context as _;
use async_trait::async_trait;
use clap::Parser;
use ghost_link::{
    presence::{self, Field, Link, Platform, Uplink},
    protocol::{Acted, Action, Capability, GhostCard},
    text,
};
use serenity::all::{
    Channel, ChannelId, ClientBuilder, Context, CreateMessage, EditProfile, EventHandler,
    GatewayIntents, Http, HttpBuilder, Message, MessageId, ReactionType, Ready, TypingStartEvent,
    UserId,
};
use tokio::sync::Mutex;
use tracing::{info, warn};

#[derive(Parser)]
#[command(name = "ghost-discord", version, about = "Carries a ghost on Discord.")]
struct Args {
    /// The bot token from the Discord developer portal.
    #[arg(long, env = "DISCORD_BOT_TOKEN", hide_env_values = true)]
    bot_token: String,
    /// The daemon's address.
    #[arg(long, default_value_t = ghost_link::default_url())]
    url: String,
    /// The daemon's token, if it has one.
    #[arg(long, env = "GHOSTD_TOKEN", hide_env_values = true)]
    token: Option<String>,
    /// The character this bot is: the name of her directory. Needed when
    /// the daemon runs more than one.
    #[arg(long, env = "GHOST_NAME")]
    ghost: Option<String>,
    /// The channel name to register as.
    #[arg(long, default_value = "discord")]
    channel: String,
}

struct Discord {
    token: String,
    http: Arc<Http>,
    me: UserId,
}

/// What the gateway hands events to.
struct Ears {
    up: Uplink,
    me: UserId,
    /// Names of guild channels, looked up once each. `None` for a DM.
    names: Mutex<HashMap<ChannelId, Option<String>>>,
}

impl Ears {
    async fn channel_name(&self, ctx: &Context, message: &Message) -> Option<String> {
        message.guild_id?;
        let mut names = self.names.lock().await;
        if let Some(known) = names.get(&message.channel_id) {
            return known.clone();
        }
        let name = match message.channel_id.to_channel(ctx).await {
            Ok(Channel::Guild(channel)) => Some(channel.name),
            _ => None,
        };
        names.insert(message.channel_id, name.clone());
        name
    }
}

#[serenity::async_trait]
impl EventHandler for Ears {
    async fn ready(&self, _ctx: Context, ready: Ready) {
        info!("the gateway is open; in {} server(s)", ready.guilds.len());
    }

    async fn message(&self, ctx: Context, message: Message) {
        let name = self.channel_name(&ctx, &message).await;
        if let Some(inbound) = convert::inbound(&message, self.me, name) {
            self.up.inbound(inbound).await;
        }
    }

    async fn typing_start(&self, _ctx: Context, event: TypingStartEvent) {
        if event.user_id != self.me {
            self.up.typing(event.channel_id.to_string());
        }
    }
}

#[async_trait]
impl Platform for Discord {
    async fn listen(&self, up: Uplink) -> anyhow::Result<()> {
        let intents = GatewayIntents::GUILD_MESSAGES
            | GatewayIntents::DIRECT_MESSAGES
            | GatewayIntents::MESSAGE_CONTENT
            | GatewayIntents::GUILD_MESSAGE_TYPING
            | GatewayIntents::DIRECT_MESSAGE_TYPING;
        let mut client = ClientBuilder::new_with_http(http(&self.token)?, intents)
            .event_handler(Ears {
                up,
                me: self.me,
                names: Mutex::new(HashMap::new()),
            })
            .await
            .context("set up the discord gateway")?;
        // Serenity reconnects and resumes on its own; this returns only on
        // something it cannot recover from, such as a revoked token or a
        // privileged intent that is not enabled.
        client.start().await.context("the discord gateway closed")
    }

    async fn act(&self, action: Action) -> anyhow::Result<Acted> {
        match action {
            Action::Send {
                place,
                text,
                reply_to,
            } => {
                let channel = channel_id(&place)?;
                let quoted = reply_to.as_deref().and_then(|id| id.parse::<u64>().ok());
                let mut last = None;
                for (i, part) in text::chunks(&text, convert::MESSAGE_LIMIT)
                    .into_iter()
                    .enumerate()
                {
                    let mut message = CreateMessage::new().content(part);
                    if let (0, Some(id)) = (i, quoted) {
                        message = message.reference_message((channel, MessageId::new(id)));
                    }
                    let sent = match channel.send_message(&self.http, message).await {
                        Ok(sent) => sent,
                        // The quoted message is gone. Say it anyway, unquoted.
                        Err(e) if i == 0 && quoted.is_some() => {
                            warn!("could not quote: {e}");
                            let part = text::chunks(&text, convert::MESSAGE_LIMIT).swap_remove(0);
                            channel
                                .send_message(&self.http, CreateMessage::new().content(part))
                                .await?
                        }
                        Err(e) => return Err(e.into()),
                    };
                    last = Some(sent.id.to_string());
                }
                Ok(Acted {
                    id: last,
                    items: Vec::new(),
                    measure: None,
                })
            }
            Action::Typing { place } => {
                channel_id(&place)?.broadcast_typing(&self.http).await?;
                Ok(Acted::default())
            }
            Action::React { place, to, emoji } => {
                let message = MessageId::new(to.parse().context("message id")?);
                channel_id(&place)?
                    .create_reaction(&self.http, message, ReactionType::Unicode(emoji))
                    .await?;
                Ok(Acted::default())
            }
            other => anyhow::bail!("discord bots cannot {:?}", other.needs()),
        }
    }

    /// Her name is the bot's username, set if it differs: Discord allows a
    /// couple of changes an hour. Her bio is the application's description
    /// and her picture the bot's icon, which are left to the developer
    /// portal: Discord gives back nothing to tell whether the icon there is
    /// already hers, and uploading it at every start would run into the
    /// same limit.
    async fn appear(&self, card: &GhostCard) {
        match self.http.get_current_user().await {
            Ok(me) if me.name == card.name => {}
            Ok(_) => {
                let rename = EditProfile::new().username(&card.name);
                match self.http.edit_profile(&rename).await {
                    Ok(_) => info!("her name on discord is now {}", card.name),
                    Err(e) => warn!(
                        "could not rename her to {} on discord ({e}); set it by hand in the developer portal",
                        card.name
                    ),
                }
            }
            Err(e) => warn!("could not look at her discord profile: {e}"),
        }
        presence::set_by_hand(
            card,
            &[Field::Bio, Field::Avatar],
            "the Discord developer portal (the application's description and the bot's icon)",
        );
    }
}

/// Discord's HTTP API as the bot, going the way the network setting says.
/// The gateway's own connection is a websocket, and is made directly.
fn http(token: &str) -> anyhow::Result<Http> {
    let client = ghost_net::builder()
        .build()
        .context("build an HTTP client")?;
    Ok(HttpBuilder::new(token).client(client).build())
}

fn channel_id(place: &str) -> anyhow::Result<ChannelId> {
    let id: u64 = place
        .parse()
        .with_context(|| format!("{place:?} is not a discord channel id"))?;
    anyhow::ensure!(id != 0, "0 is not a discord channel id");
    Ok(ChannelId::new(id))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Keys and settings may sit in an ignored `.env` file in the directory
    // this runs from or one above it. Anything already set in the
    // environment wins.
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,serenity=warn".into()),
        )
        .with_target(false)
        .with_writer(std::io::stderr)
        .init();
    let args = Args::parse();

    let http = Arc::new(http(&args.bot_token)?);
    let me = http.get_current_user().await.context("log in to discord")?;
    info!("on discord as {}", me.name);
    let link = Link {
        url: args.url,
        token: args.token,
        ghost: args.ghost,
        channel: args.channel,
        me: Some(convert::actor(&me, None)),
        can: vec![Capability::Send, Capability::Typing, Capability::React],
    };
    let discord = Discord {
        token: args.bot_token,
        http,
        me: me.id,
    };
    presence::run(Arc::new(discord), link).await
}
