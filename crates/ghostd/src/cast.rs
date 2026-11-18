//! The characters one daemon runs, and keeping each of them alive on her
//! own.
//!
//! A character is a ghost directory, known by the directory's name. Each
//! has her own mind, journal, memory and work, her own body (the presences
//! that carry her), and her own settings: nothing that changes is shared
//! between two of them. What they do share holds no state of theirs: this
//! program, the network rule, and the words in the deployment's directory
//! of prompts, which each mind reads for itself.
//!
//! A mind that panics or fails is started again from her directory after a
//! pause, which grows while it keeps failing. The others are not touched,
//! and her presences stay connected: they are told about to the new mind,
//! and a message that arrives in between is held for it.

use std::{
    future::Future,
    path::{Path, PathBuf},
    pin::Pin,
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{Context, bail};
use ghost_mind::{Incoming, MindHandle, Question};
use ghost_protocol::{GhostCard, Listed, PROTOCOL, Roster};
use serde_json::Value;
use tokio::sync::{Notify, watch};
use tracing::{Instrument as _, Span, error, info, warn};

use crate::server::Presences;

#[cfg(test)]
pub(crate) mod tests;

/// What makes a directory a ghost's.
const PERSONA: &str = "ghost.toml";
/// The directory beside the others that is a persona to copy, not one to run.
const TEMPLATE: &str = "_template";
/// How long a message for her is held while her mind is being started again.
const HOLD: Duration = Duration::from_secs(300);
/// How long her mind has to say where her day has her, for the roster.
const ASK_WITHIN: Duration = Duration::from_secs(2);

/// One character, as the server and her keeper see her.
pub struct Character {
    /// The name of her directory: how a client names her.
    pub id: String,
    pub dir: PathBuf,
    /// Who she is, as clients are told at hello.
    pub card: GhostCard,
    /// The presences carrying her now.
    pub body: Arc<Presences>,
    /// Her mind as it is running now. It is replaced when her mind is
    /// started again.
    mind: watch::Sender<MindHandle>,
    /// Rung to have her keeper stop her mind in order and make it again
    /// from what is on disk and in the daemon's configuration now.
    again: Notify,
}

impl Character {
    pub fn new(
        id: String,
        dir: PathBuf,
        card: GhostCard,
        body: Arc<Presences>,
        mind: MindHandle,
    ) -> Self {
        Self {
            id,
            dir,
            card,
            body,
            mind: watch::Sender::new(mind),
            again: Notify::new(),
        }
    }

    /// Have her mind stopped in order and made again, at once: how a
    /// change to the brains she runs on takes effect. Her presences stay
    /// connected, and a message that arrives in between waits for the new
    /// mind, as after a failure.
    pub fn start_again(&self) {
        self.again.notify_one();
    }

    /// Her mind as it is running now.
    pub fn mind(&self) -> MindHandle {
        self.mind.borrow().clone()
    }

    /// Her mind now, and each time it is started again.
    pub fn minds(&self) -> watch::Receiver<MindHandle> {
        self.mind.subscribe()
    }

    /// Tell her mind a message reached her on `channel`. If her mind is
    /// being started again, the message waits for the new one rather than
    /// being lost: a presence does not send it twice.
    pub async fn hear(&self, channel: &str, message: Incoming) {
        let mut minds = self.mind.subscribe();
        loop {
            let mind = minds.borrow_and_update().clone();
            if mind.heard(channel, message.clone()).await.is_ok() {
                return;
            }
            if !matches!(
                tokio::time::timeout(HOLD, minds.changed()).await,
                Ok(Ok(()))
            ) {
                warn!(
                    "a message on {channel} was lost: her mind was not running for {} minutes",
                    HOLD.as_secs() / 60
                );
                return;
            }
        }
    }
}

/// Every character a daemon runs.
pub struct Cast(pub Vec<Arc<Character>>);

impl Cast {
    /// Have every mind stopped in order and made again: see
    /// [`Character::start_again`].
    pub fn start_again(&self) {
        for who in &self.0 {
            who.start_again();
        }
    }

    /// The character a client says it is for. Naming nobody is enough when
    /// there is only one.
    pub fn pick(&self, named: Option<&str>) -> Result<&Arc<Character>, String> {
        let ids = || {
            self.0
                .iter()
                .map(|c| c.id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        };
        match (named, self.0.as_slice()) {
            (None, [only]) => Ok(only),
            (None, _) => Err(format!(
                "this daemon runs {}; say which of them this is for (--ghost or GHOST_NAME)",
                ids()
            )),
            (Some(name), _) => self
                .0
                .iter()
                .find(|c| c.id == name)
                .ok_or_else(|| format!("this daemon does not run {name:?}; it runs {}", ids())),
        }
    }

    /// Everyone the daemon runs, each as a hello naming her is told, with
    /// where her day has her now. Her mind is asked that, all of them at
    /// once; one that does not answer in time is listed without it, so a
    /// mind being started again cannot hold up the list.
    pub async fn roster(&self) -> Roster {
        let each = self.0.iter().map(|who| async move {
            let asked = tokio::time::timeout(ASK_WITHIN, who.mind().ask(Question::Status)).await;
            let (awake, doing) = match asked {
                Ok(Ok(status)) => phase(&status),
                _ => (None, None),
            };
            Listed {
                id: who.id.clone(),
                card: who.card.clone(),
                awake,
                doing,
                avatar: ghost_core::ghost::avatar(&who.dir).is_some(),
            }
        });
        Roster {
            protocol: PROTOCOL,
            ghosts: futures_util::future::join_all(each).await,
        }
    }
}

/// Whether her day has her awake, and what it has her doing, as her status
/// says.
fn phase(status: &Value) -> (Option<bool>, Option<String>) {
    let moment = &status["moment"];
    let awake = moment["phase"].as_str().map(|phase| phase == "awake");
    let doing = moment["doing"]["what"].as_str().map(str::to_owned);
    (awake, doing)
}

/// The ghost directories `paths` names, each with her id. A path is either
/// a ghost directory itself, or one whose subdirectories are, every one
/// with a `ghost.toml` but the template.
pub fn find(paths: &[PathBuf]) -> anyhow::Result<Vec<(String, PathBuf)>> {
    let mut dirs = Vec::new();
    for path in paths {
        if path.join(PERSONA).is_file() {
            dirs.push(path.clone());
            continue;
        }
        let mut inside: Vec<PathBuf> = std::fs::read_dir(path)
            .with_context(|| format!("read {}", path.display()))?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|dir| dir.join(PERSONA).is_file() && !is_set_aside(dir))
            .collect();
        if inside.is_empty() {
            bail!(
                "{} has no {PERSONA}, and none of the directories in it has one",
                path.display()
            );
        }
        inside.sort();
        dirs.extend(inside);
    }
    let mut found: Vec<(String, PathBuf)> = Vec::new();
    for dir in dirs {
        let id = id(&dir)?;
        if let Some((_, other)) = found.iter().find(|(known, _)| *known == id) {
            bail!(
                "{} and {} are both {id:?}: a character is known by the name of her directory, \
                 so no two can share one",
                other.display(),
                dir.display()
            );
        }
        found.push((id, dir));
    }
    Ok(found)
}

/// The template, or anything hidden.
fn is_set_aside(dir: &Path) -> bool {
    dir.file_name()
        .and_then(|n| n.to_str())
        .is_none_or(|name| name == TEMPLATE || name.starts_with('.'))
}

/// A character's id: the name of her directory.
fn id(dir: &Path) -> anyhow::Result<String> {
    let full = dir
        .canonicalize()
        .with_context(|| format!("find {}", dir.display()))?;
    full.file_name()
        .and_then(|n| n.to_str())
        .map(str::to_owned)
        .with_context(|| format!("{} has no name to know her by", full.display()))
}

/// A mind ready to live: what it does until it stops, and the handle on it.
pub struct Life {
    pub run: Pin<Box<dyn Future<Output = ghost_mind::Result<()>> + Send>>,
    pub handle: MindHandle,
}

/// How to make her mind again from her directory, as it is on disk.
pub type Maker = Box<dyn Fn() -> anyhow::Result<Life> + Send + Sync>;

/// How long to wait before starting a mind again: `first` after a failure,
/// doubling with each failure after it up to `most`. A mind that ran for
/// `steady` before it failed starts the count again.
#[derive(Debug, Clone, Copy)]
pub struct Pauses {
    pub first: Duration,
    pub most: Duration,
    pub steady: Duration,
}

impl Default for Pauses {
    fn default() -> Self {
        Self {
            first: Duration::from_secs(1),
            most: Duration::from_secs(120),
            steady: Duration::from_secs(600),
        }
    }
}

/// Keep `who` alive, starting with `life`, until `stop` turns true; then
/// stop her in order. Each time her mind panics or fails it is logged and
/// started again with `make`, after a pause. Everything logged in here and
/// in her mind is in the span this runs in, which names her.
pub async fn keep(
    who: Arc<Character>,
    life: Life,
    make: Maker,
    mut stop: watch::Receiver<bool>,
    pauses: Pauses,
) {
    let mut next = Some(life);
    let mut pause = pauses.first;
    loop {
        let life = match next.take().map_or_else(&make, Ok) {
            Ok(life) => life,
            Err(e) => {
                error!("her mind could not be started again: {e:#}; trying again in {pause:?}");
                if stopped_within(pause, &mut stop).await {
                    return;
                }
                pause = (pause * 2).min(pauses.most);
                continue;
            }
        };
        who.mind.send_replace(life.handle.clone());
        // Presences that stayed connected through a restart are hers still.
        who.body.announce(&life.handle).await;
        let began = Instant::now();
        let mut living = tokio::spawn(life.run.instrument(Span::current()));
        let ended = tokio::select! {
            ended = &mut living => ended,
            () = told(&mut stop) => {
                life.handle.stop().await;
                living.await
            }
            () = who.again.notified() => {
                life.handle.stop().await;
                let _ = living.await;
                if *stop.borrow() {
                    return;
                }
                info!("her mind is started again on the brains the daemon has now");
                pause = pauses.first;
                continue;
            }
        };
        if *stop.borrow() {
            return;
        }
        if began.elapsed() >= pauses.steady {
            pause = pauses.first;
        }
        match ended {
            Ok(Ok(())) => return,
            Ok(Err(e)) => error!("her mind failed: {e}; starting it again in {pause:?}"),
            Err(e) => error!("her mind fell over: {e}; starting it again in {pause:?}"),
        }
        if stopped_within(pause, &mut stop).await {
            return;
        }
        pause = (pause * 2).min(pauses.most);
    }
}

/// Wait `pause`, or less if told to stop. Says whether it was told.
async fn stopped_within(pause: Duration, stop: &mut watch::Receiver<bool>) -> bool {
    tokio::time::timeout(pause, told(stop)).await.is_ok()
}

/// Resolves once told to stop, or once nobody can tell anymore.
async fn told(stop: &mut watch::Receiver<bool>) {
    let _ = stop.wait_for(|stop| *stop).await;
}
