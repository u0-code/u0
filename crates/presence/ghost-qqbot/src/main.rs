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
    presence::{self, Field, Link, Platform, Uplink},
    protocol::{Acted, Action, Actor, Capability, GhostCard},
    text,
};
use reqwest::Method;
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::{api::Api, convert::Me, gateway::Event, replies::Replies};

/// How long to wait for the gateway to accept the bot before giving up.
const READY_WITHIN: Duration = Duration::from_secs(60);
/// The most QQ takes in one upload, as Tencent's own SDK has it.
const LARGEST_UPLOAD: usize = 20 * 1024 * 1024;

#[derive(Parser)]
#[command(
    name = "ghost-qqbot",
    version,
    about = "Carries a ghost on QQ through the official QQ Bot API.",
    mut_arg("channel", |a| a.default_value("qq"))
)]
struct Args {
    /// The bot's `AppID`, from the QQ open platform.
    #[arg(long, env = "QQBOT_APPID")]
    app_id: String,
    /// The bot's `AppSecret`, from the QQ open platform.
    #[arg(long, env = "QQBOT_SECRET", hide_env_values = true)]
    app_secret: String,
    #[command(flatten)]
    daemon: presence::Daemon,
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
        let id = match action {
            Action::Send {
                place,
                text,
                reply_to,
            } => self.say(&place, &text, reply_to.as_deref()).await?,
            Action::File {
                place,
                name,
                data,
                text,
            } => {
                self.hand_over(&place, &name, &data, text.as_deref())
                    .await?
            }
            other => anyhow::bail!("qq bots cannot {:?} here", other.needs()),
        };
        Ok(Acted {
            id,
            items: Vec::new(),
            measure: None,
        })
    }

    /// The bot API has no call for a bot's name, description or avatar, as
    /// far as its documentation says: they are set on the open platform.
    async fn appear(&self, card: &GhostCard) {
        presence::set_by_hand(
            card,
            &[Field::Name, Field::Bio, Field::Avatar],
            "the QQ open platform's bot settings (q.qq.com)",
        );
    }
}

impl QqBot {
    /// Send `text` to `place`, in as many messages as it takes. Returns the
    /// id of the last.
    async fn say(
        &self,
        place: &str,
        text: &str,
        reply_to: Option<&str>,
    ) -> anyhow::Result<Option<String>> {
        let path =
            convert::target(place).with_context(|| format!("{place:?} is not somewhere on qq"))?;
        let mut last = None;
        for part in text::chunks(text, convert::MESSAGE_LIMIT) {
            let passive = self.replies().answer(place, reply_to, Instant::now());
            let body = convert::outgoing(&part, passive.as_ref());
            let sent = self
                .api
                .call(Method::POST, &path, Some(&body))
                .await
                .with_context(|| sending(passive.is_some()))?;
            last = sent["id"].as_str().map(str::to_owned);
        }
        Ok(last)
    }

    /// Send an image to `place`: uploaded first, then sent as a message
    /// that carries it, after `text` if there is any. QQ takes other kinds
    /// of file from a bot only in ways not built here, so they are refused.
    async fn hand_over(
        &self,
        place: &str,
        name: &str,
        data: &str,
        text: Option<&str>,
    ) -> anyhow::Result<Option<String>> {
        use base64::Engine as _;
        let (messages, uploads) = convert::target(place)
            .zip(convert::uploads(place))
            .with_context(|| format!("{place:?} is not somewhere on qq"))?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data)
            .with_context(|| format!("{name} did not come as base64"))?;
        if !convert::is_image(&bytes) {
            anyhow::bail!(
                "{name} is not an image: a qq bot here sends images (PNG, JPEG, GIF or WebP) and no other files"
            );
        }
        if bytes.len() > LARGEST_UPLOAD {
            anyhow::bail!(
                "{name} is {} bytes, more than qq takes in one upload",
                bytes.len()
            );
        }
        // Uploaded before anything is said, so that an image qq will not
        // take leaves no words behind waiting for it.
        let uploaded = self
            .api
            .call(Method::POST, &uploads, Some(&convert::upload(data)))
            .await
            .context("upload an image to qq")?;
        let file_info = uploaded["file_info"]
            .as_str()
            .filter(|f| !f.is_empty())
            .context("qq took the image and gave nothing to send it with")?;
        let mut last = match text.map(str::trim).filter(|t| !t.is_empty()) {
            Some(text) => self.say(place, text, None).await?,
            None => None,
        };
        let passive = self.replies().answer(place, None, Instant::now());
        let sent = self
            .api
            .call(
                Method::POST,
                &messages,
                Some(&convert::media(file_info, passive.as_ref())),
            )
            .await
            .with_context(|| sending(passive.is_some()))?;
        if let Some(id) = sent["id"].as_str() {
            last = Some(id.to_owned());
        }
        Ok(last)
    }
}

/// What was being done when a send was refused.
fn sending(passive: bool) -> &'static str {
    if passive {
        "send a reply on qq"
    } else {
        // Most refusals come from here: QQ allows a bot to speak first
        // only rarely.
        "send on qq with nothing recent here to answer, which qq allows only rarely"
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Args = presence::init("info");

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
        daemon: args.daemon,
        me: Some(Actor {
            id: me.id,
            name: me.name,
            handle: None,
            automated: true,
        }),
        can: vec![Capability::Send, Capability::Files],
        // QQ lets a bot answer a message only for a while after it came,
        // and seldom lets it write first.
        allows: ghost_link::protocol::Allows {
            answer_direct_secs: Some(30 * 60),
            answer_group_secs: Some(5 * 60),
            writes_first: false,
        },
    };
    let bot = QqBot {
        api,
        replies: Mutex::new(Replies::default()),
        events: tokio::sync::Mutex::new(Some(events)),
    };
    presence::run(Arc::new(bot), link).await
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use base64::Engine as _;
    use serde_json::{Value, json};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    use super::*;

    /// One request the stand-in for QQ was sent: its path and its body.
    type Seen = Arc<Mutex<Vec<(String, Value)>>>;

    /// A stand-in for QQ's token service and open API on this machine. It
    /// hands out a token, takes any upload, and gives each message an id.
    async fn stand_in() -> (String, Seen) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let seen: Seen = Arc::default();
        let log = seen.clone();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let mut got = Vec::new();
                let mut buf = [0u8; 8192];
                // The head, then as much body as it says there is.
                let (head, body) = loop {
                    let n = socket.read(&mut buf).await.unwrap();
                    got.extend_from_slice(&buf[..n]);
                    let text = String::from_utf8_lossy(&got).into_owned();
                    let Some(end) = text.find("\r\n\r\n") else {
                        continue;
                    };
                    let length = text[..end]
                        .lines()
                        .find_map(|l| {
                            let (k, v) = l.split_once(':')?;
                            k.eq_ignore_ascii_case("content-length")
                                .then(|| v.trim().parse::<usize>().ok())?
                        })
                        .unwrap_or(0);
                    if got.len() >= end + 4 + length {
                        break (
                            text[..end].to_owned(),
                            got[end + 4..end + 4 + length].to_vec(),
                        );
                    }
                    if n == 0 {
                        break (text[..end].to_owned(), got[end + 4..].to_vec());
                    }
                };
                let path = head
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or_default()
                    .to_owned();
                let body: Value = serde_json::from_slice(&body).unwrap_or_default();
                let answer = if path.ends_with("/getAppAccessToken") {
                    json!({"access_token": "t-1", "expires_in": 7200})
                } else if path.ends_with("/files") {
                    json!({"file_uuid": "u-1", "file_info": "INFO-1", "ttl": 3600})
                } else {
                    let seen = log.lock().unwrap();
                    json!({"id": format!("sent-{}", seen.len() + 1)})
                };
                if !path.ends_with("/getAppAccessToken") {
                    log.lock().unwrap().push((path, body));
                }
                let answer = answer.to_string();
                let reply = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{answer}",
                    answer.len()
                );
                socket.write_all(reply.as_bytes()).await.unwrap();
            }
        });
        (base, seen)
    }

    fn bot(base: &str) -> QqBot {
        QqBot {
            api: Arc::new(Api::new(base, base, "app", "secret").unwrap()),
            replies: Mutex::new(Replies::default()),
            events: tokio::sync::Mutex::new(None),
        }
    }

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";

    fn file(place: &str, name: &str, bytes: &[u8], text: Option<&str>) -> Action {
        Action::File {
            place: place.into(),
            name: name.into(),
            data: base64::engine::general_purpose::STANDARD.encode(bytes),
            text: text.map(str::to_owned),
        }
    }

    #[tokio::test]
    async fn an_image_is_uploaded_then_sent_as_a_media_message_after_her_words() {
        let (base, seen) = stand_in().await;
        let bot = bot(&base);
        bot.replies().received("c2c:U1", "m-1", Instant::now());
        let data = base64::engine::general_purpose::STANDARD.encode(PNG);

        let acted = bot
            .act(file("c2c:U1", "a.png", PNG, Some("这个")))
            .await
            .unwrap();

        let seen = seen.lock().unwrap().clone();
        assert_eq!(
            seen,
            [
                (
                    "/v2/users/U1/files".to_owned(),
                    json!({"file_type": 1, "file_data": data, "srv_send_msg": false})
                ),
                (
                    "/v2/users/U1/messages".to_owned(),
                    json!({"content": "这个", "msg_type": 0, "msg_id": "m-1", "msg_seq": 1})
                ),
                (
                    "/v2/users/U1/messages".to_owned(),
                    json!({"msg_type": 7, "media": {"file_info": "INFO-1"}, "msg_id": "m-1", "msg_seq": 2})
                ),
            ]
        );
        // Each message is charged to what she was sent; the id is the image's.
        assert_eq!(acted.id.as_deref(), Some("sent-3"));
    }

    #[tokio::test]
    async fn in_a_group_an_image_alone_is_uploaded_there_and_sent_with_no_words() {
        let (base, seen) = stand_in().await;
        let bot = bot(&base);
        bot.replies().received("group:G7", "m-9", Instant::now());
        bot.act(file("group:G7", "a.png", PNG, None)).await.unwrap();
        let paths: Vec<String> = seen
            .lock()
            .unwrap()
            .iter()
            .map(|(p, _)| p.clone())
            .collect();
        assert_eq!(paths, ["/v2/groups/G7/files", "/v2/groups/G7/messages"]);
        assert_eq!(seen.lock().unwrap()[1].1["msg_type"], 7);
    }

    #[tokio::test]
    async fn a_file_that_is_not_an_image_is_refused_and_nothing_is_sent() {
        let (base, seen) = stand_in().await;
        let bot = bot(&base);
        bot.replies().received("c2c:U1", "m-1", Instant::now());
        let refused = bot
            .act(file("c2c:U1", "notes.pdf", b"%PDF-1.7 ...", Some("here")))
            .await
            .unwrap_err();
        assert!(
            format!("{refused:#}").contains("notes.pdf is not an image"),
            "{refused:#}"
        );
        assert!(seen.lock().unwrap().is_empty(), "not even her words go out");
    }
}
