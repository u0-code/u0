//! Her day: waking, planning it, living through it, looking back on it.
//!
//! None of this is triggered by anyone talking to her. The clock in
//! `ghost-core` says when her phase turns and when a planned block ends; this
//! module asks a brain what the day holds and how each part of it went, and
//! writes the answers into her journal.

use chrono::{DateTime, Duration, NaiveDate, NaiveTime, Utc};
use ghost_brain::{Brain, Completion};
use ghost_core::{
    journal::Event,
    life::{Block, Day, Phase, Reach},
    memory::Person,
    narrate, will,
};
use serde_json::Value;
use tracing::{info, warn};

use super::{Mind, Outcome, attend::minutes_from};
use crate::{Error, Result, actions};

/// The longest the life engine goes without checking on her day.
const HEARTBEAT: Duration = Duration::minutes(30);
/// How many times planning a day may fail before she goes without a plan.
const PLAN_RETRIES: u8 = 3;

pub(super) struct PlanDraft {
    blocks: Vec<Block>,
    canon: Vec<String>,
}

pub(super) struct LivedDraft {
    text: String,
    canon: Vec<String>,
}

pub(super) struct ReflectDraft {
    reflection: String,
    /// `(who, impression, feel)`.
    people: Vec<(String, String, i8)>,
    /// `(hours from now, what, about)`.
    intentions: Vec<(f64, String, Option<String>)>,
    canon: Vec<String>,
    /// `(lesson, the seq of the one it replaces)`.
    lessons: Vec<(String, Option<i64>)>,
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
        let yesterday = match self.ghost.journal().last_of(&["reflected"])? {
            Some(entry) => match entry.event {
                Event::Reflected { text } => text,
                _ => String::new(),
            },
            None => String::new(),
        };
        let template = self.prompts.plan.clone();
        let pairs = vec![
            ("date", local.format("%A %Y-%m-%d").to_string()),
            ("time", local.format("%H:%M").to_string()),
            ("sleep", clock.sleep().format("%H:%M").to_string()),
            (
                "yesterday",
                if yesterday.is_empty() {
                    "(nothing yet)".to_owned()
                } else {
                    yesterday
                },
            ),
        ];
        let system = self.self_prompt(now)?;
        let (city, country) = {
            let home = &self.ghost.identity().seed().home;
            (home.city.clone(), home.country.clone())
        };
        let (brain, senses) = (self.tick.clone(), self.senses.clone());
        let (want_weather, max_tokens) = (self.cfg.weather, self.cfg.max_tokens);
        self.planning = true;
        self.budget.spend(now);
        self.jobs.spawn(async move {
            let weather = if want_weather {
                senses.weather(&city, &country).await
            } else {
                None
            };
            let mut pairs = pairs;
            pairs.push((
                "weather",
                weather
                    .as_ref()
                    .map(|w| format!("The weather today: {w}."))
                    .unwrap_or_default(),
            ));
            let request = Completion::ask(system, narrate::fill(&template, &pairs), max_tokens)
                .expecting_json();
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
        let request =
            Completion::ask(self.self_prompt(now)?, prompt, self.cfg.max_tokens).expecting_json();
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
                    text: "(nothing written down for this part of the day)".into(),
                    block: Some(block),
                })?;
            }
        }
        self.agenda.life = Some(now);
        Ok(())
    }

    /// Look back on the day as she falls asleep.
    fn reflect(&mut self, now: DateTime<Utc>) -> Result<()> {
        if !self.cfg.live || self.reflecting {
            return Ok(());
        }
        let journal = self.ghost.journal();
        let memory = self.ghost.memory();
        let tz = self.life.clock().tz();
        let (since_seq, since_at) = journal
            .last_of(&["woke"])?
            .map_or((0, now - Duration::hours(20)), |e| (e.seq, e.at));
        let lived = journal.of_kinds_after(&["lived", "posted"], since_seq)?;
        let people: Vec<Person> = memory.people_since(since_at)?.into_iter().take(8).collect();
        if lived.is_empty() && people.is_empty() {
            return Ok(());
        }
        let today = lived
            .iter()
            .filter_map(|e| {
                let text = match &e.event {
                    Event::Lived { text, .. } => text.clone(),
                    Event::Posted { text, .. } => format!("you posted: {text}"),
                    _ => return None,
                };
                Some(format!(
                    "- {} {text}",
                    e.at.with_timezone(&tz).format("%H:%M")
                ))
            })
            .collect::<Vec<_>>();
        // Her own words, so claims she made about her life can become canon.
        let said: Vec<String> = journal
            .of_kinds_after(&["said"], since_seq)?
            .into_iter()
            .rev()
            .take(40)
            .filter_map(|e| match e.event {
                Event::Said { text, .. } => Some(format!("- {}", actions::clip(&text, 200))),
                _ => None,
            })
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        let open = memory.open_intentions()?;
        let meant = if open.is_empty() {
            "(nothing)".to_owned()
        } else {
            open.iter()
                .map(|i| {
                    format!(
                        "- by {}: {}",
                        i.due.with_timezone(&tz).format("%m-%d %H:%M"),
                        i.what
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        };
        let mut cards = Vec::with_capacity(people.len());
        for person in &people {
            let standing = will::standing(person, now);
            let mut card =
                narrate::person_card(person, &standing, &memory.notes(&person.key, 4)?, now);
            if let Some(direct) = &person.direct {
                let lines: Vec<_> = memory
                    .thread(direct, 12)?
                    .into_iter()
                    .filter(|l| l.at >= since_at)
                    .collect();
                if !lines.is_empty() {
                    card.push_str("\n  today:\n    ");
                    card.push_str(
                        &narrate::transcript(&lines, i64::MAX, tz).replace('\n', "\n    "),
                    );
                }
            }
            cards.push(card);
        }
        let prompt = narrate::fill(
            &self.prompts.reflect,
            &[
                ("time", now.with_timezone(&tz).format("%H:%M").to_string()),
                (
                    "today",
                    if today.is_empty() {
                        "(nothing written down)".to_owned()
                    } else {
                        today.join("\n")
                    },
                ),
                (
                    "people",
                    if cards.is_empty() {
                        "(nobody)".to_owned()
                    } else {
                        cards.join("\n")
                    },
                ),
                ("intentions", meant),
                ("public", self.public_review(now)?),
                (
                    "said",
                    if said.is_empty() {
                        "(nothing)".to_owned()
                    } else {
                        said.join("\n")
                    },
                ),
            ],
        );
        let request =
            Completion::ask(self.self_prompt(now)?, prompt, self.cfg.max_tokens).expecting_json();
        let brain = self.think.clone();
        self.reflecting = true;
        self.budget.spend(now);
        self.jobs.spawn(async move {
            let result = ask(brain.as_ref(), &request)
                .await
                .and_then(|v| reflect_draft(&v));
            Outcome::Reflected { people, result }
        });
        Ok(())
    }

    pub(super) fn on_reflected(
        &mut self,
        people: &[Person],
        result: Result<ReflectDraft>,
        now: DateTime<Utc>,
    ) -> Result<()> {
        self.reflecting = false;
        let draft = match result {
            Ok(draft) => draft,
            Err(e) => {
                warn!("could not look back on the day: {e}");
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
        })?;
        for (who, impression, feel) in draft.people {
            let Some(person) = find(&who) else { continue };
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
        self.learn(draft.lessons)?;
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

fn reflect_draft(v: &Value) -> Result<ReflectDraft> {
    let reflection = text(&v["reflection"]);
    if reflection.is_empty() {
        return Err(Error::Parse("no reflection".into()));
    }
    let list = |key: &str| v[key].as_array().cloned().unwrap_or_default();
    #[allow(clippy::cast_possible_truncation)]
    let people = list("people")
        .iter()
        .map(|p| {
            (
                text(&p["who"]),
                text(&p["impression"]),
                p["feel"].as_f64().unwrap_or(0.0).round().clamp(-2.0, 2.0) as i8,
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
            vec![("Haoran".into(), "asks real questions".into(), 2)]
        );
        assert_eq!(draft.intentions.len(), 1);
        assert!(reflect_draft(&json!({"reflection": ""})).is_err());
    }
}
