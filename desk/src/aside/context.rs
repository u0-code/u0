//! The Context section: what filled each brain's context window, the way a
//! coding agent's app shows its own. Two blocks, one per brain at work in
//! the place on screen:
//!
//! - **Role**, her brain when she last looked at the conversation: what it
//!   was given, by part, as her looks keep it (`look.steps`).
//! - **Worker**, the model stepping the work of this place (the open one,
//!   or else the newest): what its newest step was given, as `work.steps`
//!   says.
//!
//! Each block reads as a headline (how much of the window is used, and
//! how much that is against the window), a thin bar of its parts in the
//! order of the rows under it, and a row per part with its tokens and its
//! share of the whole window, ending with the room still free. The parts
//! are measured in characters where the request is put together; the
//! provider counts the tokens of the whole, which are shared out among them
//! by those sizes. A window nobody knows is left out, and the shares are
//! then of what was read.

use chrono::{DateTime, Utc};
use ghost_link::protocol::WorkContext;
use gpui::{AnyElement, Div, FontWeight, Hsla, SharedString, div, prelude::*, px, relative};

use super::ROW;
use crate::{
    desk::{Desk, Watched},
    fonts::Faces,
    model::Entry,
    theme,
    thread::{self},
    ui,
};

/// The work whose context the Worker block shows for the place `place`:
/// the newest of its work still going, or else the newest, with whether it
/// is still going.
pub(crate) fn context_work(journal: &[Entry], place: &str) -> Option<(String, bool)> {
    let tasks = thread::place_tasks(journal, place);
    let task = tasks
        .iter()
        .rev()
        .find(|t| t.open())
        .or_else(|| tasks.last())?;
    Some((task.id.clone(), task.open()))
}

/// A part of a context, as its row and its stretch of the bar show it.
#[derive(Debug, Clone, PartialEq)]
struct Part {
    name: String,
    /// What it is, for a name that does not say so itself.
    tip: Option<&'static str>,
    color: Hsla,
}

/// One block's numbers, worked out from what was kept.
#[derive(Debug, Clone, PartialEq)]
struct Filled {
    /// Tokens the brain read.
    used: u64,
    /// Its context window, where known.
    window: Option<u64>,
    /// Each part, in the order of the rows, with its share of `used`.
    parts: Vec<(Part, u64)>,
    /// Room the work never fills: above where it folds its transcript.
    buffer: u64,
}

impl Filled {
    /// The parts' sizes in characters, in order, shared out over the
    /// `used` tokens the provider counted. `folds_at` is where a work
    /// folds its transcript, inside `window`.
    fn new(used: u64, sized: Vec<(Part, u64)>, window: Option<u64>, folds_at: Option<u64>) -> Self {
        let total: u64 = sized.iter().map(|(_, n)| n).sum();
        let parts = sized
            .into_iter()
            .filter(|(_, n)| *n > 0)
            .map(|(part, n)| {
                let tokens = (u128::from(used) * u128::from(n) / u128::from(total.max(1))) as u64;
                (part, tokens)
            })
            .collect();
        let buffer = match (window, folds_at) {
            (Some(w), Some(f)) => w.saturating_sub(f.max(used)),
            _ => 0,
        };
        Self {
            used,
            window,
            parts,
            buffer,
        }
    }

    /// What a share is taken of: the window, or what was read when the
    /// window is not known.
    fn whole(&self) -> u64 {
        self.window.unwrap_or(self.used).max(1)
    }

    /// The room left, where the window is known.
    fn free(&self) -> Option<u64> {
        self.window
            .map(|w| w.saturating_sub(self.used).saturating_sub(self.buffer))
    }

    /// `n` as a share of the whole, as a person writes it: `10.3%`.
    fn percent(&self, n: u64) -> String {
        format!("{:.1}%", n as f64 * 100. / self.whole() as f64)
    }

    /// How full it is, in a word: `24% used`, or under one per cent, to
    /// a tenth.
    fn headline(&self) -> String {
        match self.window {
            Some(w) => {
                let share = self.used as f64 * 100. / w.max(1) as f64;
                if share > 0. && share < 1. {
                    format!("{share:.1}% used")
                } else {
                    format!("{share:.0}% used")
                }
            }
            None => format!("{} used", kilo(self.used)),
        }
    }
}

/// A part of her context when she looks at a conversation, by the name
/// her daemon keeps it under.
fn role_part(key: &str) -> Part {
    let (name, tip, color) = match key {
        "transcript" => ("Conversation", None, theme::blue()),
        "system" => (
            "Who she is",
            Some("Her persona and her day, as she is told them before each look."),
            theme::accent(),
        ),
        "people" => (
            "People here",
            Some("What she knows of everyone in this conversation."),
            theme::green(),
        ),
        "tools" => ("Tools", None, theme::coral()),
        "recalled" => (
            "Recalled",
            Some("What she asked her memory for during this look."),
            theme::purple(),
        ),
        "past" => (
            "Her past",
            Some("Pieces of her past that what was said here touched."),
            theme::amber2(),
        ),
        "frame" => (
            "The rest",
            Some("The page around it all: where she is, why she looks, and how to answer."),
            theme::muted(),
        ),
        other => return Part::other(other),
    };
    Part {
        name: name.to_owned(),
        tip,
        color,
    }
}

/// A part of what the worker was given at a step, by the name the work
/// logs it under.
fn worker_part(key: &str) -> Part {
    let (name, tip, color) = match key {
        "messages" => (
            "Messages",
            Some(
                "What the worker said and called at each step, and what it was told along the way.",
            ),
            theme::blue(),
        ),
        "results" => (
            "Tool results",
            Some("What its calls brought back."),
            theme::purple(),
        ),
        "tools" => ("Tools", None, theme::coral()),
        "system" => ("System prompt", None, theme::accent()),
        "task" => (
            "Task",
            Some(
                "What the work was asked to do; after a fold, its plan, its handoff and its notes too.",
            ),
            theme::green(),
        ),
        other => return Part::other(other),
    };
    Part {
        name: name.to_owned(),
        tip,
        color,
    }
}

impl Part {
    fn other(key: &str) -> Self {
        Self {
            name: key.to_owned(),
            tip: None,
            color: theme::ghost_line(),
        }
    }
}

/// The order the rows of each block go in: the largest part as a rule
/// first, the way a coding agent's panel lists messages first. A part a
/// newer daemon keeps that is not named here comes after them.
const ROLE_ORDER: [&str; 7] = [
    "transcript",
    "system",
    "people",
    "tools",
    "recalled",
    "past",
    "frame",
];
const WORKER_ORDER: [&str; 5] = ["messages", "results", "tools", "system", "task"];

/// `sizes` in `order`, then whatever else is there.
fn ordered(
    sizes: &std::collections::BTreeMap<String, u64>,
    order: &[&str],
    part: fn(&str) -> Part,
) -> Vec<(Part, u64)> {
    let mut out: Vec<(Part, u64)> = order
        .iter()
        .filter_map(|k| sizes.get(*k).map(|n| (part(k), *n)))
        .collect();
    out.extend(
        sizes
            .iter()
            .filter(|(k, _)| !order.contains(&k.as_str()))
            .map(|(k, n)| (part(k), *n)),
    );
    out
}

/// What filled her context at her last look at `place`, and when; `Err`
/// says why there is nothing to show.
fn role(
    watched: &Watched,
    place: &str,
    window: Option<u64>,
) -> Result<(Filled, String), &'static str> {
    let looks = watched
        .following
        .places
        .get(place)
        .ok_or("Nothing read here yet.")?;
    let look = looks
        .looks
        .values()
        .next_back()
        .ok_or("Nothing read here yet.")?;
    let usage = look
        .rounds
        .iter()
        .rev()
        .find_map(|r| r.usage)
        .ok_or("Its daemon does not say what its looks read.")?;
    let sizes = look
        .rounds
        .iter()
        .rev()
        .find_map(|r| r.context.as_ref())
        .map(|c| ordered(c, &ROLE_ORDER, role_part))
        .unwrap_or_default();
    let at = look
        .rounds
        .last()
        .and_then(|r| r.at.parse::<DateTime<Utc>>().ok());
    Ok((Filled::new(usage.input, sizes, window, None), ago(at)))
}

/// What the worker's newest step was given. A step that did not keep its
/// window is measured against `window`, the one her brain has: all of it
/// is an estimate, from what the provider counted and the sizes kept.
fn worker(context: &WorkContext, window: Option<u64>) -> Option<(Filled, String)> {
    let usage = context.usage?;
    let sizes = ordered(&context.parts, &WORKER_ORDER, worker_part);
    let at = context
        .at
        .as_deref()
        .and_then(|t| t.parse::<DateTime<Utc>>().ok());
    let when = format!("step {} · {}", context.n, ago(at));
    Some((
        Filled::new(
            usage.input,
            sizes,
            context.window.or(window),
            context.folds_at,
        ),
        when,
    ))
}

/// How long ago `at` was, as a person says it.
fn ago(at: Option<DateTime<Utc>>) -> String {
    at.map_or_else(String::new, |t| {
        format!(
            "{} ago",
            thread::span((Utc::now() - t).num_seconds().max(0))
        )
    })
}

/// A count of tokens as a person writes it: `820`, `12.3k`, `33k`, `1M`.
pub(super) fn kilo(n: u64) -> String {
    let (value, unit) = if n < 1000 {
        return n.to_string();
    } else if n < 1_000_000 {
        (n as f64 / 1000., "k")
    } else {
        (n as f64 / 1_000_000., "M")
    };
    let shown = format!("{value:.1}");
    format!("{}{unit}", shown.strip_suffix(".0").unwrap_or(&shown))
}

/// The colour of the room still free: the bar's own track.
fn free_color() -> Hsla {
    theme::panel2()
}

/// The colour of the room above where the work folds.
fn buffer_color() -> Hsla {
    theme::faint()
}

impl Desk {
    /// The Context section for the place `place` of `watched`: a block for
    /// her looks, and one for the work of this place if it has any.
    pub(super) fn context(faces: &Faces, watched: &Watched, place: &str) -> Div {
        let window = watched.status.as_ref().and_then(|s| s.think_window);
        let mut body = div().flex().flex_col();
        body = body.child(match role(watched, place, window) {
            Ok((filled, when)) => block(faces, "role", "Role", &when, &filled),
            Err(why) => quiet_block(faces, "Role", why),
        });
        if let Some((work, _)) = context_work(&watched.journal, place) {
            let shown = watched
                .following
                .work_context(&work)
                .and_then(|c| worker(c, window));
            body = body.child(match shown {
                Some((filled, when)) => block(faces, "worker", "Worker", &when, &filled),
                None => quiet_block(faces, "Worker", "No step taken yet."),
            });
        }
        body
    }
}

/// A block's head: its name, and on the right when what it shows was read.
fn head(faces: &Faces, title: &'static str, when: &str) -> Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .h(px(22.))
        .child(
            ui::text(faces, title)
                .text_size(px(12.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme::muted()),
        )
        .child(
            ui::text(faces, when.to_owned())
                .text_size(px(11.5))
                .text_color(theme::faint()),
        )
}

/// A block with nothing to show yet: its head, and one quiet line.
fn quiet_block(faces: &Faces, title: &'static str, why: &'static str) -> Div {
    div()
        .flex()
        .flex_col()
        .pb(px(6.))
        .child(head(faces, title, ""))
        .child(
            ui::text(faces, why)
                .text_size(px(13.))
                .text_color(theme::muted())
                .h(px(ROW))
                .flex()
                .items_center(),
        )
}

/// One block: its head, how full the window is, the bar, and a row for
/// each part and for the room still free.
fn block(faces: &Faces, key: &'static str, title: &'static str, when: &str, f: &Filled) -> Div {
    let numbers = match f.window {
        Some(w) => format!("{} / {}", kilo(f.used), kilo(w)),
        None => String::new(),
    };
    let headline = div()
        .flex()
        .items_center()
        .justify_between()
        .h(px(ROW))
        .child(
            ui::text(faces, f.headline())
                .text_size(px(13.))
                .text_color(theme::text()),
        )
        .child(
            ui::mono(faces, numbers)
                .text_size(px(11.5))
                .text_color(theme::muted()),
        );
    let whole = f.whole() as f32;
    let mut bar = div()
        .flex()
        .h(px(6.))
        .mt(px(4.))
        .mb(px(8.))
        .rounded(px(3.))
        .overflow_hidden()
        .bg(free_color());
    let mut rows = div().flex().flex_col();
    for (part, tokens) in &f.parts {
        bar = bar.child(
            div()
                .h_full()
                .w(relative(*tokens as f32 / whole))
                .bg(part.color),
        );
        rows = rows.child(part_row(faces, key, part, *tokens, f));
    }
    if f.buffer > 0 {
        bar = bar.child(
            div()
                .h_full()
                .w(relative(f.buffer as f32 / whole))
                .bg(buffer_color()),
        );
        let part = Part {
            name: "Fold buffer".to_owned(),
            tip: Some(
                "Room the work leaves empty: before its transcript reaches it, the work folds what it has done into a handoff and goes on from that.",
            ),
            color: buffer_color(),
        };
        rows = rows.child(part_row(faces, key, &part, f.buffer, f));
    }
    if let Some(free) = f.free() {
        let part = Part {
            name: "Free space".to_owned(),
            tip: None,
            color: free_color(),
        };
        rows = rows.child(part_row(faces, key, &part, free, f));
    }
    div()
        .flex()
        .flex_col()
        .pb(px(10.))
        .child(head(faces, title, when))
        .child(headline)
        .child(bar)
        .child(rows)
}

/// A part's row: its small square, its name (underlined in dots, with what
/// it is on hover, where the name does not say), its tokens, and its share.
fn part_row(faces: &Faces, key: &str, part: &Part, tokens: u64, f: &Filled) -> Div {
    let name = ui::text(faces, part.name.clone())
        .text_size(px(13.))
        .text_color(theme::text2());
    let name: AnyElement = match part.tip {
        Some(tip) => div()
            .id(SharedString::from(format!("context:{key}:{}", part.name)))
            .flex_none()
            .border_b_1()
            .border_dashed()
            .border_color(theme::faint())
            .child(name)
            .tooltip(ui::tip(tip))
            .into_any_element(),
        None => name.flex_none().into_any_element(),
    };
    div()
        .flex()
        .items_center()
        .h(px(ROW))
        .child(
            div()
                .size(px(8.))
                .rounded(px(2.))
                .bg(part.color)
                .flex_none()
                .mr(px(10.)),
        )
        .child(name)
        .child(div().flex_1())
        .child(
            ui::mono(faces, kilo(tokens))
                .text_size(px(11.5))
                .text_color(theme::muted())
                .flex_none()
                .w(px(52.))
                .text_right(),
        )
        .child(
            ui::mono(faces, f.percent(tokens))
                .text_size(px(11.5))
                .text_color(theme::text2())
                .flex_none()
                .w(px(48.))
                .text_right(),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sized(parts: &[(&str, u64)]) -> Vec<(Part, u64)> {
        parts.iter().map(|(k, n)| (worker_part(k), *n)).collect()
    }

    #[test]
    fn a_step_that_kept_no_window_is_measured_against_her_brains_window() {
        let step = WorkContext {
            n: 6,
            at: None,
            usage: serde_json::from_value(
                serde_json::json!({"input": 15_068, "cached": 14_464, "output": 370}),
            )
            .ok(),
            parts: std::collections::BTreeMap::new(),
            window: None,
            folds_at: None,
        };
        let (filled, when) = worker(&step, Some(1_000_000)).expect("what it read");
        assert_eq!((filled.used, filled.window), (15_068, Some(1_000_000)));
        assert!(filled.parts.is_empty());
        assert!(when.starts_with("step 6"));
        let kept = WorkContext {
            window: Some(128_000),
            ..step
        };
        assert_eq!(
            worker(&kept, Some(1_000_000)).unwrap().0.window,
            Some(128_000)
        );
    }

    #[test]
    fn tokens_read_as_a_person_writes_them() {
        assert_eq!(kilo(820), "820");
        assert_eq!(kilo(102_640), "102.6k");
        assert_eq!(kilo(33_000), "33k");
        assert_eq!(kilo(1_000_000), "1M");
        assert_eq!(kilo(1_048_576), "1M");
        assert_eq!(kilo(1_250_000), "1.2M");
    }

    #[test]
    fn what_was_read_is_shared_out_by_size_and_each_share_is_of_the_whole_window() {
        let f = Filled::new(
            10_000,
            sized(&[("messages", 3000), ("system", 1000), ("task", 0)]),
            Some(100_000),
            None,
        );
        let tokens: Vec<u64> = f.parts.iter().map(|(_, n)| *n).collect();
        assert_eq!(tokens, [7500, 2500], "an empty part has no row");
        assert_eq!(
            f.percent(7500),
            "7.5%",
            "of the window, not of what was read"
        );
        assert_eq!(f.free(), Some(90_000));
        assert_eq!(f.headline(), "10% used");
    }

    #[test]
    fn the_room_above_where_the_work_folds_is_neither_used_nor_free() {
        let f = Filled::new(
            200_000,
            sized(&[("results", 1)]),
            Some(1_000_000),
            Some(800_000),
        );
        assert_eq!(f.buffer, 200_000);
        assert_eq!(f.free(), Some(600_000));
        let past = Filled::new(
            900_000,
            sized(&[("results", 1)]),
            Some(1_000_000),
            Some(800_000),
        );
        assert_eq!(
            (past.buffer, past.free()),
            (100_000, Some(0)),
            "used past the fold"
        );
    }

    #[test]
    fn without_a_window_only_what_was_read_is_said_and_shares_are_of_it() {
        let f = Filled::new(
            4000,
            sized(&[("messages", 1), ("system", 3)]),
            None,
            Some(3000),
        );
        assert_eq!(f.headline(), "4k used");
        assert_eq!(f.free(), None);
        assert_eq!(f.buffer, 0);
        assert_eq!(f.percent(1000), "25.0%");
    }

    #[test]
    fn a_small_share_of_a_large_window_still_reads_as_more_than_nothing() {
        let f = Filled::new(4000, Vec::new(), Some(1_048_576), None);
        assert_eq!(f.headline(), "0.4% used");
    }

    #[test]
    fn parts_go_in_the_order_of_their_rows_and_unknown_ones_after() {
        let sizes = [("task", 1), ("zeta", 2), ("messages", 3), ("system", 4)]
            .into_iter()
            .map(|(k, n)| (k.to_owned(), n))
            .collect();
        let names: Vec<String> = ordered(&sizes, &WORKER_ORDER, worker_part)
            .into_iter()
            .map(|(p, _)| p.name)
            .collect();
        assert_eq!(names, ["Messages", "System prompt", "Task", "zeta"]);
    }
}
