//! What happened in the sessions u0 runs (`happened`), as its tools
//! tell it: the `events` tool, filtered by agent, session or project, and
//! the resource `u0://events`, whose subscribers (MCP's
//! `subscriptions/listen`) are told each time something happens, so an
//! agent need not ask again and again.

use std::{path::PathBuf, time::Instant};

use rmcp::{
    ErrorData, handler::server::wrapper::Parameters, schemars, service::SubscriptionContext, tool,
    tool_router,
};
use serde::Deserialize;

use super::{MOST, Tools, said};
use crate::server::happened::Milestone;

/// The resource that stands for what happened: read, it is the latest of
/// it; subscribed to, each new thing says it changed.
pub(super) const EVENTS: &str = "u0://events";

#[derive(Deserialize, schemars::JsonSchema)]
struct Since {
    /// What happened after this number, as the last call said (`last`);
    /// none, all that is kept.
    #[serde(default)]
    after: u64,
    /// How long to wait, in seconds (an hour at most), for something to
    /// happen that the filters let through; none, not at all.
    #[serde(default)]
    seconds: u64,
    /// Only in the threads of this agent (its registry id).
    agent: Option<String>,
    /// Only in this thread (its id).
    session: Option<String>,
    /// Only in the threads of this project (its folder).
    project: Option<PathBuf>,
}

impl Tools {
    /// Whether `m` is of what `ask` asks for.
    fn wanted(&self, ask: &Since, m: &Milestone) -> bool {
        ask.agent.as_ref().is_none_or(|a| *a == m.agent)
            && ask.session.as_ref().is_none_or(|s| *s == *m.session.0)
            && ask.project.as_ref().is_none_or(|p| {
                (self.shared.meta(&m.agent, &m.session)).is_some_and(|meta| meta.project == *p)
            })
    }

    /// The latest of what happened, as `u0://events` reads.
    pub(super) fn latest(&self) -> rmcp::model::ReadResourceResult {
        let kept = self.shared.happened.subscribe();
        let latest: Vec<Milestone> = kept
            .borrow()
            .iter()
            .rev()
            .take(100)
            .rev()
            .cloned()
            .collect();
        let text = said(&serde_json::json!({"happened": latest})).unwrap_or_default();
        rmcp::model::ReadResourceResult::new(vec![rmcp::model::ResourceContents::text(
            text, EVENTS,
        )])
    }

    /// Tell the subscriber of `context` each time something happens, while
    /// it listens and its key lets it in.
    pub(super) async fn listen_events(
        &self,
        context: SubscriptionContext,
    ) -> Result<(), ErrorData> {
        let wants =
            (context.accepted().resource_subscriptions.iter().flatten()).any(|u| u == EVENTS);
        if !wants {
            context.cancelled().await;
            return Ok(());
        }
        let mut kept = self.heard.clone();
        loop {
            tokio::select! {
                () = context.cancelled() => return Ok(()),
                changed = kept.changed() => {
                    if changed.is_err() || self.caller().is_none() {
                        return Ok(());
                    }
                    if context.sink().notify_resource_updated(EVENTS).await.is_err() {
                        return Ok(());
                    }
                }
            }
        }
    }
}

#[tool_router(router = event_router, vis = "pub(super)")]
impl Tools {
    /// What happened in the threads u0 runs after a number (a turn
    /// ended, an agent asks the user), the latest last, each numbered;
    /// only that of an `agent`, a `session` or a `project` where one is
    /// named. Call again with `last` to follow on; with `seconds`, it
    /// waits that long for something the filters let through. `missed`
    /// says some of what happened since is no longer kept. Subscribed to,
    /// the resource `u0://events` says each time something happens.
    #[tool]
    async fn events(&self, Parameters(ask): Parameters<Since>) -> Result<String, String> {
        self.while_let_in(self.happened_since(ask)).await?
    }
}

impl Tools {
    /// What `events` gives, waited for as `ask` says.
    async fn happened_since(&self, ask: Since) -> Result<String, String> {
        let until = Instant::now() + std::time::Duration::from_secs(ask.seconds).min(MOST);
        let (mut after, mut missed) = (ask.after, false);
        loop {
            let left = until.saturating_duration_since(Instant::now());
            let (happened, lost) = self.shared.happened.after(after, left).await;
            missed |= lost;
            after = happened.last().map_or(after, |m| m.n);
            let found: Vec<_> = (happened.into_iter())
                .filter(|m| self.wanted(&ask, m))
                .collect();
            if !found.is_empty() || Instant::now() >= until {
                return said(
                    &serde_json::json!({"happened": found, "missed": missed, "last": after}),
                );
            }
        }
    }
}
