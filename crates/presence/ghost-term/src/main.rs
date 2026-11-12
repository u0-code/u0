//! `ghost-term`: a presence in your terminal.
//!
//! Type a line and it reaches her as a message. What she says, when she gets
//! around to saying it, is printed. This is the reference adapter: everything
//! a real one does, with stdin and stdout standing in for a platform.
//!
//! ```text
//! ghost-term --as haoran                 a private conversation
//! ghost-term --as haoran --group tea     a group; "mei: hello" speaks as mei,
//!                                        and a line containing @ addresses her
//! ```
//!
//! A line of the form `/wait 20` pauses for that many seconds before the next
//! one, which makes piped scripts behave like a person.

use std::{
    io::Write,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use clap::Parser;
use ghost_link::{
    Console,
    presence::{self, Link, Platform, Uplink},
    protocol::{Acted, Action, Actor, Capability, Inbound, Place, PlaceKind},
};
use tokio::io::{AsyncBufReadExt, BufReader};

#[derive(Parser)]
#[command(
    name = "ghost-term",
    version,
    about = "Talk to a running ghost from your terminal."
)]
struct Args {
    /// The daemon's address.
    #[arg(long, default_value_t = ghost_link::default_url())]
    url: String,
    /// The daemon's token, if it has one.
    #[arg(long, env = "GHOSTD_TOKEN", hide_env_values = true)]
    token: Option<String>,
    /// Who you are.
    #[arg(long = "as", default_value = "you")]
    who: String,
    /// Talk in a group of this name instead of privately.
    #[arg(long)]
    group: Option<String>,
    /// The channel name to register as.
    #[arg(long, default_value = "term")]
    channel: String,
    /// After input ends, stay this many seconds to hear what she says.
    #[arg(long, default_value_t = 0)]
    linger: u64,
}

struct Term {
    who: String,
    group: Option<String>,
    her: String,
    linger: Duration,
    /// Ids for messages in both directions.
    next_id: AtomicU64,
    /// Whether the last thing printed was a typing indicator.
    typing_shown: AtomicBool,
}

impl Term {
    fn id(&self) -> String {
        // Unique across restarts, so redelivery checks never see a false repeat.
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis());
        format!("{stamp}-{}", self.next_id.fetch_add(1, Ordering::Relaxed))
    }

    fn place(&self) -> Place {
        match &self.group {
            Some(name) => Place {
                id: format!("group-{}", name.to_lowercase()),
                kind: PlaceKind::Group,
                name: Some(name.clone()),
            },
            None => Place {
                id: self.who.to_lowercase(),
                kind: PlaceKind::Direct,
                name: None,
            },
        }
    }

    /// Turn a typed line into a message. In a group, `name: text` speaks as
    /// someone else and `@` anywhere addresses her.
    fn message(&self, line: &str) -> Inbound {
        let (speaker, text) = match (&self.group, line.split_once(": ")) {
            (Some(_), Some((name, text)))
                if !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_') =>
            {
                (name.to_owned(), text)
            }
            _ => (self.who.clone(), line),
        };
        Inbound {
            place: self.place(),
            from: Actor {
                id: speaker.to_lowercase(),
                name: speaker,
                handle: None,
                automated: false,
            },
            id: self.id(),
            text: text.to_owned(),
            reply_to: None,
            addressed: self.group.is_none() || text.contains('@'),
        }
    }
}

#[async_trait]
impl Platform for Term {
    async fn listen(&self, up: Uplink) -> anyhow::Result<()> {
        let mut lines = BufReader::new(tokio::io::stdin()).lines();
        while let Some(line) = lines.next_line().await? {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Some(seconds) = line
                .strip_prefix("/wait ")
                .and_then(|s| s.trim().parse().ok())
            {
                tokio::time::sleep(Duration::from_secs(seconds)).await;
                continue;
            }
            up.inbound(self.message(line)).await;
        }
        tokio::time::sleep(self.linger).await;
        Ok(())
    }

    async fn act(&self, action: Action) -> anyhow::Result<Acted> {
        match action {
            Action::Send { text, .. } => {
                self.typing_shown.store(false, Ordering::Relaxed);
                println!("{}: {text}", self.her);
                std::io::stdout().flush()?;
                Ok(Acted {
                    id: Some(self.id()),
                    items: Vec::new(),
                })
            }
            Action::Typing { .. } => {
                if !self.typing_shown.swap(true, Ordering::Relaxed) {
                    eprintln!("  ({} is typing)", self.her);
                }
                Ok(Acted::default())
            }
            Action::React { emoji, .. } => {
                println!("  ({} reacted {emoji})", self.her);
                Ok(Acted::default())
            }
            other => anyhow::bail!("a terminal cannot {:?}", other.needs()),
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into()),
        )
        .with_target(false)
        .with_writer(std::io::stderr)
        .init();
    let args = Args::parse();

    // Ask who is there before settling in.
    let (_, welcome) = Console::connect(&args.url, args.token.clone()).await?;
    let her = welcome.ghost.name;
    eprintln!(
        "{her} is in {}. She answers when she gets to it. Ctrl-D to leave.",
        welcome.ghost.city
    );

    let term = Arc::new(Term {
        who: args.who,
        group: args.group,
        her: her.clone(),
        linger: Duration::from_secs(args.linger),
        next_id: AtomicU64::new(1),
        typing_shown: AtomicBool::new(false),
    });
    presence::run(
        term,
        Link {
            url: args.url,
            token: args.token,
            channel: args.channel,
            me: Some(Actor {
                id: "ghost".into(),
                name: her,
                handle: None,
                automated: true,
            }),
            can: vec![Capability::Send, Capability::Typing, Capability::React],
        },
    )
    .await
}
