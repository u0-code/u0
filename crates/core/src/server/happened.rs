//! What happens in the sessions the server runs, as Ghost's tools tell
//! it: the turns going on, to be waited on, and the last of what happened
//! (a turn ended, an agent asks you), numbered, for an agent to follow
//! without reading each session again.

use std::{
    collections::{HashSet, VecDeque},
    time::Duration,
};

use agent_client_protocol::v1::schema::{SessionId, StopReason};
use serde::Serialize;
use tokio::sync::watch;

/// How many of what happened are kept.
const KEPT: usize = 1000;

/// Something that happened in a session, numbered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(super) struct Milestone {
    pub n: u64,
    /// When (seconds since 1970).
    pub at: u64,
    pub agent: String,
    pub session: SessionId,
    pub what: Happened,
    /// In a turn Ghost began on its own (a task's, a workflow's, one an
    /// agent said something in): no task runs on it, so none runs on its
    /// own runs. A turn of yours in the same session is yours.
    #[serde(skip)]
    pub by_ghost: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Happened {
    /// A turn ended, as said (`end_turn`, `cancelled`, or why it failed).
    TurnEnded(String),
    /// Its agent asks you, as titled, and waits.
    Asks(String),
}

/// How a turn ended, said: as ACP names its stop reason, or why it
/// failed.
pub(super) fn ended(how: Result<StopReason, impl std::fmt::Display>) -> String {
    match how {
        Ok(reason) => serde_json::to_value(reason)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default(),
        Err(why) => format!("failed: {why}"),
    }
}

#[derive(Default)]
pub(super) struct Happenings {
    /// The sessions a turn goes on in, by agent and id.
    turning: watch::Sender<HashSet<(String, SessionId)>>,
    /// The last of what happened, the latest last.
    kept: watch::Sender<VecDeque<Milestone>>,
    /// The sessions whose turn going on Ghost began on its own, by agent
    /// and id.
    ghost: std::sync::Mutex<HashSet<(String, SessionId)>>,
}

impl Happenings {
    /// A turn begins in the session `id` of `agent`: one Ghost began on
    /// its own (`by_ghost`), or yours.
    pub fn begin(&self, agent: &str, id: &SessionId, by_ghost: bool) {
        let on = (agent.to_owned(), id.clone());
        let mut ghost = self.ghost.lock().expect("ghost");
        if by_ghost {
            ghost.insert(on.clone());
        } else {
            ghost.remove(&on);
        }
        drop(ghost);
        self.turning.send_modify(|t| {
            t.insert(on);
        });
    }

    /// The turn in the session `id` of `agent` ended, `how`.
    pub fn end(&self, agent: &str, id: &SessionId, how: String) {
        self.turning.send_modify(|t| {
            t.remove(&(agent.to_owned(), id.clone()));
        });
        self.happened(agent, id, Happened::TurnEnded(how));
        (self.ghost.lock().expect("ghost")).remove(&(agent.to_owned(), id.clone()));
    }

    /// What tells each time something happens.
    pub fn subscribe(&self) -> watch::Receiver<VecDeque<Milestone>> {
        self.kept.subscribe()
    }

    /// `what` happened in the session `id` of `agent`.
    pub fn happened(&self, agent: &str, id: &SessionId, what: Happened) {
        let by_ghost =
            (self.ghost.lock().expect("ghost")).contains(&(agent.to_owned(), id.clone()));
        self.kept.send_modify(|kept| {
            let n = kept.back().map_or(1, |m| m.n + 1);
            kept.push_back(Milestone {
                n,
                at: crate::store::secs(std::time::SystemTime::now()),
                agent: agent.to_owned(),
                session: id.clone(),
                what,
                by_ghost,
            });
            while kept.len() > KEPT {
                kept.pop_front();
            }
        });
    }

    /// Whether a turn goes on in the session `id` of `agent`.
    pub fn busy(&self, agent: &str, id: &SessionId) -> bool {
        (self.turning.borrow()).contains(&(agent.to_owned(), id.clone()))
    }

    /// Wait, `most` at the longest, until no turn goes on in the session
    /// `id` of `agent`: whether none does.
    pub async fn idle(&self, agent: &str, id: &SessionId, most: Duration) -> bool {
        let on = (agent.to_owned(), id.clone());
        let mut turning = self.turning.subscribe();
        tokio::time::timeout(most, turning.wait_for(|t| !t.contains(&on)))
            .await
            .is_ok()
    }

    /// What happened after the number `after`, waiting `most` at the
    /// longest for something to; and whether some of it is no longer kept.
    pub async fn after(&self, after: u64, most: Duration) -> (Vec<Milestone>, bool) {
        let mut kept = self.kept.subscribe();
        let newer = |k: &VecDeque<Milestone>| k.back().is_some_and(|m| m.n > after);
        let _ = tokio::time::timeout(most, kept.wait_for(newer)).await;
        let kept = kept.borrow();
        let lost = kept.front().is_some_and(|m| m.n > after + 1);
        let found = kept.iter().filter(|m| m.n > after).cloned().collect();
        (found, lost)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block<F: std::future::Future>(f: F) -> F::Output {
        crate::acp::runtime().block_on(f)
    }

    #[test]
    fn a_turn_is_waited_on_until_it_ends_and_what_happened_is_told_after_a_number() {
        let h = std::sync::Arc::new(Happenings::default());
        let id = SessionId::new("s1");
        h.begin("a", &id, false);
        assert!(h.busy("a", &id));
        assert!(!block(h.idle("a", &id, Duration::from_millis(50))));
        let later = h.clone();
        let (end, waited) = (id.clone(), id.clone());
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            later.end("a", &end, "end_turn".into());
        });
        assert!(block(h.idle("a", &waited, Duration::from_secs(10))));
        let (said, lost) = block(h.after(0, Duration::ZERO));
        assert!(!lost);
        assert_eq!(said[0].what, Happened::TurnEnded("end_turn".into()));
        let (none, _) = block(h.after(said[0].n, Duration::from_millis(20)));
        assert!(none.is_empty());
        for _ in 0..KEPT + 5 {
            h.happened("a", &id, Happened::Asks("May I?".into()));
        }
        let (_, lost) = block(h.after(1, Duration::ZERO));
        assert!(lost, "more happened than is kept");
    }

    #[test]
    fn only_a_turn_ghost_began_is_marked_ghosts_and_a_later_one_of_yours_is_not() {
        let h = Happenings::default();
        let id = SessionId::new("s1");
        h.begin("a", &id, true);
        h.happened("a", &id, Happened::Asks("May I?".into()));
        h.end("a", &id, "end_turn".into());
        h.begin("a", &id, false);
        h.end("a", &id, "end_turn".into());
        let (said, _) = block(h.after(0, Duration::ZERO));
        let marked: Vec<bool> = said.iter().map(|m| m.by_ghost).collect();
        assert_eq!(marked, [true, true, false]);
    }
}
