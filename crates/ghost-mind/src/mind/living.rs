//! Her day: waking, planning it, living through it, looking back on it.
//!
//! None of this is triggered by anyone talking to her. The clock in
//! `ghost-core` says when her phase turns and when a planned block ends; this
//! module asks a brain what the day holds and how each part of it went, and
//! writes the answers into her journal.

use std::collections::BTreeMap;

use chrono::{DateTime, Duration, NaiveDate, NaiveTime, Utc};
use ghost_brain::{Brain, Completion};
use ghost_core::{
    journal::{Event, Sources},
    life::{Block, Day, Phase, Reach},
    memory::{Intention, Person},
    narrate, will,
};
use serde_json::Value;
use tracing::{info, warn};

use super::{Mind, Outcome, decided::minutes_from};
use crate::{Error, Result, actions};

/// The longest the life engine goes without checking on her day.
const HEARTBEAT: Duration = Duration::minutes(30);
/// How many times planning a day may fail before she goes without a plan.
const PLAN_RETRIES: u8 = 3;
/// The most platforms she looks back on in one night, besides her own day.
const PLATFORMS_AT_NIGHT: usize = 4;
/// The most people on one platform she looks back on in one night.
const PEOPLE_PER_PLATFORM: usize = 8;

pub(super) struct PlanDraft {
    blocks: Vec<Block>,
    canon: Vec<String>,
}

pub(super) struct LivedDraft {
    text: String,
    canon: Vec<String>,
}

/// What a day left her with about one person.
type Impression = (String, String, i8, Option<(String, String)>);

pub(super) struct ReflectDraft {
    reflection: String,
    /// `(who, impression, feel, a rewritten account of them)`. The account
    /// is `(what is known from shared places, what they said in private)`.
    people: Vec<Impression>,
    /// `(hours from now, what, about)`.
    intentions: Vec<(f64, String, Option<String>)>,
    canon: Vec<String>,
    /// `(lesson, the seq of the one it replaces)`.
    lessons: Vec<(String, Option<i64>)>,
    /// `(what she is after, the seq of the one it replaces)`.
    pursuits: Vec<(String, Option<i64>)>,
    /// Something to take on tomorrow for herself, or empty.
    undertake: String,
}

impl Mind {
    /// Bring her day up to `now`.
    pub(super) fn live(&mut self, now: DateTime<Utc>) -> Result<()> {
        if let Some(entry) = self.life.tick(&mut self.ghost, now)? {
            let slept = entry.event == Event::Slept;
            info!(
                "{} {}",
                self.ghost.identity().name(),
                if slept { "went to sleep" } else { "woke up" }
            );
            let _ = self.entries.send(entry);
            if slept {
                self.reflect(now)?;
            } else {
                self.plan_failures = 0;
                self.consider_review(now)?;
            }
        }
        let moment = self.life.moment(self.ghost.journal(), now)?;
        if self.cfg.live && moment.phase == Phase::Awake {
            match self.life.today(self.ghost.journal(), now)? {
                None => self.plan(now)?,
                Some(day) => self.live_through(&day, now)?,
            }
        }
        let next = moment
            .until
            .min(self.life.clock().next_turn(now))
            .min(now + HEARTBEAT)
            .max(now + Duration::seconds(1));
        self.agenda.life = Some(next);
        Ok(())
    }

    /// Ask a brain what the rest of today holds.
    fn plan(&mut self, now: DateTime<Utc>) -> Result<()> {
        if self.planning || self.plan_failures >= PLAN_RETRIES || !self.budget.allow(now) {
            return Ok(());
        }
        let clock = self.life.clock();
        let local = now.with_timezone(&clock.tz());
        let day = clock.day_of(now);
        // Everything she looked back on last night: her own day first.
        let journal = self.ghost.journal();
        let night = journal.last_of(&["slept"])?.map_or(0, |e| e.seq);
        let mut looked: Vec<(bool, String)> = journal
            .of_kinds_after(&["reflected"], night)?
            .into_iter()
            .filter_map(|e| match e.event {
                Event::Reflected {
                    text,
                    channel: None,
                    ..
                } => Some((false, text)),
                Event::Reflected {
                    text,
                    channel: Some(channel),
                    ..
                } => Some((
                    true,
                    self.prompts
                        .fill("looked_back_there", &[("channel", channel), ("text", text)]),
                )),
                _ => None,
            })
            .collect();
        looked.sort_by_key(|(elsewhere, _)| *elsewhere);
        let yesterday = looked
            .into_iter()
            .map(|(_, text)| text)
            .collect::<Vec<_>>()
            .join("\n");
        let template = self.prompts.plan.clone();
        // Her own day draws on all of her past, not only what someone
        // happened to mention.
        let seed = self.ghost.identity().seed();
        let past = narrate::past(&seed.past.iter().collect::<Vec<_>>());
        let pairs = vec![
            ("past", self.prompts.section("plan_past", "past", &past)),
            ("date", local.format("%A %Y-%m-%d").to_string()),
            ("time", local.format("%H:%M").to_string()),
            ("sleep", clock.sleep().format("%H:%M").to_string()),
            ("yesterday", self.prompts.or(yesterday, "nothing_yet")),
        ];
        let (system, right_now) = self.portrait(now)?;
        let (city, country) = {
            let home = &self.ghost.identity().seed().home;
            (home.city.clone(), home.country.clone())
        };
        let (brain, senses) = (self.tick.clone(), self.senses.clone());
        let words = self.prompts.clone();
        let (want_weather, max_tokens) = (self.cfg.weather, self.cfg.max_tokens);
        self.planning = true;
        self.budget.spend(now);
        self.jobs.spawn(async move {
            let weather = if want_weather {
                senses
                    .weather(&city, &country)
                    .await
                    .map(|forecast| forecast.words(&words))
            } else {
                None
            };
            let mut pairs = pairs;
            pairs.push((
                "weather",
                weather
                    .as_ref()
                    .map(|w| words.fill("weather", &[("weather", w.clone())]))
                    .unwrap_or_default(),
            ));
            let prompt = format!("{right_now}\n{}", narrate::fill(&template, &pairs));
            let request = Completion::ask(system, prompt, max_tokens).expecting_json();
            let result = ask(brain.as_ref(), &request)
                .await
                .and_then(|v| plan_draft(&v));
            Outcome::Planned {
                day,
                weather,
                result,
            }
        });
        Ok(())
    }

    pub(super) fn on_planned(
        &mut self,
        day: NaiveDate,
        weather: Option<String>,
        result: Result<PlanDraft>,
        now: DateTime<Utc>,
    ) -> Result<()> {
        self.planning = false;
        let draft = match result {
            Ok(draft) => draft,
            Err(e) => {
                self.plan_failures += 1;
                warn!(
                    "could not plan the day ({}/{PLAN_RETRIES}): {e}",
                    self.plan_failures
                );
                self.agenda.life = Some(now + Duration::minutes(5));
                return Ok(());
            }
        };
        // Keep only blocks that make sense on her clock and are not already over.
        let resolved = self.life.clock().resolve_blocks(day, &draft.blocks);
        let blocks: Vec<Block> = resolved
            .iter()
            .filter(|b| b.to > now)
            .filter_map(|b| draft.blocks.get(b.index as usize).cloned())
            .collect();
        if blocks.is_empty() {
            self.plan_failures += 1;
            warn!("the plan had no usable blocks");
            self.agenda.life = Some(now + Duration::minutes(5));
            return Ok(());
        }
        self.plan_failures = 0;
        self.record(Event::Planned {
            day: day.format("%Y-%m-%d").to_string(),
            weather,
            blocks,
        })?;
        self.register(draft.canon, "life")?;
        self.agenda.life = Some(now);
        Ok(())
    }

    /// Write up any block of today's plan that has ended while she was running.
    fn live_through(&mut self, day: &Day, now: DateTime<Utc>) -> Result<()> {
        if self.living.is_some() || !self.budget.allow(now) {
            return Ok(());
        }
        let lived: Vec<u32> = self
            .ghost
            .journal()
            .of_kinds_after(&["lived"], day.seq)?
            .into_iter()
            .filter_map(|e| match e.event {
                Event::Lived { block, .. } => block,
                _ => None,
            })
            .collect();
        let Some(block) = day
            .blocks
            .iter()
            .find(|b| b.to <= now && b.to >= self.started && !lived.contains(&b.index))
        else {
            return Ok(());
        };
        let tz = self.life.clock().tz();
        let prompt = narrate::fill(
            &self.prompts.lived,
            &[
                ("time", now.with_timezone(&tz).format("%H:%M").to_string()),
                (
                    "from",
                    block.from.with_timezone(&tz).format("%H:%M").to_string(),
                ),
                (
                    "to",
                    block.to.with_timezone(&tz).format("%H:%M").to_string(),
                ),
                ("what", block.what.clone()),
            ],
        );
        let request = self.framed(now, &prompt)?.expecting_json();
        let brain = self.tick.clone();
        let index = block.index;
        self.living = Some(index);
        self.budget.spend(now);
        self.jobs.spawn(async move {
            let result = ask(brain.as_ref(), &request)
                .await
                .and_then(|v| lived_draft(&v));
            Outcome::Lived {
                block: index,
                result,
            }
        });
        Ok(())
    }

    pub(super) fn on_lived(
        &mut self,
        block: u32,
        result: Result<LivedDraft>,
        now: DateTime<Utc>,
    ) -> Result<()> {
        self.living = None;
        match result {
            Ok(draft) => {
                self.record(Event::Lived {
                    text: draft.text,
                    block: Some(block),
                })?;
                self.register(draft.canon, "life")?;
            }
            Err(e) => {
                warn!("could not write up a part of the day: {e}");
                // Mark it passed so one bad block does not stall the rest.
                self.record(Event::Lived {
                    text: self.prompts.text("lived_unwritten").to_owned(),
                    block: Some(block),
                })?;
            }
        }
        self.agenda.life = Some(now);
        Ok(())
    }

    /// Look back on the day as she falls asleep: once on her own day, and
    /// once on each platform she talked with people on. Each is a memory of
    /// its own, kept with the platform it is about and the stretch of her
    /// journal it was made from, and each is asked about one part of the day
    /// only, so that what is said in one place is weighed with that place in
    /// front of her.
    pub(super) fn reflect(&mut self, now: DateTime<Utc>) -> Result<()> {
        if !self.cfg.live || self.reflecting > 0 {
            return Ok(());
        }
        let journal = self.ghost.journal();
        let (since_seq, since_at) = journal
            .last_of(&["woke"])?
            .map_or((0, now - Duration::hours(20)), |e| (e.seq, e.at));
        let sources = Sources {
            from: since_seq,
            upto: journal.last_seq()?,
            since: since_at,
            until: now,
        };
        let mut on: BTreeMap<String, Vec<Person>> = BTreeMap::new();
        for person in self.ghost.memory().people_since(since_at)? {
            let there = on.entry(person.channel.clone()).or_default();
            if there.len() < PEOPLE_PER_PLATFORM {
                there.push(person);
            }
        }
        let channels: Vec<String> = on.keys().cloned().collect();
        self.reflect_on_herself(&channels, since_seq, sources, now)?;
        for (channel, people) in on.into_iter().take(PLATFORMS_AT_NIGHT) {
            self.reflect_on(&channel, people, since_seq, sources, now)?;
        }
        Ok(())
    }

    /// What she means to do, for a night's look back, so she does not
    /// resolve to do it twice.
    fn meant(&self) -> Result<String> {
        let open = self.ghost.memory().open_intentions()?;
        Ok(self.prompts.or(self.meant_lines(&open), "nothing"))
    }

    /// What she means to do, a line each, with when by.
    pub(super) fn meant_lines(&self, intentions: &[Intention]) -> String {
        let tz = self.life.clock().tz();
        intentions
            .iter()
            .map(|i| {
                self.prompts.fill(
                    "meant_line",
                    &[
                        (
                            "due",
                            i.due.with_timezone(&tz).format("%m-%d %H:%M").to_string(),
                        ),
                        ("what", i.what.clone()),
                    ],
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Her own day: what she did, what she is after, her work and her craft.
    fn reflect_on_herself(
        &mut self,
        channels: &[String],
        since_seq: i64,
        sources: Sources,
        now: DateTime<Utc>,
    ) -> Result<()> {
        let tz = self.life.clock().tz();
        let lived = self
            .ghost
            .journal()
            .of_kinds_after(&["lived", "posted"], since_seq)?;
        if lived.is_empty() && channels.is_empty() {
            return Ok(());
        }
        let today = lived
            .iter()
            .filter_map(|e| {
                let text = match &e.event {
                    Event::Lived { text, .. } => text.clone(),
                    Event::Posted { text, .. } => crate::narrate::posted(&self.prompts, text),
                    _ => return None,
                };
                Some(format!(
                    "- {} {text}",
                    e.at.with_timezone(&tz).format("%H:%M")
                ))
            })
            .collect::<Vec<_>>();
        let talked = self
            .prompts
            .section("talked", "channels", &channels.join(", "));
        let prompt = narrate::fill(
            &self.prompts.reflect,
            &[
                ("time", now.with_timezone(&tz).format("%H:%M").to_string()),
                (
                    "today",
                    self.prompts.or(today.join("\n"), "nothing_written"),
                ),
                ("intentions", self.meant()?),
                ("talked", talked),
                ("public", self.public_review(now)?),
                ("pursuits", self.pursuit_block()?),
                ("own_work", self.own_work_line(now)?),
            ],
        );
        self.look_back(None, Vec::new(), sources, &prompt, now)
    }

    /// One platform's part of her day: the people she talked with there and
    /// what she told them.
    fn reflect_on(
        &mut self,
        channel: &str,
        people: Vec<Person>,
        since_seq: i64,
        sources: Sources,
        now: DateTime<Utc>,
    ) -> Result<()> {
        let journal = self.ghost.journal();
        let memory = self.ghost.memory();
        let clock = self.life.clock();
        let tz = clock.tz();
        // Her own words, so claims she made about her life can become canon.
        let said: Vec<String> = journal
            .of_kinds_after(&["said"], since_seq)?
            .into_iter()
            .rev()
            .filter_map(|e| match e.event {
                Event::Said {
                    channel: there,
                    text,
                    ..
                } if there == channel => Some(format!("- {}", actions::clip(&text, 200))),
                _ => None,
            })
            .take(40)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        let mut cards = Vec::with_capacity(people.len());
        for person in &people {
            let standing = will::standing(person, now);
            // Alone with her thoughts, she sees all of what she knows.
            let profile = memory.profile(&person.key)?;
            let since = profile.as_ref().map_or(0, |p| p.seq);
            let mut card = crate::narrate::person_card(
                &self.prompts,
                person,
                &standing,
                &memory.notes_after(&person.key, since, 12)?,
                now,
                clock,
            );
            if let Some(profile) = profile {
                card.push_str(&self.prompts.fill(
                    "account_so_far",
                    &[
                        ("shared", profile.shared.trim().to_owned()),
                        (
                            "private",
                            self.prompts.section(
                                "account_private",
                                "private",
                                profile.private.trim(),
                            ),
                        ),
                    ],
                ));
            }
            if let Some(direct) = &person.direct {
                let lines: Vec<_> = memory
                    .thread(direct, 12)?
                    .into_iter()
                    .filter(|l| l.at >= sources.since)
                    .collect();
                if !lines.is_empty() {
                    card.push_str(
                        &self.prompts.fill(
                            "today_with_them",
                            &[(
                                "transcript",
                                crate::narrate::transcript(
                                    &self.prompts,
                                    &lines,
                                    i64::MAX,
                                    clock,
                                    now,
                                )
                                .replace('\n', "\n    "),
                            )],
                        ),
                    );
                }
            }
            cards.push(card);
        }
        let prompt = narrate::fill(
            &self.prompts.reflect_on,
            &[
                ("time", now.with_timezone(&tz).format("%H:%M").to_string()),
                ("channel", channel.to_owned()),
                ("people", cards.join("\n")),
                ("said", self.prompts.or(said.join("\n"), "nothing")),
                ("intentions", self.meant()?),
            ],
        );
        self.look_back(Some(channel.to_owned()), people, sources, &prompt, now)
    }

    fn look_back(
        &mut self,
        channel: Option<String>,
        people: Vec<Person>,
        sources: Sources,
        prompt: &str,
        now: DateTime<Utc>,
    ) -> Result<()> {
        let request = self.framed(now, prompt)?.expecting_json();
        let brain = self.think.clone();
        self.reflecting += 1;
        self.budget.spend(now);
        self.jobs.spawn(async move {
            let result = ask(brain.as_ref(), &request)
                .await
                .and_then(|v| reflect_draft(&v));
            Outcome::Reflected {
                channel,
                people,
                sources,
                result,
            }
        });
        Ok(())
    }

    /// A look back came back: keep it, with the platform it was about and
    /// the stretch of her journal it was made from.
    pub(super) fn on_reflected(
        &mut self,
        channel: Option<&str>,
        people: &[Person],
        sources: Option<Sources>,
        result: Result<ReflectDraft>,
        now: DateTime<Utc>,
    ) -> Result<()> {
        self.reflecting = self.reflecting.saturating_sub(1);
        let draft = match result {
            Ok(draft) => draft,
            Err(e) => {
                warn!(
                    "could not look back on {}: {e}",
                    channel.unwrap_or("the day")
                );
                return Ok(());
            }
        };
        let find = |name: &str| {
            let name = name.trim().trim_start_matches('@').to_lowercase();
            people.iter().find(|p| {
                p.name.to_lowercase() == name
                    || p.handle
                        .as_deref()
                        .is_some_and(|h| h.to_lowercase() == name)
            })
        };
        self.record(Event::Reflected {
            text: draft.reflection,
            channel: channel.map(str::to_owned),
            sources,
        })?;
        for (who, impression, feel, account) in draft.people {
            let Some(person) = find(&who) else { continue };
            if let Some((shared, private)) = account {
                self.record(Event::Profiled {
                    person: person.key.clone(),
                    shared,
                    private,
                    sources,
                })?;
            }
            if feel != 0 {
                self.record(Event::Felt {
                    toward: person.key.clone(),
                    delta: feel,
                    why: impression.clone(),
                })?;
            }
            if !impression.is_empty() {
                self.record(Event::Noted {
                    about: person.key.clone(),
                    text: impression,
                    place: None,
                    sources,
                })?;
            }
        }
        for (hours, what, about) in draft.intentions.into_iter().take(2) {
            let person = about.as_deref().and_then(find);
            self.intend(
                now + minutes_from(hours.clamp(1.0, 24.0 * 14.0) * 60.0),
                what,
                person.map(|p| p.key.clone()),
                person.and_then(|p| p.direct.clone()),
                false,
            )?;
        }
        self.register(draft.canon, "reflection")?;
        // Her craft and what she is after belong to her own day.
        if channel.is_none() {
            self.learn(draft.lessons)?;
            self.on_pursuits(draft.pursuits)?;
            self.start_own_work(&draft.undertake, now)?;
        }
        self.refresh_intentions()
    }

    /// Register new facts about her life.
    fn register(&mut self, facts: Vec<String>, source: &str) -> Result<()> {
        for fact in facts.into_iter().take(3) {
            self.record(Event::Canon {
                fact,
                source: source.to_owned(),
            })?;
        }
        Ok(())
    }
}

async fn ask(brain: &dyn Brain, request: &Completion) -> Result<Value> {
    let reply = brain.complete(request).await?;
    actions::object(&reply.text).ok_or_else(|| {
        Error::Parse(format!(
            "no JSON object in: {}",
            actions::clip(&reply.text, 200)
        ))
    })
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|v| v.as_str().map(str::trim))
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn text(value: &Value) -> String {
    value.as_str().unwrap_or_default().trim().to_owned()
}

fn plan_draft(v: &Value) -> Result<PlanDraft> {
    let blocks: Vec<Block> = v["blocks"]
        .as_array()
        .ok_or_else(|| Error::Parse("the plan has no blocks".into()))?
        .iter()
        .filter_map(|b| {
            let what = text(&b["what"]);
            let reach = match text(&b["reach"]).to_lowercase().as_str() {
                "free" => Reach::Free,
                "busy" => Reach::Busy,
                "away" => Reach::Away,
                _ => Reach::Glance,
            };
            (!what.is_empty()).then_some(Block {
                from: hhmm(&b["from"])?,
                to: hhmm(&b["to"])?,
                what,
                reach,
            })
        })
        .collect();
    Ok(PlanDraft {
        blocks,
        canon: strings(&v["canon"]),
    })
}

/// A time of day, normalized to `HH:MM`.
fn hhmm(value: &Value) -> Option<String> {
    NaiveTime::parse_from_str(value.as_str()?.trim(), "%H:%M")
        .ok()
        .map(|t| t.format("%H:%M").to_string())
}

fn lived_draft(v: &Value) -> Result<LivedDraft> {
    let text = text(&v["text"]);
    if text.is_empty() {
        return Err(Error::Parse("nothing was written".into()));
    }
    Ok(LivedDraft {
        text,
        canon: strings(&v["canon"]),
    })
}

pub(super) fn reflect_draft(v: &Value) -> Result<ReflectDraft> {
    let reflection = text(&v["reflection"]);
    if reflection.is_empty() {
        return Err(Error::Parse("no reflection".into()));
    }
    let list = |key: &str| v[key].as_array().cloned().unwrap_or_default();
    #[allow(clippy::cast_possible_truncation)]
    let people = list("people")
        .iter()
        .map(|p| {
            let shared = text(&p["account"]["shared"]);
            let private = text(&p["account"]["private"]);
            (
                text(&p["who"]),
                text(&p["impression"]),
                p["feel"].as_f64().unwrap_or(0.0).round().clamp(-2.0, 2.0) as i8,
                (!shared.is_empty() || !private.is_empty()).then_some((shared, private)),
            )
        })
        .filter(|(who, ..)| !who.is_empty())
        .collect();
    let intentions = list("intentions")
        .iter()
        .filter_map(|i| {
            let what = text(&i["what"]);
            let about = Some(text(&i["about"])).filter(|a| !a.is_empty());
            (!what.is_empty()).then_some((i["hours"].as_f64()?, what, about))
        })
        .collect();
    Ok(ReflectDraft {
        reflection,
        people,
        intentions,
        canon: strings(&v["canon"]),
        lessons: super::public::lessons(&v["lessons"]),
        pursuits: v["pursuits"]
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .map(|item| match item {
                        Value::String(s) => (s.trim().to_owned(), None),
                        other => (text(&other["what"]), other["replaces"].as_i64()),
                    })
                    .collect()
            })
            .unwrap_or_default(),
        undertake: text(&v["undertake"]),
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn plans_are_read_forgivingly() {
        let draft = plan_draft(&json!({
            "blocks": [
                {"from": "9:00", "to": "10:30", "what": "coffee and the news", "reach": "Free"},
                {"from": "11:00", "to": "noon", "what": "walk", "reach": "away"},
                {"from": "14:00", "to": "16:00", "what": "", "reach": "busy"},
                {"from": "20:00", "to": "21:00", "what": "dinner", "reach": "sometimes"}
            ],
            "canon": ["  ", "has started sketching the teahouse"]
        }))
        .unwrap();
        assert_eq!(draft.blocks.len(), 2);
        assert_eq!(draft.blocks[0].from, "09:00");
        assert_eq!(draft.blocks[0].reach, Reach::Free);
        assert_eq!(draft.blocks[1].reach, Reach::Glance);
        assert_eq!(draft.canon, vec!["has started sketching the teahouse"]);
        assert!(plan_draft(&json!({"plan": "none"})).is_err());
    }

    #[test]
    fn reflections_keep_what_is_usable() {
        let draft = reflect_draft(&json!({
            "reflection": "A slow day.",
            "people": [{"who": "Haoran", "impression": "asks real questions", "feel": 5}, {"who": ""}],
            "intentions": [{"hours": 20, "what": "ask about the interview", "about": "Haoran"}, {"what": "no hours"}]
        }))
        .unwrap();
        assert_eq!(
            draft.people,
            vec![("Haoran".into(), "asks real questions".into(), 2, None)]
        );
        assert_eq!(draft.intentions.len(), 1);
        assert!(reflect_draft(&json!({"reflection": ""})).is_err());
    }
}
