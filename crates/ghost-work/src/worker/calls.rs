//! Running what the model asked for, and taking what came back into the
//! transcript.
//!
//! Calls that only look may run side by side; anything that changes
//! something runs alone and in order. Each result is marked with where it
//! came from, and its pictures are kept on disk before they are shown.

use std::time::Instant;

use ghost_brain::{Call, Item};
use serde_json::Value;

use super::{Worker, preview};
use crate::{
    outside,
    prompts::Prompts,
    steps,
    tool::{self, Toolbox},
    undertaking::placeholder,
};

/// Tools that only look, and can be run side by side when the model asks
/// for several at once.
const SIDE_BY_SIDE: [&str; 3] = ["read", "search", "fetch"];

impl Worker {
    pub(super) async fn run_calls(
        &self,
        tools: &Toolbox,
        words: &Prompts,
        calls: &[Call],
        items: &mut Vec<Item>,
    ) -> (Vec<Value>, String) {
        let mut logged = Vec::new();
        let mut signature = String::new();
        // Several calls that only look are run side by side; anything
        // that changes something runs alone and in order.
        let together = calls.len() > 1
            && calls
                .iter()
                .all(|c| SIDE_BY_SIDE.contains(&c.name.as_str()));
        let mut outcomes: Vec<Option<(tool::Outcome, u128)>> = vec![None; calls.len()];
        if together {
            let mut running = tokio::task::JoinSet::new();
            for (at, call) in calls.iter().enumerate() {
                let (tools, call) = (tools.clone(), call.clone());
                running.spawn(tracing::Instrument::in_current_span(async move {
                    let started = Instant::now();
                    let outcome = tools.run(&call).await;
                    (at, outcome, started.elapsed().as_millis())
                }));
            }
            while let Some(done) = running.join_next().await {
                if let Ok((at, outcome, ms)) = done {
                    outcomes[at] = Some((outcome, ms));
                }
            }
        }
        for (at, call) in calls.iter().enumerate() {
            let (outcome, ms) = match outcomes[at].take() {
                Some(done) => done,
                None if together => (tool::Outcome::fail("the tool fell over"), 0),
                None => {
                    let started = Instant::now();
                    let outcome = tools.run(call).await;
                    (outcome, started.elapsed().as_millis())
                }
            };
            tracing::info!(
                "  {}({}) → {}{}",
                call.name,
                preview(&call.args.to_string(), 100),
                preview(&outcome.output, 100),
                if outcome.failed { " [failed]" } else { "" }
            );
            // What it returned, as it returned it: the mark of where it
            // came from is for the model, not for whoever reads the log.
            logged.push(steps::call(
                &call.name,
                &call.args,
                &outcome.output,
                outcome.failed,
                outcome.pictures.len(),
                ms,
            ));
            // Waiting on parts that are still at it comes back the same
            // way every time, and is not being stuck.
            if call.name != "wait" {
                signature.push_str(&format!(
                    "{}\u{1f}{}\u{1f}{}\u{1e}",
                    call.name, call.args, outcome.output
                ));
            }
            if !outcome.failed
                && matches!(call.name.as_str(), "write" | "edit")
                && let Some(path) = call.args["path"].as_str()
                && let Err(e) = self.undertaking.update(|state| {
                    state.touched.retain(|p| p != path);
                    state.touched.push(path.to_owned());
                    let extra = state.touched.len().saturating_sub(8);
                    state.touched.drain(..extra);
                })
            {
                tracing::warn!("{path} is not kept among the files touched: {e}");
            }
            // A call to a tool the worker does not carry was answered by
            // the toolbox, not by anything outside.
            let ran = tools.has(&call.name);
            items.push(self.take_in(words, call, outcome, ran));
        }
        (logged, signature)
    }

    /// A tool's outcome as a transcript item. What came from outside is
    /// marked with where it came from (see [`outside`]); what the body
    /// says about it, such as where its pictures are kept, follows the
    /// mark. Its pictures are kept on disk and named in its output; a
    /// brain that cannot see them is told how many there were, and the
    /// bytes go no further.
    fn take_in(&self, words: &Prompts, call: &Call, outcome: tool::Outcome, ran: bool) -> Item {
        let tool::Outcome {
            output,
            failed,
            mut pictures,
            origin,
        } = outcome;
        let mut output = if ran {
            outside::mark(words, call, origin.as_deref(), output)
        } else {
            output
        };
        if !pictures.is_empty() {
            for file in self.undertaking.keep(&pictures) {
                output.push('\n');
                output.push_str(&placeholder(&file));
            }
            if !self.brain.sees() {
                output.push('\n');
                output.push_str(&ghost_brain::not_shown(pictures.len()));
                pictures.clear();
            }
        }
        Item::Result {
            id: call.id.clone(),
            name: call.name.clone(),
            output,
            failed,
            pictures,
        }
    }
}
