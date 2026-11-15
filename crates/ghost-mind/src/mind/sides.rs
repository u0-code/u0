//! Her sides, and how she comes to a place.
//!
//! A persona may give her more than one side (`[[facet]]` in `ghost.toml`):
//! clingy and sharp-tongued, say, both really her. Which one shows is never
//! kept anywhere. There is no mood to store, no counter that builds up, no
//! timer that decays and no extra call to update any of it, because every
//! one of those drifts away from what actually happened and then has to be
//! corrected. Instead, each time she looks at a place, a few facts are
//! worked out afresh from her rhythm, her memory and the date, each with its
//! cause: she woke not long ago; this person got to her yesterday, and why;
//! they left her waiting since this morning; her day has leaned one way.
//! They are put late in what she reads, beside the rule for how a person
//! with sides is, and the brain does the rest the way a person does: from
//! causes she could name.
//!
//! What carries over from one day to the next is what she herself noted
//! with `feel`, which is in her journal, and where she stands with someone,
//! which follows from it.

use chrono::{DateTime, Duration, NaiveDate, Utc};
use ghost_core::{
    identity::Seal,
    journal::Event,
    life::Phase,
    memory::{Line, PlaceState},
    narrate::capital,
    social::PlaceKind,
    when::when,
    will,
};

use super::Mind;
use crate::{Result, actions::clip};

/// How long after she wakes she has only just woken.
const JUST_UP: Duration = Duration::minutes(90);
/// How long before she sleeps it is late for her.
const LATE: Duration = Duration::minutes(90);
/// How far back what she felt toward someone is still on her mind, and how
/// many of those feelings are said.
const FELT_WITHIN: Duration = Duration::days(7);
const FELT_SHOWN: usize = 2;
/// Within this, more than one time someone got to her is a pattern.
const AGAIN_WITHIN: Duration = Duration::hours(24);
/// How long her message waits before an answer to it is a late one.
const SLOW: Duration = Duration::hours(2);
/// How long a quiet stretch is before someone writing is coming back.
const AWHILE: Duration = Duration::hours(12);
/// How much of her unanswered message is quoted.
const QUOTED: usize = 60;

impl Mind {
    /// Her sides as her persona describes them, for under who she is.
    /// Empty when she has none, so a persona without them reads as before.
    pub(super) fn facets_words(&self) -> String {
        let facets: Vec<String> = self
            .ghost
            .identity()
            .seed()
            .facets
            .iter()
            .map(|f| {
                self.prompts.fill(
                    "facet",
                    &[
                        ("name", f.name.trim().to_owned()),
                        ("when", f.when.trim().to_owned()),
                        (
                            "examples",
                            self.prompts
                                .section("facet_examples", "examples", f.examples.trim()),
                        ),
                    ],
                )
            })
            .collect();
        self.prompts
            .section("facets", "facets", &facets.join("\n\n"))
    }

    /// How she comes to `place` at `now`: the facts that bear on which side
    /// of her shows, and the rule for sides, or nothing when there is
    /// nothing to say. `lines` is the stretch of the conversation she has in
    /// front of her.
    pub(super) fn right_now(
        &self,
        place: &PlaceState,
        lines: &[Line],
        now: DateTime<Utc>,
    ) -> Result<String> {
        let mut facts = self.the_hour(now);
        if let Some(facet) = self.todays_lean(now)? {
            facts.push(self.prompts.fill("lean", &[("facet", facet)]));
        }
        facts.extend(self.with_them(place, lines, now)?);
        if facts.is_empty() {
            return Ok(String::new());
        }
        let facts: Vec<String> = facts
            .into_iter()
            .map(|fact| self.prompts.fill("right_now_fact", &[("fact", fact)]))
            .collect();
        Ok(self
            .prompts
            .fill("right_now", &[("facts", facts.join("\n"))]))
    }

    /// The hour against her rhythm: just up, or nearly time for bed. While
    /// she is asleep, what she is told of her moment says so already.
    fn the_hour(&self, now: DateTime<Utc>) -> Vec<String> {
        let clock = self.life.clock();
        if clock.phase_at(now) == Phase::Asleep {
            return Vec::new();
        }
        let woke = clock.instant(clock.day_of(now), clock.wake());
        if woke <= now && now - woke < JUST_UP {
            return vec![self.prompts.text("now_woke").to_owned()];
        }
        if clock.next_sleep(now) - now < LATE {
            return vec![self.prompts.fill(
                "now_late",
                &[("sleep", clock.sleep().format("%H:%M").to_string())],
            )];
        }
        Vec::new()
    }

    /// The side her day leans toward, when she has sides: the same all
    /// through one waking day, and drawn afresh the next.
    fn todays_lean(&self, now: DateTime<Utc>) -> Result<Option<String>> {
        let facets = &self.ghost.identity().seed().facets;
        if facets.is_empty() {
            return Ok(None);
        }
        // The seal she was born with stays hers through every revision of
        // her persona, so a revision does not change the day's lean.
        let born = match self.ghost.journal().first()? {
            Some(entry) => match entry.event {
                Event::Born { seal, .. } => seal,
                _ => self.ghost.identity().seal().clone(),
            },
            None => self.ghost.identity().seal().clone(),
        };
        let day = self.life.clock().day_of(now);
        Ok(Some(
            facets[lean(&born, day, facets.len())]
                .name
                .trim()
                .to_owned(),
        ))
    }

    /// What bears on her mood about the person she is looking at this
    /// for: where she stands with them, what she last felt toward them, and
    /// in a private conversation how long they kept her waiting. Where she
    /// stands is said only with something else, since who is here already
    /// says it.
    fn with_them(
        &self,
        place: &PlaceState,
        lines: &[Line],
        now: DateTime<Utc>,
    ) -> Result<Vec<String>> {
        let memory = self.ghost.memory();
        let clock = self.life.clock();
        let first_new = lines.iter().find(|l| l.incoming && l.seq > place.read_upto);
        let alone_with_them = place.kind == PlaceKind::Direct;
        let person = if alone_with_them {
            memory.person_of_direct(&place.key)?
        } else {
            match lines
                .iter()
                .rev()
                .find(|l| l.incoming && l.seq > place.read_upto)
                .and_then(|l| l.person.as_deref())
            {
                Some(key) => memory.person(key)?,
                None => None,
            }
        };
        let Some(person) = person else {
            return Ok(Vec::new());
        };
        let name = || ("name", person.name.clone());
        let mut facts = Vec::new();

        let felt = memory.feelings_toward(&person.key, now - FELT_WITHIN, 12)?;
        for feeling in &felt[felt.len().saturating_sub(FELT_SHOWN)..] {
            // What moved her may have been said in private; elsewhere she
            // has the feeling and not its reason.
            let why = if alone_with_them {
                self.prompts
                    .section("felt_why", "why", &clip(feeling.why.trim(), 160))
            } else {
                String::new()
            };
            facts.push(self.prompts.fill(
                if feeling.delta > 0 {
                    "felt_closer"
                } else {
                    "felt_away"
                },
                &[
                    ("when", capital(&when(feeling.at, now, clock))),
                    name(),
                    ("why", why),
                ],
            ));
        }
        let stung = felt
            .iter()
            .filter(|f| f.delta < 0 && now - f.at < AGAIN_WITHIN)
            .count();
        if stung > 1 {
            facts.push(self.prompts.fill("felt_again", &[name()]));
        }

        if alone_with_them {
            let before = match first_new {
                Some(new) => memory.thread_before(&place.key, new.seq - 1, 1)?.pop(),
                None => lines.last().cloned(),
            };
            let quoted = |l: &Line| clip(&l.text.replace('\n', " "), QUOTED);
            match (first_new, before) {
                (Some(new), Some(b)) if !b.incoming && new.at - b.at >= SLOW => {
                    facts.push(self.prompts.fill(
                        "them_slow",
                        &[
                            name(),
                            ("when", when(b.at, now, clock)),
                            ("text", quoted(&b)),
                        ],
                    ));
                }
                (Some(new), Some(b)) if b.incoming && new.at - b.at >= AWHILE => {
                    facts.push(
                        self.prompts
                            .fill("them_back", &[name(), ("when", when(b.at, now, clock))]),
                    );
                }
                (None, Some(b)) if !b.incoming && now - b.at >= SLOW => {
                    facts.push(self.prompts.fill(
                        "them_silent",
                        &[
                            name(),
                            ("when", when(b.at, now, clock)),
                            ("text", quoted(&b)),
                        ],
                    ));
                }
                _ => {}
            }
        }

        if !facts.is_empty() {
            let stage = will::standing(&person, now).stage.word().to_owned();
            facts.insert(
                0,
                self.prompts.fill("them_stage", &[name(), ("stage", stage)]),
            );
        }
        Ok(facts)
    }
}

/// Which of `sides` sides her day leans toward on `day`: drawn from the
/// seal she was born with and the date, so it is hers, the same all day,
/// and unrelated from one day to the next.
fn lean(born: &Seal, day: NaiveDate, sides: usize) -> usize {
    let drawn = Seal::of(format!("{born} {day}").as_bytes());
    let n = u64::from_str_radix(&drawn.as_str()[..16], 16).unwrap_or(0);
    let sides = u64::try_from(sides.max(1)).unwrap_or(1);
    usize::try_from(n % sides).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn todays_lean_holds_all_day_and_is_drawn_afresh_across_days() {
        let born = Seal::of(b"a persona");
        let day = |d| NaiveDate::from_ymd_opt(2026, 11, d).unwrap();
        // The same day gives the same side, however often it is asked.
        assert_eq!(lean(&born, day(16), 2), lean(&born, day(16), 2));
        // Over a fortnight, both sides come up.
        let leans: std::collections::BTreeSet<usize> =
            (1..=14).map(|d| lean(&born, day(d), 2)).collect();
        assert_eq!(leans.len(), 2, "{leans:?}");
        // It is hers: someone born otherwise does not lean in step with her.
        let other = Seal::of(b"another persona");
        assert!((1..=14).any(|d| lean(&born, day(d), 2) != lean(&other, day(d), 2)));
        assert!((1..=14).all(|d| lean(&born, day(d), 3) < 3));
    }
}
