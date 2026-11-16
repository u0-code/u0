//! Several characters in one daemon, over the real protocol on this
//! machine's loopback: each one's mind is real, her brain is scripted, and
//! her presence is a test client that answers whatever she does. The rig
//! is used by the daemon's other tests over the protocol too.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use base64::Engine as _;
use ghost_brain::{Brain, Completion, Reply};
use ghost_core::Ghost;
use ghost_link::Conn;
use ghost_mind::{Mind, MindConfig, Numb, Parts, Prompts, Question, WorkConfig, Workshop};
use ghost_protocol::{
    Acted, Action, Actor, Capability, Frame, GhostCard, Hello, Inbound, ListGhosts, PROTOCOL,
    Place, PlaceKind, PresenceHello, Role, Roster, RpcError, Welcome, code, method,
};
use tokio::{net::TcpListener, sync::mpsc};

use super::*;
use crate::server::{self, Presences, Shared};

/// A brain that always says one thing, and counts how often it is asked.
pub(crate) struct Says {
    line: String,
    asked: AtomicUsize,
}

impl Says {
    pub(crate) fn new(line: &str) -> Arc<Self> {
        Arc::new(Self {
            line: line.to_owned(),
            asked: AtomicUsize::new(0),
        })
    }

    fn asked(&self) -> usize {
        self.asked.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl Brain for Says {
    fn label(&self) -> String {
        "says".into()
    }

    async fn complete(&self, _: &Completion) -> ghost_brain::Result<Reply> {
        self.asked.fetch_add(1, Ordering::SeqCst);
        Ok(Reply {
            text: format!(
                r#"{{"thought":"","actions":[{{"do":"say","text":"{}"}}]}}"#,
                self.line
            ),
            ..Reply::default()
        })
    }
}

/// One character to run: her id, her brain, how she lives, and whether her
/// first mind falls over.
pub(crate) struct Spec {
    pub(crate) id: &'static str,
    pub(crate) brain: Arc<Says>,
    pub(crate) config: MindConfig,
    pub(crate) falls_first: bool,
    /// Done to her directory after she is born and before her mind starts.
    pub(crate) before: Option<fn(&Path)>,
}

impl Spec {
    pub(crate) fn new(id: &'static str) -> Self {
        Self {
            id,
            brain: Says::new(&format!("{id} here")),
            config: quick(),
            falls_first: false,
            before: None,
        }
    }
}

/// A mind with every wait taken out, no day of her own, and no work: she
/// answers at once whatever the hour.
fn quick() -> MindConfig {
    MindConfig {
        tempo: 0.0,
        live: false,
        weather: false,
        talk_tools: false,
        browse_minutes: [0, 0],
        work: WorkConfig {
            enabled: false,
            ..WorkConfig::default()
        },
        ..MindConfig::default()
    }
}

/// A daemon running the characters in `specs`, listening on loopback.
pub(crate) struct Rig {
    pub(crate) url: String,
    pub(crate) cast: Vec<Arc<Character>>,
    /// How many times each one's mind has been made.
    made: Vec<Arc<AtomicUsize>>,
    stop: watch::Sender<bool>,
    _root: tempfile::TempDir,
}

impl Rig {
    pub(crate) async fn run(specs: Vec<Spec>) -> Self {
        Self::guarded(specs, None).await
    }

    /// The same, admitting only clients that give `token`.
    pub(crate) async fn guarded(specs: Vec<Spec>, token: Option<&str>) -> Self {
        let root = tempfile::tempdir().unwrap();
        let (stop, stopping) = watch::channel(false);
        let pauses = Pauses {
            first: Duration::from_millis(20),
            most: Duration::from_millis(200),
            steady: Duration::from_secs(60),
        };
        let (mut cast, mut counts, mut lives) = (Vec::new(), Vec::new(), Vec::new());
        for spec in specs {
            let dir = root.path().join(spec.id);
            std::fs::create_dir(&dir).unwrap();
            std::fs::write(
                dir.join(PERSONA),
                include_str!("../../../../ghosts/_template/ghost.toml"),
            )
            .unwrap();
            Ghost::birth(&dir).unwrap();
            if let Some(before) = spec.before {
                before(&dir);
            }
            let body = Arc::new(Presences::default());
            let count = Arc::new(AtomicUsize::new(0));
            let make = maker(&dir, &spec, &body, &count);
            let life = make().unwrap();
            let card = GhostCard {
                name: spec.id.into(),
                pronouns: "she/her".into(),
                city: "here".into(),
                country: "CN".into(),
                seal: "seal".into(),
                bio: None,
            };
            let who = Arc::new(Character::new(
                spec.id.into(),
                dir,
                card,
                body,
                life.handle.clone(),
            ));
            cast.push(who.clone());
            counts.push(count);
            lives.push((who, life, make));
        }
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}", listener.local_addr().unwrap());
        tokio::spawn(server::serve(
            listener,
            Arc::new(Shared {
                cast: Cast(cast.clone()),
                token: token.map(str::to_owned),
                origins: Vec::new(),
            }),
        ));
        for (who, life, make) in lives {
            tokio::spawn(keep(who, life, make, stopping.clone(), pauses));
        }
        Self {
            url,
            cast,
            made: counts,
            stop,
            _root: root,
        }
    }

    /// Everything in the journal of the character at `n`, as text.
    pub(crate) async fn journal(&self, n: usize) -> String {
        let entries = self.cast[n]
            .mind()
            .ask(Question::Journal {
                from: 1,
                limit: 10_000,
            })
            .await
            .unwrap();
        entries.to_string()
    }

    /// Wait until the journal of the character at `n` has `text` in it.
    pub(crate) async fn until_written(&self, n: usize, text: &str) {
        for _ in 0..200 {
            if self.journal(n).await.contains(text) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!("{text:?} never reached {}'s journal", self.cast[n].id);
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        self.stop.send_replace(true);
    }
}

/// How to make the mind of `spec` from her directory. If she falls first,
/// the first mind made panics a moment after it starts.
fn maker(dir: &Path, spec: &Spec, body: &Arc<Presences>, made: &Arc<AtomicUsize>) -> Maker {
    let (dir, brain, config, body, made) = (
        dir.to_path_buf(),
        spec.brain.clone(),
        spec.config.clone(),
        body.clone(),
        made.clone(),
    );
    let falls = AtomicBool::new(spec.falls_first);
    Box::new(move || {
        made.fetch_add(1, Ordering::SeqCst);
        let (mind, handle) = Mind::new(Parts {
            ghost: Ghost::open(&dir)?,
            think: brain.clone(),
            tick: brain.clone(),
            body: body.clone(),
            senses: Arc::new(Numb),
            prompts: Prompts::shipped(),
            config: config.clone(),
            workshop: Workshop::default(),
        });
        if falls.swap(false, Ordering::SeqCst) {
            drop(mind);
            return Ok(Life {
                run: Box::pin(async {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    panic!("a mind that falls over, on purpose");
                }),
                handle,
            });
        }
        Ok(Life {
            run: Box::pin(mind.run()),
            handle,
        })
    })
}

/// A presence on the channel `t` for the character named `ghost`. It
/// answers every action with success and hands on what she said.
struct Client {
    up: mpsc::Sender<Frame>,
    said: mpsc::Receiver<String>,
    next: usize,
}

impl Client {
    async fn connect(url: &str, ghost: Option<&str>) -> anyhow::Result<Self> {
        let hello = Hello {
            protocol: PROTOCOL,
            role: Role::Presence,
            token: None,
            ghost: ghost.map(Into::into),
            presence: Some(PresenceHello {
                channel: "t".into(),
                me: None,
                can: vec![Capability::Send],
            }),
            who: None,
        };
        let (mut conn, _) = Conn::open(url, &hello).await?;
        let (up, mut going) = mpsc::channel::<Frame>(64);
        let (saying, said) = mpsc::channel(64);
        tokio::spawn(async move {
            loop {
                let frame = tokio::select! {
                    Some(frame) = going.recv() => {
                        if conn.send(&frame).await.is_err() {
                            break;
                        }
                        continue;
                    }
                    frame = conn.recv() => frame,
                };
                let Ok(Some(frame)) = frame else { break };
                let (Some(id), Some(method::ACT)) = (frame.id, frame.method.as_deref()) else {
                    continue;
                };
                if let Ok(Action::Send { text, .. }) = frame.params::<Action>() {
                    let _ = saying.send(text).await;
                }
                let done = Acted {
                    id: Some(format!("out-{id}")),
                    ..Acted::default()
                };
                if conn.send(&Frame::ok(id, &done)).await.is_err() {
                    break;
                }
            }
        });
        Ok(Self { up, said, next: 0 })
    }

    /// Someone called fox writes to her privately.
    async fn write(&mut self, text: &str) {
        self.next += 1;
        let message = Inbound {
            place: Place {
                id: "fox".into(),
                kind: PlaceKind::Direct,
                name: None,
            },
            from: Actor {
                id: "fox".into(),
                name: "fox".into(),
                handle: None,
                automated: false,
            },
            id: format!("m{}", self.next),
            text: text.into(),
            reply_to: None,
            addressed: true,
            images: Vec::new(),
        };
        self.up
            .send(Frame::notify(method::INBOUND, &message))
            .await
            .unwrap();
    }

    /// The next thing she says here, if she says something within a while.
    async fn hears(&mut self) -> Option<String> {
        tokio::time::timeout(Duration::from_secs(10), self.said.recv())
            .await
            .ok()
            .flatten()
    }
}

/// The code a hello naming `ghost` is refused with, or `None` if it is
/// taken.
async fn refusal(url: &str, ghost: Option<&str>) -> Option<i64> {
    match Client::connect(url, ghost).await {
        Ok(_) => None,
        Err(e) => Some(
            e.downcast_ref::<RpcError>()
                .unwrap_or_else(|| panic!("not refused by the daemon: {e:#}"))
                .code,
        ),
    }
}

#[tokio::test]
async fn a_message_to_one_character_reaches_her_alone() {
    let rig = Rig::run(vec![Spec::new("lin"), Spec::new("mei")]).await;
    let mut lin = Client::connect(&rig.url, Some("lin")).await.unwrap();
    let mut mei = Client::connect(&rig.url, Some("mei")).await.unwrap();

    lin.write("only for lin, 7f3a").await;
    mei.write("only for mei, 9c1b").await;
    // Each answers on her own presence, with her own brain.
    assert_eq!(lin.hears().await.as_deref(), Some("lin here"));
    assert_eq!(mei.hears().await.as_deref(), Some("mei here"));

    let (theirs, hers) = (rig.journal(0).await, rig.journal(1).await);
    assert!(theirs.contains("7f3a") && !theirs.contains("9c1b"));
    assert!(hers.contains("9c1b") && !hers.contains("7f3a"));
    // Neither said anything of the other's.
    assert!(!theirs.contains("mei here") && !hers.contains("lin here"));
}

#[tokio::test]
async fn one_character_spending_her_budget_leaves_the_other_answering() {
    let mut spent = Spec::new("lin");
    spent.config.calls_per_hour = 1;
    let other = Spec::new("mei");
    let (lin_brain, mei_brain) = (spent.brain.clone(), other.brain.clone());
    let rig = Rig::run(vec![spent, other]).await;
    let mut lin = Client::connect(&rig.url, Some("lin")).await.unwrap();
    let mut mei = Client::connect(&rig.url, Some("mei")).await.unwrap();

    lin.write("first").await;
    assert_eq!(lin.hears().await.as_deref(), Some("lin here"));
    // Her one call this hour is gone. What comes now she hears, and does
    // not think about.
    lin.write("second, 4d2e").await;
    rig.until_written(0, "4d2e").await;

    for n in 0..3 {
        mei.write(&format!("mei, {n}")).await;
        assert_eq!(mei.hears().await.as_deref(), Some("mei here"), "{n}");
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(lin_brain.asked(), 1);
    assert!(lin.said.try_recv().is_err());
    assert!(mei_brain.asked() >= 3);
}

#[tokio::test]
async fn a_hello_must_name_a_character_the_daemon_runs_when_it_runs_several() {
    let two = Rig::run(vec![Spec::new("lin"), Spec::new("mei")]).await;
    assert_eq!(
        refusal(&two.url, Some("bo")).await,
        Some(code::NO_SUCH_GHOST)
    );
    assert_eq!(refusal(&two.url, None).await, Some(code::NO_SUCH_GHOST));
    assert_eq!(refusal(&two.url, Some("mei")).await, None);

    // One alone is whoever a hello is for, unless it names someone else.
    let one = Rig::run(vec![Spec::new("lin")]).await;
    assert_eq!(refusal(&one.url, None).await, None);
    assert_eq!(refusal(&one.url, Some("lin")).await, None);
    assert_eq!(
        refusal(&one.url, Some("mei")).await,
        Some(code::NO_SUCH_GHOST)
    );
    // A console is held to the same.
    let console = ghost_link::Console::connect(&two.url, None, None).await;
    assert!(console.is_err());
    let (_, welcome) = ghost_link::Console::connect(&two.url, None, Some("mei".into()))
        .await
        .unwrap();
    assert_eq!(welcome.ghost.name, "mei");
}

#[tokio::test]
async fn a_mind_that_falls_over_is_started_again_while_the_other_keeps_answering() {
    let mut falls = Spec::new("lin");
    falls.falls_first = true;
    let rig = Rig::run(vec![falls, Spec::new("mei")]).await;
    // Connected before she falls, and never reconnected.
    let mut lin = Client::connect(&rig.url, Some("lin")).await.unwrap();
    let mut mei = Client::connect(&rig.url, Some("mei")).await.unwrap();

    mei.write("are you there").await;
    assert_eq!(mei.hears().await.as_deref(), Some("mei here"));
    // Written to while she may be down: it waits for her next mind.
    lin.write("after the fall, 2b7c").await;
    assert_eq!(lin.hears().await.as_deref(), Some("lin here"));
    assert!(rig.made[0].load(Ordering::SeqCst) >= 2);
    assert!(rig.journal(0).await.contains("2b7c"));
    // The other was made once, and is still answering.
    assert_eq!(rig.made[1].load(Ordering::SeqCst), 1);
    mei.write("still there?").await;
    assert_eq!(mei.hears().await.as_deref(), Some("mei here"));
}

#[tokio::test]
async fn each_character_hands_out_her_own_avatar_and_one_without_hands_out_none() {
    let rig = Rig::run(vec![Spec::new("lin"), Spec::new("mei")]).await;
    let assets = rig.cast[0].dir.join(ghost_core::ghost::ASSETS_DIR);
    std::fs::create_dir_all(&assets).unwrap();
    std::fs::write(assets.join("avatar.png"), b"lin's face").unwrap();

    let avatar = |ghost: &'static str| {
        let url = rig.url.clone();
        async move {
            let (mut console, _) = ghost_link::Console::connect(&url, None, Some(ghost.into()))
                .await
                .unwrap();
            console.avatar().await.unwrap()
        }
    };
    let hers = avatar("lin").await.unwrap();
    assert_eq!(hers.mime, "image/png");
    assert_eq!(
        base64::engine::general_purpose::STANDARD
            .decode(hers.data)
            .unwrap(),
        b"lin's face"
    );
    assert_eq!(avatar("mei").await, None);
}

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// A connection to the daemon that has said nothing yet.
async fn bare(url: &str) -> Socket {
    tokio_tungstenite::connect_async(url).await.unwrap().0
}

/// Send `frame`, and read what the daemon says back: `None` once it has
/// closed the connection.
async fn exchange(ws: &mut Socket, frame: &Frame) -> Option<Frame> {
    let _ = ghost_link::socket::send(ws, frame).await;
    ghost_link::socket::recv(ws).await.ok().flatten()
}

fn hello_for(ghost: &str) -> Frame {
    let hello = Hello {
        protocol: PROTOCOL,
        role: Role::Console,
        token: None,
        ghost: Some(ghost.into()),
        presence: None,
        who: None,
    };
    Frame::request(2, method::HELLO, &hello)
}

fn list_asking(token: Option<&str>) -> Frame {
    let asked = ListGhosts {
        token: token.map(Into::into),
    };
    Frame::request(1, method::LIST, &asked)
}

/// The ids in a roster, in its order.
fn ids(roster: &Roster) -> Vec<&str> {
    roster.ghosts.iter().map(|g| g.id.as_str()).collect()
}

/// The code a refused answer carries.
fn refused_with(e: &anyhow::Error) -> Option<i64> {
    e.downcast_ref::<RpcError>().map(|e| e.code)
}

#[tokio::test]
async fn a_daemon_running_one_character_names_her_before_any_hello_and_hello_keeps_its_rules() {
    let rig = Rig::run(vec![Spec::new("lin")]).await;
    let roster = ghost_link::list(&rig.url, None).await.unwrap();
    assert_eq!(roster.protocol, PROTOCOL);
    assert_eq!(ids(&roster), ["lin"]);
    let lin = &roster.ghosts[0];
    assert_eq!(
        (lin.card.name.as_str(), lin.card.seal.as_str()),
        ("lin", "seal")
    );
    assert!(
        lin.awake.is_some(),
        "her mind is running, so her day is known"
    );
    assert!(!lin.avatar);

    // Listed first, asked with no params at all, then a hello on the same
    // connection, which is taken...
    let mut ws = bare(&rig.url).await;
    let bare_ask = Frame::request(1, method::LIST, &());
    assert_eq!(bare_ask.params, None);
    let listed = exchange(&mut ws, &bare_ask).await.unwrap();
    assert_eq!(ids(&listed.into_result::<Roster>().unwrap()), ["lin"]);
    let welcome = exchange(&mut ws, &hello_for("lin")).await.unwrap();
    assert_eq!(welcome.into_result::<Welcome>().unwrap().ghost.name, "lin");
    // ...or refused, by the rules a hello always had.
    let mut ws = bare(&rig.url).await;
    exchange(&mut ws, &list_asking(None)).await.unwrap();
    let refused = exchange(&mut ws, &hello_for("mei")).await.unwrap();
    assert_eq!(refused.error.map(|e| e.code), Some(code::NO_SUCH_GHOST));
    // Anything else before a hello is refused as it was.
    let mut ws = bare(&rig.url).await;
    let status = exchange(&mut ws, &Frame::request(1, method::STATUS, &())).await;
    assert_eq!(
        status.and_then(|f| f.error).map(|e| e.code),
        Some(code::INVALID_REQUEST)
    );
}

#[tokio::test]
async fn a_daemon_running_several_lists_each_in_order_with_her_own_card_and_picture() {
    let rig = Rig::run(vec![Spec::new("lin"), Spec::new("mei")]).await;
    let assets = rig.cast[1].dir.join(ghost_core::ghost::ASSETS_DIR);
    std::fs::create_dir_all(&assets).unwrap();
    std::fs::write(assets.join("avatar.png"), b"mei's face").unwrap();

    let roster = ghost_link::list(&rig.url, None).await.unwrap();
    assert_eq!(ids(&roster), ["lin", "mei"]);
    let names: Vec<&str> = roster.ghosts.iter().map(|g| g.card.name.as_str()).collect();
    assert_eq!(names, ["lin", "mei"]);
    let pictures: Vec<bool> = roster.ghosts.iter().map(|g| g.avatar).collect();
    assert_eq!(pictures, [false, true]);
    assert!(roster.ghosts.iter().all(|g| g.awake.is_some()));

    // A console for one of them may still ask who else there is.
    let (mut console, _) = ghost_link::Console::connect(&rig.url, None, Some("lin".into()))
        .await
        .unwrap();
    assert_eq!(ids(&console.list().await.unwrap()), ["lin", "mei"]);
}

#[tokio::test]
async fn a_daemon_with_a_token_names_nobody_to_a_client_without_it() {
    let token = "made-up-for-this-test";
    let rig = Rig::guarded(vec![Spec::new("lin"), Spec::new("mei")], Some(token)).await;
    for wrong in [None, Some("a guess")] {
        let e = ghost_link::list(&rig.url, wrong.map(Into::into))
            .await
            .unwrap_err();
        assert_eq!(refused_with(&e), Some(code::UNAUTHORIZED), "{wrong:?}");
    }
    let roster = ghost_link::list(&rig.url, Some(token.into()))
        .await
        .unwrap();
    assert_eq!(ids(&roster), ["lin", "mei"]);

    // A refused question ends the connection: no hello can follow it.
    let mut ws = bare(&rig.url).await;
    let refused = exchange(&mut ws, &list_asking(None)).await.unwrap();
    assert_eq!(refused.error.map(|e| e.code), Some(code::UNAUTHORIZED));
    assert_eq!(exchange(&mut ws, &hello_for("lin")).await, None);
    // And a hello without the token is refused as it always was.
    let e = ghost_link::Console::connect(&rig.url, None, Some("lin".into()))
        .await
        .err()
        .unwrap();
    assert_eq!(refused_with(&e), Some(code::UNAUTHORIZED));
}

#[test]
fn her_status_says_whether_she_is_awake_and_what_her_day_has_her_doing() {
    let busy = serde_json::json!({"moment": {"phase": "awake", "doing": {"what": "rounds"}}});
    assert_eq!(phase(&busy), (Some(true), Some("rounds".to_owned())));
    let asleep = serde_json::json!({"moment": {"phase": "asleep", "doing": null}});
    assert_eq!(phase(&asleep), (Some(false), None));
    assert_eq!(phase(&serde_json::json!({})), (None, None));
}

#[test]
fn a_parent_directory_runs_every_ghost_in_it_but_the_template() {
    let root = tempfile::tempdir().unwrap();
    for name in ["mei", "lin", "_template", ".hidden"] {
        std::fs::create_dir(root.path().join(name)).unwrap();
        std::fs::write(root.path().join(name).join(PERSONA), "").unwrap();
    }
    std::fs::create_dir(root.path().join("notes")).unwrap();
    let found = find(&[root.path().to_path_buf()]).unwrap();
    let ids: Vec<&str> = found.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(ids, ["lin", "mei"]);
    // A ghost directory named itself is taken as it is, the template too.
    let one = find(&[root.path().join("_template")]).unwrap();
    assert_eq!(one[0].0, "_template");
    // The same character twice is refused, as is a directory with none.
    let twice = find(&[root.path().join("lin"), root.path().to_path_buf()]).unwrap_err();
    assert!(twice.to_string().contains("\"lin\""), "{twice}");
    assert!(find(&[root.path().join("notes")]).is_err());
}
