//! What is going on now, and for how long: the last row of a look she is
//! in the middle of ("Thinking 4s") or of a piece of work still running
//! ("Working 12s"), an amber dot breathing beside it.
//!
//! The protocol does not say what she is calling while she calls it: a
//! look keeps a round only once all its calls have come back, and a piece
//! of work logs a step once its calls are done. So the row cannot name
//! the command still running; what it can say truly is how long it has
//! been since anything came back, which is how long this step is taking.

use chrono::{DateTime, Duration, Utc};
use ghost_link::protocol::{LoggedStep, Look};
use gpui::{Div, prelude::*, px};

use super::task_first::breathing;
use crate::{
    fonts::Faces,
    style::{self, S2, SMALL, meta},
    theme, ui,
};

/// The last row of something still going on: an amber dot breathing, a
/// word, and, when it is known, how long since it last moved on, counting
/// as the desk draws again.
pub(super) fn row(faces: &Faces, word: &'static str, since: Option<DateTime<Utc>>) -> Div {
    let ring = ui::dot(theme::accent()).flex_none().size(px(7.));
    style::row()
        .gap(px(S2))
        .child(breathing(ring))
        .child(
            ui::text(faces, word)
                .text_size(px(SMALL))
                .text_color(theme::muted()),
        )
        .when_some(since, |d, since| {
            d.child(meta(faces, super::span((Utc::now() - since).num_seconds())))
        })
}

/// The longest a round's calls are counted as taking, so that a time
/// logged wrong cannot push the clock past what it can hold.
const ROUND_MOST_MS: u64 = 24 * 60 * 60 * 1000;

/// When a look still going on last heard back: the end of its newest
/// round, which is when it was called and the time its calls took, one
/// after another; `from`, when it began, if it has no round yet.
pub fn look_since(look: &Look, from: DateTime<Utc>) -> DateTime<Utc> {
    let ended = look.rounds.last().and_then(|round| {
        let at: DateTime<Utc> = round.at.parse().ok()?;
        let ms: u64 = round.calls.iter().map(|c| c.ms).sum();
        let ms = i64::try_from(ms.min(ROUND_MOST_MS)).ok()?;
        Some(at + Duration::milliseconds(ms))
    });
    ended.map_or(from, |t| t.max(from))
}

/// When a piece of work still running last moved on: when its newest step
/// was logged, which is once that step's calls were done. None when no
/// step says when it was taken.
pub fn work_since(steps: &[LoggedStep]) -> Option<DateTime<Utc>> {
    steps
        .iter()
        .rev()
        .find_map(|s| s.at.as_deref()?.parse().ok())
}

#[cfg(test)]
mod tests {
    use ghost_link::protocol::{LookRound, ToolCall};
    use serde_json::json;

    use super::*;

    fn call(ms: u64) -> ToolCall {
        ToolCall {
            name: "shell".into(),
            args: json!({}),
            out: String::new(),
            cut: false,
            chars: 0,
            ms,
            failed: false,
            pictures: 0,
        }
    }

    fn at(s: &str) -> DateTime<Utc> {
        s.parse().unwrap()
    }

    #[test]
    fn a_look_has_been_thinking_since_its_last_round_came_back() {
        let from = at("2026-11-18T12:00:00Z");
        let mut look = Look {
            look: 1,
            seq: None,
            running: true,
            rounds: Vec::new(),
        };
        assert_eq!(
            look_since(&look, from),
            from,
            "no round yet: since it began"
        );
        look.rounds.push(LookRound {
            at: "2026-11-18T12:00:05Z".into(),
            calls: vec![call(2_000), call(1_500)],
            usage: None,
            context: None,
        });
        assert_eq!(
            look_since(&look, from),
            at("2026-11-18T12:00:08.500Z"),
            "its calls ran one after another"
        );
    }

    #[test]
    fn work_has_been_on_its_step_since_the_last_one_was_logged() {
        let step = |n: u64, at: Option<&str>| LoggedStep {
            n,
            at: at.map(str::to_owned),
            said: String::new(),
            ms: None,
            calls: Vec::new(),
        };
        assert_eq!(work_since(&[]), None);
        assert_eq!(
            work_since(&[
                step(1, Some("2026-11-18T12:00:00Z")),
                step(2, Some("2026-11-18T12:00:30Z")),
                step(3, None),
            ]),
            Some(at("2026-11-18T12:00:30Z"))
        );
    }
}
