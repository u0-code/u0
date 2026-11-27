//! What of a thread is folded away, as Claude folds it: in a turn that is
//! over, the agent's work from its first call to its answer becomes one
//! line ("Read 3 files, ran 2 commands, and 4 more steps"); what it said
//! before that call and its answer (or the notes it ended with) stay out,
//! and so does each subagent it set to work, a card of its own under the
//! fold. A turn begins with what you said, or in a subagent with what it
//! was told. Inside a fold opened again, a run of three or more quiet tool calls
//! becomes one line of its own. A turn still going on is never folded: it
//! is what you are watching. A turn loaded from an agent has no end
//! marked; it is over when another follows it or nothing is going on.
//!
//! Text in, rows out: what is drawn is decided here, without a window.

use std::collections::HashSet;

use agent_client_protocol::v1::schema::{ToolCallContent, ToolKind};

use crate::session::{Item, Tool};

/// One row of the thread as drawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    /// The item at this index, as it is.
    Item(usize),
    /// A finished turn's work, `from..to`, folded or opened; how long the
    /// turn took, when that is known.
    Turn {
        from: usize,
        to: usize,
        secs: Option<u64>,
        open: bool,
    },
    /// A run of tool calls, `from..to`, folded or opened.
    Group { from: usize, to: usize, open: bool },
}

/// The key a turn's fold is opened under.
pub fn turn_key(session: u64, from: usize) -> String {
    format!("{session}-turn-{from}")
}

/// The key a group of calls is opened under.
pub fn group_key(session: u64, from: usize) -> String {
    format!("{session}-group-{from}")
}

/// How many quiet calls in a row are folded into one line.
const RUN: usize = 3;

/// The rows of a thread. `open_call(i)` says whether the call at `i` is
/// open (then it is not folded into a group); `expanded` holds the folds
/// opened; `going` whether a turn is going on now.
pub fn rows(
    session: u64,
    items: &[Item],
    expanded: &HashSet<String>,
    open_call: impl Fn(usize) -> bool,
    going: bool,
    fold_turns: bool,
) -> Vec<Row> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < items.len() {
        if !asks(&items[i]) {
            out.push(Row::Item(i));
            i += 1;
            continue;
        }
        out.push(Row::Item(i));
        // What went with what you said stays under it, out of the work.
        while matches!(
            items.get(i + 1),
            Some(Item::Image { yours: true, .. } | Item::File { .. })
        ) {
            i += 1;
            out.push(Row::Item(i));
        }
        // The turn runs to the next thing you said.
        let end = items[i + 1..]
            .iter()
            .position(asks)
            .map_or(items.len(), |p| i + 1 + p);
        let ended = items[i + 1..end]
            .iter()
            .rposition(|x| matches!(x, Item::Ended { .. }))
            .map(|p| i + 1 + p)
            .or((end < items.len() || !going).then_some(end));
        // The work runs from the first call to the last: a plan, a thought
        // or a note between them goes with it, but none of them opens or
        // extends it, so what the agent said before its first call and
        // after its last (its answer) stays out.
        let work = ended.and_then(|e| {
            let first = (i + 1..e).find(|&k| is_call(&items[k]))?;
            let last = (i + 1..e).rfind(|&k| is_call(&items[k]))?;
            out.extend((i + 1..first).map(Row::Item));
            Some((first, last + 1, e))
        });
        match work {
            Some((from, to, e)) if fold_turns => {
                let secs = match items.get(e) {
                    Some(Item::Ended { secs, .. }) => Some(*secs),
                    _ => None,
                };
                let open = expanded.contains(&turn_key(session, from));
                out.push(Row::Turn {
                    from,
                    to,
                    secs,
                    open,
                });
                if open {
                    grouped(session, items, from, to, expanded, &open_call, &mut out);
                } else {
                    let children = (from..to).filter(|&k| matches!(items[k], Item::Child { .. }));
                    out.extend(children.map(Row::Item));
                }
                out.extend((to..end).map(Row::Item));
            }
            Some((from, ..)) => {
                grouped(session, items, from, end, expanded, &open_call, &mut out);
            }
            // Going on, or no call made: every row shows.
            None => out.extend((i + 1..end).map(Row::Item)),
        }
        i = end;
    }
    out
}

/// Whether the item begins a turn: what you said, or what a subagent was
/// told.
fn asks(item: &Item) -> bool {
    matches!(item, Item::You(_) | Item::Told(_))
}

/// Whether the item is work done: a call, a question asked. A subagent
/// set to work is not: it is never folded away.
fn is_call(item: &Item) -> bool {
    matches!(item, Item::Tool(_) | Item::Asked(_) | Item::Elicited(_))
}

/// The items `from..to`, with each run of quiet calls folded into a group.
fn grouped(
    session: u64,
    items: &[Item],
    from: usize,
    to: usize,
    expanded: &HashSet<String>,
    open_call: &impl Fn(usize) -> bool,
    out: &mut Vec<Row>,
) {
    let quiet = |j: usize| matches!(items[j], Item::Tool(_)) && !open_call(j);
    let mut j = from;
    while j < to {
        if quiet(j) {
            let run_end = (j..to).find(|&k| !quiet(k)).unwrap_or(to);
            if run_end - j >= RUN {
                let open = expanded.contains(&group_key(session, j));
                out.push(Row::Group {
                    from: j,
                    to: run_end,
                    open,
                });
                if open {
                    out.extend((j..run_end).map(Row::Item));
                }
                j = run_end;
                continue;
            }
        }
        out.push(Row::Item(j));
        j += 1;
    }
}

/// What the calls among `items` did, in words: "Read 3 files, edited 2
/// and ran 4 commands".
pub fn summary(items: &[Item]) -> String {
    let mut read = 0;
    let mut edited = 0;
    let mut created = 0;
    let mut deleted = 0;
    let mut ran = 0;
    let mut searched = 0;
    let mut fetched = 0;
    let mut other = 0;
    for item in items {
        let Item::Tool(t) = item else { continue };
        let Tool { call, .. } = &**t;
        match call.kind {
            ToolKind::Read => read += 1,
            ToolKind::Edit
                if call
                    .content
                    .iter()
                    .any(|c| matches!(c, ToolCallContent::Diff(d) if d.old_text.is_none())) =>
            {
                created += 1;
            }
            ToolKind::Edit | ToolKind::Move => edited += 1,
            ToolKind::Delete => deleted += 1,
            ToolKind::Execute => ran += 1,
            ToolKind::Search => searched += 1,
            ToolKind::Fetch => fetched += 1,
            _ => other += 1,
        }
    }
    let count = |n: usize, one: &str, many: &str| match n {
        0 => None,
        1 => Some((n, crate::tr(one).to_string())),
        n => Some((n, crate::tr_with(many, &[("n", &n)]).to_string())),
    };
    let parts: Vec<(usize, String)> = [
        count(read, "read a file", "read %{n} files"),
        count(edited, "edited a file", "edited %{n} files"),
        count(created, "created a file", "created %{n} files"),
        count(deleted, "deleted a file", "deleted %{n} files"),
        count(ran, "ran a command", "ran %{n} commands"),
        count(searched, "searched once", "searched %{n} times"),
        count(fetched, "fetched a page", "fetched %{n} pages"),
        count(other, "used a tool", "used %{n} tools"),
    ]
    .into_iter()
    .flatten()
    .collect();
    if parts.is_empty() {
        return String::new();
    }
    // Two kinds named and the rest counted, past three, as Claude says it.
    let named = if parts.len() > 3 { 2 } else { parts.len() };
    let mut words: Vec<String> = parts[..named].iter().map(|(_, p)| p.clone()).collect();
    let rest: usize = parts[named..].iter().map(|(n, _)| n).sum();
    match rest {
        0 => {}
        1 => words.push(crate::tr("and 1 more step").to_string()),
        n => words.push(crate::tr_with("and %{n} more steps", &[("n", &n)]).to_string()),
    }
    let mut text = words.join(", ");
    if let Some(first) = text.get_mut(..1) {
        first.make_ascii_uppercase();
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::Session;

    fn session() -> Session {
        let mut s = Session::new(7, "x", "/r".into());
        s.said("go");
        for (id, kind) in [
            ("a", "read"),
            ("b", "read"),
            ("c", "search"),
            ("d", "execute"),
        ] {
            s.apply(
                serde_json::from_str(&format!(
                    r#"{{"sessionUpdate":"tool_call","toolCallId":"{id}","title":"t","kind":"{kind}","status":"completed"}}"#
                ))
                .unwrap(),
            );
        }
        s.apply(
            serde_json::from_str(
                r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Done."}}"#,
            )
            .unwrap(),
        );
        s
    }

    #[test]
    fn what_went_with_your_message_stays_under_it_when_the_turn_folds() {
        let mut s = Session::new(7, "x", "/r".into());
        s.attached.push(
            serde_json::from_str(
                r#"{"type":"resource_link","name":"a.pdf","uri":"file:///a.pdf"}"#,
            )
            .unwrap(),
        );
        s.said("read it");
        s.apply(
            serde_json::from_str(
                r#"{"sessionUpdate":"tool_call","toolCallId":"a","title":"t","kind":"read","status":"completed"}"#,
            )
            .unwrap(),
        );
        s.ended(
            Ok(agent_client_protocol::v1::schema::StopReason::EndTurn),
            Vec::new(),
        );
        let rows = rows(7, &s.items, &HashSet::new(), |_| false, false, true);
        assert!(matches!(s.items[1], Item::File { .. }));
        assert_eq!(rows[..2], [Row::Item(0), Row::Item(1)]);
    }

    #[test]
    fn a_subagent_set_to_work_shows_under_its_turns_fold() {
        let mut s = Session::new(7, "x", "/r".into());
        s.said("go");
        for update in [
            r#"{"sessionUpdate":"tool_call","toolCallId":"a","title":"t","kind":"read","status":"completed"}"#,
            r#"{"sessionUpdate":"subagent_update","sessionId":"c1","title":"Review","state":{"state":"running"}}"#,
            r#"{"sessionUpdate":"tool_call","toolCallId":"b","title":"t","kind":"read","status":"completed"}"#,
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Done."}}"#,
        ] {
            s.apply(serde_json::from_str(update).unwrap());
        }
        let rows = rows(7, &s.items, &HashSet::new(), |_| false, false, true);
        assert_eq!(
            rows,
            vec![
                Row::Item(0),
                Row::Turn {
                    from: 1,
                    to: 4,
                    secs: None,
                    open: false
                },
                Row::Item(2),
                Row::Item(4)
            ]
        );
    }

    #[test]
    fn a_subagents_task_begins_a_turn_that_folds_as_yours_does() {
        let mut s = Session::new(7, "x", "/r".into());
        s.meta.id = Some(agent_client_protocol::v1::schema::SessionId::new("c1"));
        for update in [
            r#"{"sessionUpdate":"session_message","messageId":"m","senderSessionId":"p","recipientSessionId":"c1","content":[{"type":"text","text":"Look."}]}"#,
            r#"{"sessionUpdate":"tool_call","toolCallId":"a","title":"t","kind":"read","status":"completed"}"#,
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Found."}}"#,
        ] {
            s.apply(serde_json::from_str(update).unwrap());
        }
        let rows = rows(7, &s.items, &HashSet::new(), |_| false, false, true);
        assert_eq!(
            rows,
            vec![
                Row::Item(0),
                Row::Turn {
                    from: 1,
                    to: 2,
                    secs: None,
                    open: false
                },
                Row::Item(2)
            ]
        );
    }

    #[test]
    fn a_turn_going_on_shows_every_row() {
        let s = session();
        let rows = rows(7, &s.items, &HashSet::new(), |_| false, true, true);
        assert_eq!(rows.len(), s.items.len());
        assert!(rows.iter().all(|r| matches!(r, Row::Item(_))));
    }

    #[test]
    fn a_finished_turn_folds_its_work_and_keeps_the_answer() {
        let mut s = session();
        s.ended(
            Ok(agent_client_protocol::v1::schema::StopReason::EndTurn),
            Vec::new(),
        );
        let rows = rows(7, &s.items, &HashSet::new(), |_| false, false, true);
        // You, the fold over four calls, the answer, the end.
        assert_eq!(
            rows,
            vec![
                Row::Item(0),
                Row::Turn {
                    from: 1,
                    to: 5,
                    secs: Some(0),
                    open: false
                },
                Row::Item(5),
                Row::Item(6)
            ]
        );
        assert_eq!(
            summary(&s.items[1..5]),
            "Read 2 files, ran a command, searched once"
        );
    }

    #[test]
    fn what_the_agent_said_before_its_first_call_stays_out_of_the_fold() {
        let mut s = Session::new(7, "x", "/r".into());
        s.said("go");
        s.apply(serde_json::from_str(r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Looking."}}"#).unwrap());
        s.apply(serde_json::from_str(r#"{"sessionUpdate":"tool_call","toolCallId":"a","title":"t","kind":"read","status":"completed"}"#).unwrap());
        s.apply(serde_json::from_str(r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Done."}}"#).unwrap());
        s.ended(
            Ok(agent_client_protocol::v1::schema::StopReason::EndTurn),
            Vec::new(),
        );
        let rows = rows(7, &s.items, &HashSet::new(), |_| false, false, true);
        assert_eq!(
            rows[..3],
            [
                Row::Item(0),
                Row::Item(1),
                Row::Turn {
                    from: 2,
                    to: 3,
                    secs: Some(0),
                    open: false
                }
            ]
        );
    }

    #[test]
    fn a_plan_or_a_thought_beside_the_calls_neither_opens_nor_extends_the_fold() {
        let plan = |status: &str| {
            serde_json::from_str(&format!(
                r#"{{"sessionUpdate":"plan","entries":[{{"content":"a","priority":"high","status":"{status}"}}]}}"#
            ))
            .unwrap()
        };
        let mut s = Session::new(7, "x", "/r".into());
        s.said("go");
        s.apply(
            serde_json::from_str(
                r#"{"sessionUpdate":"agent_thought_chunk","content":{"type":"text","text":"hm"}}"#,
            )
            .unwrap(),
        );
        s.apply(plan("in_progress"));
        s.apply(serde_json::from_str(r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Looking."}}"#).unwrap());
        s.apply(serde_json::from_str(r#"{"sessionUpdate":"tool_call","toolCallId":"a","title":"t","kind":"read","status":"completed"}"#).unwrap());
        s.apply(serde_json::from_str(r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Done."}}"#).unwrap());
        s.apply(plan("completed"));
        s.ended(
            Ok(agent_client_protocol::v1::schema::StopReason::EndTurn),
            Vec::new(),
        );
        let rows = rows(7, &s.items, &HashSet::new(), |_| false, false, true);
        // You, the thought, the plan, "Looking.", the fold over the one
        // call, the answer, the end.
        assert_eq!(
            rows,
            vec![
                Row::Item(0),
                Row::Item(1),
                Row::Item(2),
                Row::Item(3),
                Row::Turn {
                    from: 4,
                    to: 5,
                    secs: Some(0),
                    open: false
                },
                Row::Item(5),
                Row::Item(6)
            ]
        );
    }

    #[test]
    fn past_three_kinds_the_rest_are_counted_as_more_steps() {
        let mut s = Session::new(7, "x", "/r".into());
        for (id, kind) in [
            ("a", "read"),
            ("b", "edit"),
            ("c", "execute"),
            ("d", "fetch"),
            ("e", "fetch"),
        ] {
            s.apply(
                serde_json::from_str(&format!(
                    r#"{{"sessionUpdate":"tool_call","toolCallId":"{id}","title":"t","kind":"{kind}","status":"completed"}}"#
                ))
                .unwrap(),
            );
        }
        assert_eq!(
            summary(&s.items),
            "Read a file, edited a file, and 3 more steps"
        );
    }

    #[test]
    fn an_opened_turn_folds_a_run_of_quiet_calls_into_a_group() {
        let mut s = session();
        s.ended(
            Ok(agent_client_protocol::v1::schema::StopReason::EndTurn),
            Vec::new(),
        );
        let open: HashSet<String> = [turn_key(7, 1)].into_iter().collect();
        let rows = rows(7, &s.items, &open, |i| i == 4, false, true);
        // The fourth call is open, so only the first three are a group.
        assert_eq!(
            rows[1..4],
            [
                Row::Turn {
                    from: 1,
                    to: 5,
                    secs: Some(0),
                    open: true
                },
                Row::Group {
                    from: 1,
                    to: 4,
                    open: false
                },
                Row::Item(4)
            ]
        );
    }

    #[test]
    fn a_turn_loaded_without_an_end_folds_once_nothing_goes_on() {
        let s = session();
        let rows = rows(7, &s.items, &HashSet::new(), |_| false, false, true);
        assert_eq!(
            rows,
            vec![
                Row::Item(0),
                Row::Turn {
                    from: 1,
                    to: 5,
                    secs: None,
                    open: false
                },
                Row::Item(5)
            ]
        );
    }

    #[test]
    fn with_turns_unfolded_every_row_of_a_finished_turn_shows() {
        let mut s = Session::new(1, "x", std::env::temp_dir());
        s.said("go");
        s.apply(serde_json::from_str(r#"{"sessionUpdate":"tool_call","toolCallId":"a","title":"t","kind":"read","status":"completed"}"#).unwrap());
        s.apply(serde_json::from_str(r#"{"sessionUpdate":"tool_call","toolCallId":"b","title":"u","kind":"edit","status":"completed"}"#).unwrap());
        s.apply(serde_json::from_str(r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"done"}}"#).unwrap());
        s.ended(
            Ok(agent_client_protocol::v1::schema::StopReason::EndTurn),
            Vec::new(),
        );
        let folded = rows(7, &s.items, &HashSet::new(), |_| false, false, true);
        assert!(folded.iter().any(|r| matches!(r, Row::Turn { .. })));
        let shown = rows(7, &s.items, &HashSet::new(), |_| false, false, false);
        assert!(!shown.iter().any(|r| matches!(r, Row::Turn { .. })));
    }
}
