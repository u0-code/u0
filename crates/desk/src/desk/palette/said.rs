//! What was said, found by the palette: each server the window holds looks
//! through the records it keeps (`Call::Search`) once typing stops for a
//! moment; what a search for older words finds is dropped. A row is the
//! line it was found in, under its thread, and opens the thread there.

use std::{ops::Range, time::Duration, time::SystemTime};

use gpui::{Context, Task, Window};

use super::{Act, Desk, Lead, Row, lists, said_ago};
use crate::hub::Host;

/// How long typing stops before the servers are asked.
const QUIET: Duration = Duration::from_millis(250);
/// The fewest letters looked for: fewer find nearly every thread.
const LEAST: usize = 3;
/// The rows it shows at most.
const MOST: usize = 10;

/// What was said that answers what is typed, as the servers found it.
#[derive(Default)]
pub struct Heard {
    /// What is looked for, in lower case: what the answers kept answer.
    query: String,
    /// Each session found, by the server it is on: its key, the item of
    /// its thread where it was said, and that line.
    said: Vec<(Host, u64, usize, String)>,
    /// The search waiting for typing to stop; dropped, and so never made,
    /// when typing goes on.
    waiting: Option<Task<()>>,
    /// A thread opened at a message before its record was read: the
    /// message is brought into view once it is.
    reveal: Option<(u64, usize)>,
}

impl Heard {
    /// Look for `typed` from now on: what was found for other words is
    /// dropped, and a search not yet made is not. The words to search
    /// for, where they are new and enough.
    pub(super) fn ask(&mut self, typed: &str) -> Option<String> {
        let query = typed.trim().to_lowercase();
        if query == self.query {
            return None;
        }
        self.query.clone_from(&query);
        self.said.clear();
        self.waiting = None;
        (query.chars().count() >= LEAST).then_some(query)
    }

    /// What `host` found of `query`: kept in place of what it said before,
    /// only while `query` is what is looked for.
    fn found(&mut self, host: Host, query: &str, said: Vec<(u64, usize, String)>) {
        if query != self.query {
            return;
        }
        self.said.retain(|s| s.0 != host);
        let found = said
            .into_iter()
            .map(|(key, at, line)| (host, key, at, line));
        self.said.extend(found);
    }
}

/// Where "What was said" goes among the groups found: after the last
/// holding a strong match (every word typed at a word's start), before
/// the rest, which only guessed.
pub(super) fn place(strong: &[bool]) -> usize {
    strong.iter().rposition(|s| *s).map_or(0, |at| at + 1)
}

/// Where in `line`, in characters, `query` (in lower case) is first.
fn marked(line: &str, query: &str) -> Vec<Range<usize>> {
    let lower = |c: char| c.to_lowercase().next().unwrap_or(c);
    let line: Vec<char> = line.chars().map(lower).collect();
    let query: Vec<char> = query.chars().collect();
    (!query.is_empty())
        .then(|| {
            line.windows(query.len())
                .position(|w| w == query.as_slice())
        })
        .flatten()
        .map(|at| at..at + query.len())
        .into_iter()
        .collect()
}

impl Desk {
    /// What is typed in the palette changed: what was said is looked for
    /// a moment after typing stops, on every server the window holds.
    pub(super) fn look_for_said(&mut self, typed: &str, cx: &mut Context<Self>) {
        let Some(query) = self.palette.heard.ask(typed) else {
            return;
        };
        let asks: Vec<(Host, Vec<_>)> = (self.machines.iter())
            .map(|m| {
                let there =
                    (self.sessions.all.iter()).filter(|s| s.host == m.host && self.said_in(s));
                let records = there
                    .filter_map(|s| Some((s.key, s.meta.agent.clone(), s.meta.id.clone()?)))
                    .collect();
                (m.host, records)
            })
            .collect();
        self.palette.heard.waiting = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(QUIET).await;
            let _ = this.update(cx, |this, _| {
                for (host, records) in asks {
                    let query = query.clone();
                    this.call(host, crate::proto::Call::Search { query, records });
                }
            });
        }));
    }

    /// What `host` found of `query` in what was said.
    pub(crate) fn said_found(&mut self, host: Host, query: &str, said: Vec<(u64, usize, String)>) {
        self.palette.heard.found(host, query, said);
    }

    /// Whether what was said in `s` is looked through: a thread the
    /// palette lists.
    fn said_in(&self, s: &crate::session::Session) -> bool {
        s.parent.is_none()
            && s.beside.is_none()
            && !s.is_draft()
            && !s.meta.flags.archived
            && !self.deleting(s.key)
    }

    /// The rows of what was said, the latest used first; none of the
    /// threads `listed` already.
    pub(super) fn said_rows(&self, listed: &[u64], now: SystemTime) -> Vec<Row> {
        let heard = &self.palette.heard;
        let mut found: Vec<_> = (heard.said.iter())
            .filter(|(_, key, ..)| !listed.contains(key))
            .filter_map(|(_, key, at, line)| Some((self.sessions.at(*key)?, *at, line)))
            .filter(|(s, ..)| self.said_in(s))
            .collect();
        found.sort_by_key(|(s, ..)| std::cmp::Reverse(lists::used(s)));
        (found.into_iter().take(MOST))
            .map(|(s, at, line)| {
                let project = self.project_on(&super::super::Place::project_of(s));
                let under = [s.meta.title.clone(), project, self.name_of(&s.meta.agent)];
                Row {
                    lead: Lead::Thread(crate::sidebar::list::Glyph::of(s), s.key),
                    bold: marked(line, &heard.query),
                    end: Some((said_ago(lists::used(s), now), crate::theme::faint())),
                    ..Row::new("", line.clone(), Act::Said(s.key, at)).sub(under.join(" · "))
                }
            })
            .collect()
    }

    /// Open the thread `key` at its item `at`, once its record is read if
    /// it is not yet.
    pub(super) fn show_at(
        &mut self,
        key: u64,
        at: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.show(key, window, cx);
        if self.sessions.at(key).is_some_and(|s| s.items.len() > at) {
            self.reveal_item(at, cx);
        } else {
            self.palette.heard.reveal = Some((key, at));
        }
    }

    /// The record of `key` was read: the message it was opened at comes
    /// into view.
    pub(crate) fn read_to_reveal(&mut self, key: u64, cx: &mut Context<Self>) {
        if let Some((k, at)) = self.palette.heard.reveal
            && k == key
        {
            self.palette.heard.reveal = None;
            if self.current == Some(key) {
                self.reveal_item(at, cx);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Heard, marked, place};
    use crate::hub::Host;

    #[test]
    fn what_a_search_for_older_words_finds_is_dropped() {
        let mut heard = Heard::default();
        assert_eq!(heard.ask("re"), None, "two letters are not looked for");
        assert_eq!(heard.ask(" Redis "), Some("redis".into()));
        heard.found(Host(1), "re", vec![(1, 0, "re".into())]);
        assert!(heard.said.is_empty(), "an answer to what was typed before");
        heard.found(Host(1), "redis", vec![(2, 4, "redis".into())]);
        heard.found(Host(2), "redis", vec![(3, 1, "Redis".into())]);
        assert_eq!(heard.said.len(), 2, "each server's answer is kept");
        assert_eq!(
            heard.ask("redis"),
            None,
            "the same words are not asked again"
        );
        assert_eq!(heard.said.len(), 2);
        assert_eq!(heard.ask("redis pool"), Some("redis pool".into()));
        assert!(heard.said.is_empty(), "new words drop what the old found");
    }

    #[test]
    fn a_server_answering_again_replaces_what_it_said() {
        let mut heard = Heard::default();
        heard.ask("redis");
        heard.found(
            Host(1),
            "redis",
            vec![(2, 4, "a".into()), (5, 0, "b".into())],
        );
        heard.found(Host(2), "redis", vec![(3, 1, "c".into())]);
        heard.found(Host(1), "redis", vec![(2, 6, "d".into())]);
        let keys: Vec<(u64, usize)> = heard.said.iter().map(|s| (s.1, s.2)).collect();
        assert_eq!(keys, vec![(3, 1), (2, 6)]);
    }

    #[test]
    fn what_was_said_ranks_after_every_strong_match_and_before_guesses() {
        assert_eq!(place(&[]), 0);
        assert_eq!(place(&[false, false]), 0, "only guesses: it leads");
        assert_eq!(place(&[true, false, true, false]), 3);
        assert_eq!(place(&[true, true]), 2);
    }

    #[test]
    fn the_words_typed_are_marked_where_the_line_has_them() {
        assert_eq!(marked("Move sessions to Redis now", "redis"), vec![17..22]);
        assert_eq!(marked("…会话迁到 Redis", "redis"), vec![6..11]);
        assert!(marked("nothing here", "redis").is_empty());
    }
}
