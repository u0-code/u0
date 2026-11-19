//! Telling the people who run her body what she has been doing.
//!
//! Once a day, at the time `[operators] report` names, she comes to mean to
//! tell each operator she has a private conversation with what she has
//! done and seen since she last told them. That is an ordinary intention,
//! carried out the way any other is: owed, so it is not chasing, and not
//! directed, so it waits for her to be awake and free. When she looks at
//! it, what she did since her last report is gathered from her journal and
//! put in front of her; what she makes of it is hers.
//!
//! Her last report to someone, and whether she has already meant to report
//! to them today, are read from the reports her memory folds out of her
//! journal, so a restart changes neither.
//!
//! What people told her in private is theirs. The conversations are shown
//! as who and how much, never what was said; and what she looked up, kept
//! or worked on because of someone else's private conversation is counted,
//! not shown.

use chrono::{DateTime, Duration, Utc};
use ghost_core::{
    journal::Event,
    memory::{Intention, PlaceState},
    social::{self, PlaceKind},
    when::when,
};
use tracing::info;

use super::Mind;
use crate::{Result, actions::clip};

/// How many of each kind of thing she did are put in front of her: the
/// newest. The rest are counted.
const SHOWN: usize = 8;
/// How many places she talked in.
const PLACES_SHOWN: usize = 8;
/// How much of any one thing.
const LINE: usize = 160;

/// One kind of thing she did, oldest first, and how many of it came of
/// someone else's private conversation and are left out.
#[derive(Default)]
struct Kind {
    lines: Vec<String>,
    private: usize,
}

/// The kinds gathered from her journal entry by entry, by their place in
/// the list they are gathered into.
const SEARCHED: usize = 0;
const KEPT: usize = 1;
const WORK: usize = 2;
const FEEDS: usize = 3;

impl Mind {
    /// When she reports on the calendar day `days` after today, in her own
    /// timezone: if she reports at all, and has operators to report to.
    pub(super) fn report_time(&self, now: DateTime<Utc>, days: i64) -> Option<DateTime<Utc>> {
        let operators = &self.cfg.operators;
        let at = operators.report?;
        if operators.accounts.values().all(Vec::is_empty) {
            return None;
        }
        let clock = self.life.clock();
        let today = now.with_timezone(&clock.tz()).date_naive();
        Some(clock.on(today + Duration::days(days), at))
    }

    /// Her report time has come, or came while her body was not running:
    /// for each operator she has a private conversation with and has not
    /// meant to report to today, she comes to mean to, due at that time.
    pub(super) fn report(&mut self, now: DateTime<Utc>) -> Result<()> {
        let Some(due) = self.report_time(now, 0) else {
            self.agenda.report = None;
            return Ok(());
        };
        if due > now {
            self.agenda.report = Some(due);
            return Ok(());
        }
        let tz = self.life.clock().tz();
        let today = due.with_timezone(&tz).date_naive();
        let operators: Vec<String> = self
            .cfg
            .operators
            .accounts
            .iter()
            .flat_map(|(channel, ids)| ids.iter().map(|id| social::key(channel, id)))
            .collect();
        for key in operators {
            let memory = self.ghost.memory();
            // A private conversation is the only way she has to reach
            // someone: she never writes first to a stranger.
            let direct = memory
                .person(&key)?
                .and_then(|p| p.direct.map(|d| (p.name, d)));
            let (name, place) = match direct {
                Some((name, place)) if memory.place(&place)?.is_some() => (name, place),
                _ => {
                    info!("no report to operator {key}: she has no private conversation with them");
                    continue;
                }
            };
            // One a day. And one still waiting, from a day she could not
            // get to them, already covers everything up to when she does.
            let last = memory.reports_to(&key, i64::MAX, 1)?;
            let waiting = memory
                .open_intentions()?
                .iter()
                .any(|i| i.report && i.about.as_deref() == Some(key.as_str()));
            if waiting
                || last
                    .first()
                    .is_some_and(|r| r.due.with_timezone(&tz).date_naive() == today)
            {
                continue;
            }
            let what = self.prompts.fill("report_what", &[("name", name)]);
            self.record(Event::Intended {
                due,
                what,
                about: Some(key),
                place: Some(place),
                owed: true,
                directed: false,
                report: true,
            })?;
        }
        self.agenda.report = self.report_time(now, 1);
        self.refresh_intentions()
    }

    /// What is happening, when the intention she is looking at is a report:
    /// what she has done and seen since her last report to them, and how
    /// to tell it.
    pub(super) fn report_brief(
        &self,
        intention: &Intention,
        here: &PlaceState,
        now: DateTime<Utc>,
    ) -> Result<String> {
        let memory = self.ghost.memory();
        let journal = self.ghost.journal();
        let clock = self.life.clock();
        let to = intention.about.as_deref().unwrap_or_default();
        // It starts where her last report to them did; before the first,
        // where her day did.
        let (after, since) = match memory.reports_to(to, intention.seq, 1)?.first() {
            Some(last) => (last.seq, "report_since_last"),
            None => (
                journal
                    .last_n_of(&["woke"], 2)?
                    .into_iter()
                    .rev()
                    .find(|e| e.seq < intention.seq)
                    .map_or(0, |e| e.seq),
                "report_since_first",
            ),
        };
        let since_at = journal.entry(after.max(1))?.map_or(intention.due, |e| e.at);

        // What she searched, kept, worked on and saw in public, each with
        // where it came from: a place, or a piece of work.
        let mut found: [Kind; 4] = Default::default();
        let kinds = [
            "looked_up",
            "opened",
            "kept",
            "concluded",
            "reported",
            "saw",
            "posted",
        ];
        for entry in journal.of_kinds_after(&kinds, after)? {
            let p = &self.prompts;
            let (of, origin, line) = match entry.event {
                Event::LookedUp { query, place, .. } => (
                    SEARCHED,
                    place,
                    p.fill("report_looked_up", &[("query", clip(&query, LINE))]),
                ),
                Event::Opened { url, place, .. } => (
                    SEARCHED,
                    place,
                    p.fill("report_opened", &[("url", clip(&url, LINE))]),
                ),
                // Only what she still holds, under the note she kept it with.
                Event::Kept { note, from, .. } if memory.belonging(entry.seq)?.is_some() => (
                    KEPT,
                    from,
                    p.fill(
                        "report_kept_line",
                        &[
                            ("thing", entry.seq.to_string()),
                            ("note", clip(&note, LINE)),
                        ],
                    ),
                ),
                Event::Concluded {
                    id,
                    outcome,
                    summary,
                } if outcome != "dropped" => {
                    let how = match outcome.as_str() {
                        "finished" => "how_finished",
                        "blocked" => "how_blocked",
                        _ => "how_gave_up",
                    };
                    let line = p.fill(
                        "report_concluded",
                        &[
                            ("how", p.text(how).to_owned()),
                            ("brief", self.brief_of(&id)),
                            ("summary", clip(&summary, LINE)),
                        ],
                    );
                    (WORK, Some(id), line)
                }
                Event::Reported { id, text, .. } => {
                    let line = p.fill(
                        "report_told",
                        &[("brief", self.brief_of(&id)), ("text", clip(&text, LINE))],
                    );
                    (WORK, Some(id), line)
                }
                Event::Saw { channel, text } => (
                    FEEDS,
                    None,
                    p.fill(
                        "report_saw",
                        &[("channel", channel), ("text", clip(&text, LINE))],
                    ),
                ),
                Event::Posted { channel, text, .. } => (
                    FEEDS,
                    None,
                    p.fill(
                        "report_posted",
                        &[("channel", channel), ("text", clip(&text, LINE))],
                    ),
                ),
                _ => continue,
            };
            let kind = &mut found[of];
            if self.private_elsewhere(origin.as_deref(), &here.key)? {
                kind.private += 1;
            } else {
                kind.lines.push(line);
            }
        }
        let [searched, kept, work, feeds] = found;

        let met = Kind {
            lines: memory
                .people_since(since_at)?
                .into_iter()
                .filter(|p| p.first_at >= since_at && p.key != to)
                .map(|p| {
                    self.prompts
                        .fill("report_met", &[("name", p.name), ("channel", p.channel)])
                })
                .rev()
                .collect(),
            private: 0,
        };
        let mut talked = Kind::default();
        for t in memory.talked_after(after, &here.key, PLACES_SHOWN)? {
            talked.lines.push(self.prompts.fill(
                "report_talked_line",
                &[
                    ("place", self.place_words(&t.place, &[], false)?),
                    ("heard", t.heard.to_string()),
                    ("said", t.said.to_string()),
                ],
            ));
        }

        let seen: String = [
            ("report_searched", searched),
            ("report_kept", kept),
            ("report_work", work),
            ("report_feeds", feeds),
            ("report_people", met),
            ("report_talked", talked),
        ]
        .into_iter()
        .map(|(key, kind)| self.section_of(key, kind))
        .collect();
        let name = memory.person(to)?.map_or_else(|| to.to_owned(), |p| p.name);
        Ok(ghost_core::narrate::fill(
            &self.prompts.report,
            &[
                ("what", intention.what.clone()),
                ("name", name),
                (
                    "since",
                    self.prompts
                        .fill(since, &[("when", when(since_at, now, clock))]),
                ),
                (
                    "seen",
                    if seen.trim().is_empty() {
                        self.prompts.text("report_nothing").to_owned()
                    } else {
                        seen
                    },
                ),
            ],
        ))
    }

    /// Whether something came of a private conversation with someone other
    /// than the one she is in: `origin` is the place it came from, or the
    /// piece of work it came of.
    fn private_elsewhere(&self, origin: Option<&str>, here: &str) -> Result<bool> {
        let Some(origin) = origin else {
            return Ok(false);
        };
        let memory = self.ghost.memory();
        let place = match memory.place(origin)? {
            Some(place) => Some(place),
            None => match memory.undertaking(origin)?.and_then(|u| u.place) {
                Some(key) => memory.place(&key)?,
                None => None,
            },
        };
        Ok(place.is_some_and(|p| p.kind == PlaceKind::Direct && p.key != here))
    }

    /// A piece of work by what it was, in a few words.
    fn brief_of(&self, id: &str) -> String {
        self.ghost
            .memory()
            .undertaking(id)
            .ok()
            .flatten()
            .map_or_else(|| id.to_owned(), |u| clip(&u.brief, 100))
    }

    /// One kind of thing she did, under its heading: the newest few, how
    /// many more there were, and how many are someone else's and left out.
    fn section_of(&self, key: &str, kind: Kind) -> String {
        let Kind { mut lines, private } = kind;
        let more = lines.len().saturating_sub(SHOWN);
        lines.drain(..more);
        if more > 0 {
            lines.push(
                self.prompts
                    .fill("report_more", &[("count", more.to_string())]),
            );
        }
        if private > 0 {
            lines.push(
                self.prompts
                    .fill("report_private", &[("count", private.to_string())]),
            );
        }
        self.prompts.section(key, "lines", &lines.join("\n"))
    }
}
