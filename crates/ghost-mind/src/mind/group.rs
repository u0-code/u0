//! How she holds herself in a group when nobody is talking to her.
//!
//! She should neither pester people nor fall silent for good. Other
//! projects found that knobs held in memory (a willingness that decays, a
//! dice roll per message, a learned gate in front of the model) swing
//! between flooding a group and going mute in it. So nothing here is a
//! number she carries around. Everything is counted from her journal each
//! time it is needed, and there are three parts:
//!
//! - **When she looks.** Each time in a row she has looked at a group and
//!   said nothing, she takes twice as long to look again, up to eight times:
//!   she always comes back. A burst of messages, her name, or someone close
//!   to her writing brings her back at the usual pace.
//! - **What she sees.** Facts, not a verdict: how much of the talk here is
//!   hers, when she last spoke, whether the newest message is someone
//!   else's, and how her last unasked line went down. The rule for what to
//!   do with them is said once, in the words of the prompt files, and the
//!   decision is hers.
//! - **Three floors**, which can only make her quieter, for what a model
//!   does not reliably hold to on its own: she does not keep answering bots
//!   with no person in between; when she has agreed to be quiet in a group
//!   she does not look at its chatter until the time is up; and when she
//!   spoke up there unasked and nobody has taken it up, she leaves the
//!   chatter alone for a while rather than say more. A simulated group
//!   showed the last one is needed: told the rule and shown that her line
//!   had gone unanswered, she still answered the next message too.

use chrono::{DateTime, Duration, Utc};
use ghost_core::{
    journal::Event,
    memory::{Line, PlaceState},
    social::PlaceKind,
    when::when,
    will::{self, Stage},
};

use super::{Mind, decided::minutes_from};
use crate::{Result, actions::clip};

/// How many times in a row she can look and say nothing before her wait
/// stops growing: each doubles it, so three is eight times as long.
const DOUBLINGS: usize = 3;
/// This many unread messages are a burst: something is going on, and she
/// looks at the usual pace however quiet she has been.
const BURST: i64 = 6;
/// How many of the latest messages her share of the talk is counted over.
const SHARE_OF: usize = 50;
/// How far back she looks for the last thing she said unasked.
const UNPROMPTED_WINDOW: usize = 200;
/// How soon someone has to answer her for her line to count as answered.
const ANSWERED_WITHIN: Duration = Duration::minutes(10);
/// How many of her turns in a row, answered only by bots, she takes before
/// she stops answering bots there.
const BOT_TURNS: usize = 2;
/// How many of the latest messages are read to tell whether she is in a
/// loop with bots.
const BOT_WINDOW: usize = 60;
/// How long, at human pace, she leaves a group's chatter alone after
/// speaking up unasked when nobody has answered her or turned to her.
const UNANSWERED_PAUSE_MINUTES: f64 = 30.0;
/// The longest she can agree to be quiet for at once.
const QUIET_AT_MOST_HOURS: f64 = 7.0 * 24.0;

impl Mind {
    /// How much longer than usual she waits before looking at what was said
    /// in a group that none of it was said to her: twice as long for each
    /// time in a row she has looked there and said nothing, up to eight
    /// times. Not at all when there is reason to look anyway.
    pub(super) fn backoff(&self, place: &PlaceState, now: DateTime<Utc>) -> Result<f32> {
        if place.kind != PlaceKind::Group || self.worth_a_look(place, now)? {
            return Ok(1.0);
        }
        let silent = self.ghost.memory().silent_looks(&place.key, DOUBLINGS)?;
        Ok(2f32.powi(i32::try_from(silent).unwrap_or(0)))
    }

    /// Whether what is unread in a group calls for a look at the usual
    /// pace, however quiet she has been there: a burst of it, her name in
    /// it, or someone she is a friend of or close to having written.
    fn worth_a_look(&self, place: &PlaceState, now: DateTime<Utc>) -> Result<bool> {
        if place.unread >= BURST {
            return Ok(true);
        }
        if self.names_her(place)? {
            return Ok(true);
        }
        let memory = self.ghost.memory();
        for line in self.unread_lines(place)? {
            let close = match line.person.as_deref() {
                Some(key) => memory
                    .person(key)?
                    .is_some_and(|p| will::standing(&p, now).stage >= Stage::Friend),
                None => false,
            };
            if close {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Whether what is unread in a place calls her by any name she goes
    /// by there, with or without an @.
    fn names_her(&self, place: &PlaceState) -> Result<bool> {
        let names = self.names_on(&place.channel);
        Ok(self.unread_lines(place)?.iter().any(|line| {
            let text = line.text.to_lowercase();
            names.iter().any(|name| text.contains(name.as_str()))
        }))
    }

    /// The names she goes by on a platform, in lower case: her own, and
    /// her account's name and handle there.
    fn names_on(&self, channel: &str) -> Vec<String> {
        let mut names = vec![self.ghost.identity().name().to_owned()];
        if let Some(me) = self.channels.get(channel).and_then(|c| c.me.as_ref()) {
            names.push(me.name.clone());
            names.extend(me.handle.clone());
        }
        names
            .into_iter()
            .map(|n| n.trim().to_lowercase())
            .filter(|n| n.chars().count() >= 2)
            .collect()
    }

    /// The messages in a place she has not looked at yet, oldest first.
    fn unread_lines(&self, place: &PlaceState) -> Result<Vec<Line>> {
        Ok(self
            .ghost
            .memory()
            .thread_between(
                &place.key,
                place.read_upto,
                i64::MAX,
                usize::try_from(BURST).unwrap_or(usize::MAX),
            )?
            .into_iter()
            .filter(|l| l.incoming)
            .collect())
    }

    /// Whether what is said in a place, when none of it is said to her, is
    /// left unlooked at for now: she agreed to be quiet there, or the people
    /// who run her want her there only when she is spoken to.
    pub(super) fn holds_back(&self, place: &PlaceState, now: DateTime<Utc>) -> bool {
        place.kind != PlaceKind::Direct
            && (place.quiet_at(now)
                || self
                    .cfg
                    .places
                    .get(&place.key)
                    .is_some_and(|p| p.addressed_only))
    }

    /// Whether she and automated accounts have been answering each other in
    /// a group with no person in between: her last two turns there were
    /// each answered only by bots, and no person has written since.
    pub(super) fn in_a_loop_with_bots(&self, place: &PlaceState) -> Result<bool> {
        if place.kind != PlaceKind::Group {
            return Ok(false);
        }
        let memory = self.ghost.memory();
        let mut turns = 0;
        let mut answered = false;
        let mut in_her_turn = false;
        for line in memory.thread(&place.key, BOT_WINDOW)?.iter().rev() {
            if line.incoming {
                let automated = match &line.person {
                    Some(key) => memory.person(key)?.is_some_and(|p| p.automated),
                    None => false,
                };
                if !automated {
                    return Ok(false);
                }
                answered = true;
                in_her_turn = false;
                continue;
            }
            if !in_her_turn {
                if !answered {
                    return Ok(false);
                }
                turns += 1;
                if turns == BOT_TURNS {
                    return Ok(true);
                }
                answered = false;
            }
            in_her_turn = true;
        }
        Ok(false)
    }

    /// Until when what the others say in a group is left unlooked at
    /// because the last thing she said there was unasked and nobody has
    /// answered her or turned to her since. She said her piece; saying more
    /// before anyone takes it up is pressing it on them. `None` when that
    /// is not so, or the pause is over.
    pub(super) fn waiting_to_be_answered(
        &self,
        place: &PlaceState,
        now: DateTime<Utc>,
    ) -> Result<Option<DateTime<Utc>>> {
        if place.kind != PlaceKind::Group {
            return Ok(None);
        }
        let lines = self.ghost.memory().thread(&place.key, UNPROMPTED_WINDOW)?;
        let Some(said) = unanswered_unasked(&lines) else {
            return Ok(None);
        };
        // Her name in what came since is someone turning to her, @ or not.
        if self.names_her(place)? {
            return Ok(None);
        }
        let until = said + minutes_from(UNANSWERED_PAUSE_MINUTES * f64::from(self.tempo.scale()));
        Ok((until > now).then_some(until))
    }

    /// She agreed to be quiet in a group for `hours`.
    pub(super) fn go_quiet(
        &mut self,
        place: &PlaceState,
        hours: f64,
        now: DateTime<Utc>,
    ) -> Result<bool> {
        if place.kind != PlaceKind::Group || !hours.is_finite() || hours <= 0.0 {
            return Ok(false);
        }
        self.record(Event::Quieted {
            place: place.key.clone(),
            until: now + minutes_from(hours.min(QUIET_AT_MOST_HOURS) * 60.0),
        })?;
        Ok(true)
    }

    /// What she is told when she looks at a shared place and nothing new
    /// was said to her: how a sensible person behaves in a group, once,
    /// and then what is so here.
    pub(super) fn unaddressed(&self, place: &PlaceState, now: DateTime<Utc>) -> Result<String> {
        let facts: Vec<String> = self
            .facts_here(place, now)?
            .into_iter()
            .map(|fact| self.prompts.fill("fact_line", &[("fact", fact)]))
            .collect();
        Ok(self
            .prompts
            .fill("addressed_nobody", &[("facts", facts.join("\n"))]))
    }

    /// What is so in a shared place, one fact to a line, worked out from
    /// what was said there.
    fn facts_here(&self, place: &PlaceState, now: DateTime<Utc>) -> Result<Vec<String>> {
        let memory = self.ghost.memory();
        let clock = self.life.clock();
        let mut facts = Vec::new();

        let share = memory.share(&place.key, SHARE_OF)?;
        facts.push(self.prompts.fill(
            "fact_share",
            &[
                ("hers", share.hers.to_string()),
                ("of", share.of.to_string()),
            ],
        ));

        facts.push(match place.last_out_at {
            None => self.prompts.text("fact_never_said").to_owned(),
            Some(at) => match memory.lines_after(&place.key, place.last_out_seq)? {
                0 => self
                    .prompts
                    .fill("fact_newest_yours", &[("when", when(at, now, clock))]),
                since => self.prompts.fill(
                    "fact_last_said",
                    &[("when", when(at, now, clock)), ("since", since.to_string())],
                ),
            },
        });

        if let Some(aside) = memory.aside(&place.key)? {
            facts.push(match aside.to {
                Some(to) => self
                    .prompts
                    .fill("fact_aside_to", &[("from", aside.from), ("to", to)]),
                None => self.prompts.fill("fact_aside", &[("from", aside.from)]),
            });
        }

        if let Some(said) =
            memory.last_unprompted(&place.key, UNPROMPTED_WINDOW, ANSWERED_WITHIN)?
        {
            let landed = if said.answered {
                "fact_unprompted_answered"
            } else if now - said.at < ANSWERED_WITHIN {
                "fact_unprompted_waiting"
            } else {
                "fact_unprompted_unanswered"
            };
            facts.push(self.prompts.fill(
                landed,
                &[
                    ("when", when(said.at, now, clock)),
                    ("text", clip(&said.text, 120)),
                    ("minutes", ANSWERED_WITHIN.num_minutes().to_string()),
                ],
            ));
        }

        if let Some(note) = self
            .cfg
            .places
            .get(&place.key)
            .and_then(|p| p.note.as_deref())
            .filter(|n| !n.trim().is_empty())
        {
            facts.push(
                self.prompts
                    .fill("fact_note", &[("note", note.trim().to_owned())]),
            );
        }
        Ok(facts)
    }
}

/// When her latest turn in `lines` (oldest first) ended, if it was unasked,
/// meaning nothing said to her came in since her turn before it, and
/// nobody has said anything to her since.
fn unanswered_unasked(lines: &[Line]) -> Option<DateTime<Utc>> {
    let last = lines.iter().rposition(|l| !l.incoming)?;
    if lines[last + 1..].iter().any(|l| l.addressed) {
        return None;
    }
    let start = lines[..=last]
        .iter()
        .rposition(|l| l.incoming)
        .map_or(0, |i| i + 1);
    let before = lines[..start].iter().rposition(|l| !l.incoming);
    let asked = lines[before.map_or(0, |i| i + 1)..start]
        .iter()
        .any(|l| l.addressed);
    (!asked).then(|| lines[last].at)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(minute: i64, incoming: bool, addressed: bool) -> Line {
        Line {
            seq: minute,
            at: DateTime::<Utc>::UNIX_EPOCH + Duration::minutes(minute),
            incoming,
            person: None,
            name: String::new(),
            platform_id: None,
            reply_to: None,
            addressed,
            text: String::new(),
            directive: None,
        }
    }

    #[test]
    fn a_line_she_said_unasked_waits_until_someone_turns_to_her() {
        let at = |m| DateTime::<Utc>::UNIX_EPOCH + Duration::minutes(m);
        // Others talk, she chimes in, others go on without her.
        let ignored = [
            line(1, true, false),
            line(2, false, false),
            line(3, true, false),
        ];
        assert_eq!(unanswered_unasked(&ignored), Some(at(2)));
        // Someone answers her: nothing to wait for.
        let answered = [
            line(1, true, false),
            line(2, false, false),
            line(3, true, true),
        ];
        assert_eq!(unanswered_unasked(&answered), None);
        // She was asked: what she said was an answer, not a chime-in.
        let asked = [
            line(1, true, true),
            line(2, false, false),
            line(3, true, false),
        ];
        assert_eq!(unanswered_unasked(&asked), None);
        // Her turn of several lines ends at its last line.
        let turn = [
            line(1, true, false),
            line(2, false, false),
            line(4, false, false),
        ];
        assert_eq!(unanswered_unasked(&turn), Some(at(4)));
        // She never said anything here.
        assert_eq!(unanswered_unasked(&[line(1, true, false)]), None);
    }
}
