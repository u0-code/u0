//! Attention: hearing, deciding when to look, looking, and acting on it.

use std::collections::VecDeque;

use chrono::{DateTime, Duration, Utc};
use ghost_brain::{Brain, Completion};
use ghost_core::{
    journal::Event,
    life::Phase,
    memory::{Intention, Person, PlaceState},
    narrate,
    social::{self, PlaceKind},
    tempo::Notice,
    will,
};
use tracing::{debug, warn};

use super::{Attend, Mind, Outcome, after, speak::Bubble};
use crate::{
    Act, Can, Error, Incoming, Result,
    actions::{self, Action, Deliberation, clip},
};

/// How long after she speaks somewhere she is still "in" that conversation.
const ENGAGED_FOR: Duration = Duration::minutes(3);
/// Most she will say per hour in a place where only automated accounts talk.
const AUTOMATED_SAYS_PER_HOUR: i64 = 6;
/// How many times a brain may ask to recall before it has to answer.
const RECALL_ROUNDS: u8 = 2;
/// How many times a failed deliberation is retried.
const THINK_RETRIES: u8 = 3;

/// One look at a place, from start to finish.
pub(super) struct Thinking {
    pub place: String,
    /// Seq of the newest incoming message she saw.
    pub upto: i64,
    /// How much there was to read.
    pub unread_chars: usize,
    pub round: u8,
    pub why: Why,
    pub recalled: Option<String>,
    /// Use the tick brain: nobody is talking to her.
    pub cheap: bool,
}

/// What made her look.
pub(super) enum Why {
    Unread,
    Intention(Intention),
}

impl Mind {
    /// A message reached her. It goes in the journal as unread; nothing else
    /// happens until she looks.
    pub(super) fn hear(&mut self, channel: &str, m: Incoming, now: DateTime<Utc>) -> Result<()> {
        let place_key = social::key(channel, &m.place.id);
        let from_her = self
            .channels
            .get(channel)
            .and_then(|c| c.me.as_ref())
            .is_some_and(|me| me.id == m.from.id);
        if from_her || m.text.trim().is_empty() {
            return Ok(());
        }
        if self.ghost.memory().knows_message(&place_key, &m.id)? {
            debug!("{place_key}: {} delivered twice", m.id);
            return Ok(());
        }
        if m.place.kind == PlaceKind::Direct
            && self
                .ghost
                .memory()
                .person(&social::key(channel, &m.from.id))?
                .is_some_and(|p| p.gone)
        {
            // She left. Their private messages no longer reach her.
            debug!("{place_key}: from someone she has left");
            return Ok(());
        }
        // A reply to something she said is meant for her, whether or not the
        // channel could tell.
        let addressed = m.addressed
            || match &m.reply_to {
                Some(id) => self.ghost.memory().is_hers(&place_key, id)?,
                None => false,
            };
        self.record(Event::Heard {
            channel: channel.to_owned(),
            place: m.place,
            from: m.from,
            id: m.id,
            text: m.text,
            reply_to: m.reply_to,
            addressed,
        })?;
        self.schedule_attend(&place_key, now)
    }

    /// Decide when she will next look at a place with something unread.
    pub(super) fn schedule_attend(&mut self, place_key: &str, now: DateTime<Utc>) -> Result<()> {
        let Some(place) = self.ghost.memory().place(place_key)? else {
            return Ok(());
        };
        if place.unread == 0 {
            return Ok(());
        }
        let addressed = place.kind == PlaceKind::Direct || place.unread_addressed > 0;
        // Let a burst settle before looking.
        let floor = after(now, self.tempo.settle());
        if let Some(existing) = self.agenda.attend.get_mut(place_key)
            && (existing.addressed || !addressed)
        {
            // Already going to look. More messages only push that out a little.
            existing.due = existing.due.max(floor);
            return Ok(());
        }
        let moment = self.life.moment(self.ghost.journal(), now)?;
        let notice = Notice {
            phase: moment.phase,
            reach: moment.reach,
            until: (moment.until - now).to_std().unwrap_or_default(),
            engaged: place.last_out_at.is_some_and(|at| now - at < ENGAGED_FOR),
            addressed,
            patience: self.patience(&place, now)?,
        };
        let due = after(now, self.tempo.notice(&notice, &mut self.rng)).max(floor);
        let due = self
            .agenda
            .attend
            .get(place_key)
            .map_or(due, |existing| existing.due.min(due).max(floor));
        self.agenda
            .attend
            .insert(place_key.to_owned(), Attend { due, addressed });
        Ok(())
    }

    /// How her standing with whoever is in a private place stretches the wait.
    fn patience(&self, place: &PlaceState, now: DateTime<Utc>) -> Result<f32> {
        if place.kind != PlaceKind::Direct {
            return Ok(1.0);
        }
        Ok(self
            .ghost
            .memory()
            .person_of_direct(&place.key)?
            .map_or(1.0, |p| {
                will::standing(&p, now).stage.patience() * if p.automated { 3.0 } else { 1.0 }
            }))
    }

    /// The moment she looks at a place.
    pub(super) fn attend(&mut self, place_key: &str, now: DateTime<Utc>) -> Result<()> {
        self.agenda.attend.remove(place_key);
        if self.thinking.contains(place_key) || self.agenda.speaking.contains_key(place_key) {
            // Already on it. Whatever is new gets picked up when that finishes.
            return Ok(());
        }
        let Some(place) = self.ghost.memory().place(place_key)? else {
            return Ok(());
        };
        if place.unread == 0 || !self.channels.contains_key(&place.channel) {
            // Nothing new, or no way to look. Coming back online reschedules.
            return Ok(());
        }
        let moment = self.life.moment(self.ghost.journal(), now)?;
        if moment.phase == Phase::Asleep && !self.tempo.instant() {
            return self.schedule_attend(place_key, now);
        }
        if self.only_bots_talking(&place)?
            && self
                .ghost
                .memory()
                .said_since(place_key, now - Duration::hours(1))?
                >= AUTOMATED_SAYS_PER_HOUR
        {
            self.record(Event::Read {
                channel: place.channel.clone(),
                place: place.id.clone(),
                upto: place.last_in_seq,
            })?;
            self.record(Event::Decided {
                about: place.key.clone(),
                decision: "stayed out of it".into(),
                reason: "only automated accounts are talking, and she has said enough".into(),
            })?;
            return Ok(());
        }
        if !self.budget.allow(now) {
            warn!("brain budget for this hour is spent; {place_key} waits");
            self.agenda.attend.insert(
                place_key.to_owned(),
                Attend {
                    due: now + Duration::minutes(10),
                    addressed: true,
                },
            );
            return Ok(());
        }
        let addressed = place.kind == PlaceKind::Direct || place.unread_addressed > 0;
        self.ponder(
            Thinking {
                place: place.key.clone(),
                upto: 0,
                unread_chars: 0,
                round: 0,
                why: Why::Unread,
                recalled: None,
                cheap: !addressed,
            },
            &place,
            now,
        )
    }

    fn only_bots_talking(&self, place: &PlaceState) -> Result<bool> {
        let memory = self.ghost.memory();
        let unread: Vec<_> = memory
            .thread(&place.key, self.cfg.thread_window)?
            .into_iter()
            .filter(|l| l.incoming && l.seq > place.read_upto)
            .collect();
        for line in &unread {
            let automated = match &line.person {
                Some(key) => memory.person(key)?.is_some_and(|p| p.automated),
                None => false,
            };
            if !automated {
                return Ok(false);
            }
        }
        Ok(!unread.is_empty())
    }

    /// Hand a look at a place to a brain. The answer comes back as an outcome.
    pub(super) fn ponder(
        &mut self,
        mut job: Thinking,
        place: &PlaceState,
        now: DateTime<Utc>,
    ) -> Result<()> {
        let request = self.compose(place, &mut job, now)?;
        let brain = if job.cheap {
            self.tick.clone()
        } else {
            self.think.clone()
        };
        self.budget.spend(now);
        self.thinking.insert(place.key.clone());
        if job.round == 0 && matches!(job.why, Why::Unread) && self.can(&place.channel, Can::Read) {
            let upto = self
                .ghost
                .memory()
                .line(job.upto)?
                .and_then(|(_, line)| line.platform_id);
            self.fire(
                &place.channel,
                Act::Read {
                    place: place.id.clone(),
                    upto,
                },
            );
        }
        self.jobs.spawn(async move {
            let result = deliberate(brain.as_ref(), &request).await;
            Outcome::Thought { job, result }
        });
        Ok(())
    }

    /// Put together what she sees when she looks at a place.
    fn compose(
        &self,
        place: &PlaceState,
        job: &mut Thinking,
        now: DateTime<Utc>,
    ) -> Result<Completion> {
        let memory = self.ghost.memory();
        let tz = self.life.clock().tz();
        let lines = memory.thread(&place.key, self.cfg.thread_window)?;
        job.upto = lines
            .iter()
            .filter(|l| l.incoming)
            .map(|l| l.seq)
            .max()
            .unwrap_or(0)
            .max(place.read_upto);
        job.unread_chars = lines
            .iter()
            .filter(|l| l.incoming && l.seq > place.read_upto)
            .map(|l| l.text.chars().count())
            .sum();

        let mut people = memory.people_in(&place.key, 6)?;
        if people.is_empty()
            && let Some(person) = memory.person_of_direct(&place.key)?
        {
            people.push(person);
        }
        let mut cards = Vec::with_capacity(people.len());
        for person in &people {
            let standing = will::standing(person, now);
            let notes = memory.notes(&person.key, 8)?;
            cards.push(narrate::person_card(person, &standing, &notes, now));
        }

        let addressed = match (place.kind, place.unread_addressed > 0) {
            (PlaceKind::Direct, _) => {
                "This is a private conversation. Whatever is new was written to you.".to_owned()
            }
            (_, true) => "Someone here addressed you directly.".to_owned(),
            (_, false) => format!(
                "Nobody addressed you. In a shared space the normal thing is to say nothing unless you actually have something to add. You have spoken here {} time(s) in the last hour.",
                memory.said_since(&place.key, now - Duration::hours(1))?
            ),
        };
        let why = match &job.why {
            Why::Unread => match place.unread {
                1 => "There is 1 new message here since you last looked.".to_owned(),
                n => format!("There are {n} new messages here since you last looked."),
            },
            Why::Intention(intention) => format!(
                "Nobody just wrote. Earlier you told yourself: \"{}\". The time you set has come. Decide whether to do it now, and how. If the moment has passed, let it go.",
                intention.what
            ),
        };
        let recalled = job
            .recalled
            .as_deref()
            .map(|r| format!("\n# What you remember\n\n{r}\n"))
            .unwrap_or_default();

        let prompt = narrate::fill(
            &self.prompts.attend,
            &[
                ("where", self.describe_place(place, &people)),
                (
                    "people",
                    if cards.is_empty() {
                        "(nobody you know yet)".to_owned()
                    } else {
                        cards.join("\n")
                    },
                ),
                (
                    "transcript",
                    narrate::transcript(&lines, place.read_upto, tz),
                ),
                ("recalled", recalled),
                ("why", why),
                ("addressed", addressed),
            ],
        );
        Ok(Completion::ask(self.self_prompt(now)?, prompt, self.cfg.max_tokens).expecting_json())
    }

    fn describe_place(&self, place: &PlaceState, people: &[Person]) -> String {
        let mut out = match place.kind {
            PlaceKind::Direct => format!(
                "{}: a private conversation with {}.",
                place.channel,
                people.first().map_or("someone", |p| p.name.as_str())
            ),
            PlaceKind::Group => format!(
                "{}: the group chat {}. Several people are here, and not everything said is meant for you.",
                place.channel,
                place
                    .name
                    .as_deref()
                    .map_or_else(|| "(unnamed)".to_owned(), |n| format!("\"{n}\""))
            ),
            PlaceKind::Public => format!(
                "{}: a public thread. Anyone can read what is written here.",
                place.channel
            ),
        };
        if let Some(me) = self
            .channels
            .get(&place.channel)
            .and_then(|c| c.me.as_ref())
        {
            out.push_str(&format!(
                " You are {} here.",
                me.handle
                    .as_deref()
                    .map_or_else(|| me.name.clone(), |h| format!("@{h}"))
            ));
        }
        out
    }

    /// A brain answered, or failed to.
    pub(super) fn on_thought(
        &mut self,
        mut job: Thinking,
        result: Result<Deliberation>,
        now: DateTime<Utc>,
    ) -> Result<()> {
        self.thinking.remove(&job.place);
        let Some(place) = self.ghost.memory().place(&job.place)? else {
            return Ok(());
        };
        let deliberation = match result {
            Ok(d) => d,
            Err(e) => return self.could_not_think(&place, &job, &e, now),
        };
        self.retries.remove(&job.place);
        if job.round < RECALL_ROUNDS
            && let Some(query) = deliberation.recall()
        {
            let hits = self.ghost.memory().search(query, 8)?;
            let tz = self.life.clock().tz();
            job.recalled = Some(if hits.is_empty() {
                format!("You searched your memory for \"{query}\" and found nothing.")
            } else {
                hits.iter()
                    .map(|h| {
                        format!(
                            "- {} {}",
                            h.at.map(|at| at.with_timezone(&tz).format("%Y-%m-%d").to_string())
                                .unwrap_or_default(),
                            clip(&h.text, 300)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            });
            job.round += 1;
            return self.ponder(job, &place, now);
        }
        self.apply(&place, &job, deliberation, now)
    }

    fn could_not_think(
        &mut self,
        place: &PlaceState,
        job: &Thinking,
        error: &Error,
        now: DateTime<Utc>,
    ) -> Result<()> {
        warn!("{}: {error}", place.key);
        if let Why::Intention(intention) = &job.why {
            // Try the intention again later rather than losing it.
            self.pursuing.remove(&intention.seq);
            self.held.insert(intention.seq, now + Duration::minutes(10));
            return self.refresh_intentions();
        }
        let tries = self.retries.entry(place.key.clone()).or_default();
        *tries += 1;
        if *tries < THINK_RETRIES {
            let due = now + Duration::seconds(20 * i64::from(*tries));
            self.agenda.attend.insert(
                place.key.clone(),
                Attend {
                    due,
                    addressed: true,
                },
            );
            return Ok(());
        }
        // Give up on this look so she does not loop on it forever.
        self.retries.remove(&place.key);
        self.record(Event::Read {
            channel: place.channel.clone(),
            place: place.id.clone(),
            upto: job.upto,
        })?;
        self.record(Event::Decided {
            about: place.key.clone(),
            decision: "nothing".into(),
            reason: format!("could not think: {error}"),
        })?;
        Ok(())
    }

    /// Turn what she decided into journal entries and things in motion.
    fn apply(
        &mut self,
        place: &PlaceState,
        job: &Thinking,
        deliberation: Deliberation,
        now: DateTime<Utc>,
    ) -> Result<()> {
        if job.upto > place.read_upto {
            self.record(Event::Read {
                channel: place.channel.clone(),
                place: place.id.clone(),
                upto: job.upto,
            })?;
        }
        let mut bubbles = VecDeque::new();
        let mut did: Vec<String> = Vec::new();
        for action in deliberation.actions {
            match action {
                Action::Say { text, reply_to } => {
                    let text = text.trim().to_owned();
                    if !text.is_empty() {
                        let reply_to = match reply_to {
                            Some(seq) => self.platform_id(place, seq)?,
                            None => None,
                        };
                        bubbles.push_back(Bubble { text, reply_to });
                    }
                }
                Action::React { to, emoji } => {
                    if self.can(&place.channel, Can::React)
                        && let Some(id) = self.platform_id(place, to)?
                    {
                        self.fire(
                            &place.channel,
                            Act::React {
                                place: place.id.clone(),
                                to: id.clone(),
                                emoji: emoji.clone(),
                            },
                        );
                        self.record(Event::Reacted {
                            channel: place.channel.clone(),
                            place: place.id.clone(),
                            to: id,
                            emoji: emoji.clone(),
                        })?;
                        did.push(format!("reacted {emoji}"));
                    }
                }
                Action::Silence { why } => did.push(if why.is_empty() {
                    "stayed quiet".to_owned()
                } else {
                    format!("stayed quiet: {why}")
                }),
                Action::Later { minutes, why } => {
                    let about = self.resolve(place, None)?.map(|p| p.key);
                    self.record(Event::Intended {
                        due: now + minutes_from(minutes.clamp(1.0, 7.0 * 24.0 * 60.0)),
                        what: format!("get back to this conversation: {why}"),
                        about,
                        place: Some(place.key.clone()),
                    })?;
                    did.push("put it off".to_owned());
                }
                Action::Note { about, text } => {
                    if let Some(person) = self.resolve(place, about.as_deref())? {
                        self.record(Event::Noted {
                            about: person.key,
                            text,
                        })?;
                        did.push("noted something".to_owned());
                    }
                }
                Action::Feel { toward, delta, why } => {
                    #[allow(clippy::cast_possible_truncation)]
                    let delta = delta.round().clamp(-3.0, 3.0) as i8;
                    if delta != 0
                        && let Some(person) = self.resolve(place, toward.as_deref())?
                    {
                        self.record(Event::Felt {
                            toward: person.key,
                            delta,
                            why,
                        })?;
                        did.push(format!("felt {delta:+}"));
                    }
                }
                Action::Intend { hours, what, about } => {
                    let person = self.resolve(place, about.as_deref())?;
                    let target = person
                        .as_ref()
                        .and_then(|p| p.direct.clone())
                        .unwrap_or_else(|| place.key.clone());
                    self.record(Event::Intended {
                        due: now + minutes_from(hours.clamp(0.25, 24.0 * 30.0) * 60.0),
                        what,
                        about: person.map(|p| p.key),
                        place: Some(target),
                    })?;
                    did.push("made a note to self".to_owned());
                }
                Action::Canon { fact } => {
                    self.record(Event::Canon {
                        fact,
                        source: format!("said in {}", place.key),
                    })?;
                }
                Action::Leave { why } => {
                    if let Some(done) = self.leave(place, why, now)? {
                        did.push(done.to_owned());
                    }
                }
                // Recall has had its rounds; posting only happens while scrolling.
                Action::Recall { .. } | Action::Post { .. } => {}
            }
        }
        if !bubbles.is_empty() {
            did.insert(
                0,
                match bubbles.len() {
                    1 => "said something".to_owned(),
                    n => format!("said {n} things"),
                },
            );
        }
        let decision = if did.is_empty() {
            "nothing".to_owned()
        } else {
            did.join("; ")
        };
        self.record(Event::Decided {
            about: place.key.clone(),
            decision: decision.clone(),
            reason: deliberation.thought,
        })?;
        if let Why::Intention(intention) = &job.why {
            self.record(Event::Settled {
                intention: intention.seq,
                outcome: decision,
            })?;
            self.pursuing.remove(&intention.seq);
        }
        self.refresh_intentions()?;
        if bubbles.is_empty() {
            // Pick up anything that arrived while she was thinking.
            self.schedule_attend(&place.key, now)
        } else {
            self.begin_speaking(place, job, bubbles, now);
            Ok(())
        }
    }

    /// Leaving is the end of a process. Before things have cooled it only
    /// registers as how badly she wanted to.
    fn leave(
        &mut self,
        place: &PlaceState,
        why: String,
        now: DateTime<Utc>,
    ) -> Result<Option<&'static str>> {
        if place.kind != PlaceKind::Direct {
            return Ok(None);
        }
        let Some(person) = self.ghost.memory().person_of_direct(&place.key)? else {
            return Ok(None);
        };
        if will::may_leave(&will::standing(&person, now)) {
            self.record(Event::Left {
                person: person.key,
                why,
            })?;
            Ok(Some("left"))
        } else {
            self.record(Event::Felt {
                toward: person.key,
                delta: -3,
                why: format!("wanted to leave: {why}"),
            })?;
            Ok(Some("pulled away"))
        }
    }

    /// The channel's id for a message she pointed at, if it is in this place.
    fn platform_id(&self, place: &PlaceState, seq: i64) -> Result<Option<String>> {
        Ok(self
            .ghost
            .memory()
            .line(seq)?
            .filter(|(key, _)| *key == place.key)
            .and_then(|(_, line)| line.platform_id))
    }

    /// Who a brain meant by a name. No name means whoever this private
    /// conversation is with.
    fn resolve(&self, place: &PlaceState, name: Option<&str>) -> Result<Option<Person>> {
        let memory = self.ghost.memory();
        let direct = || -> Result<Option<Person>> {
            if place.kind == PlaceKind::Direct {
                Ok(memory.person_of_direct(&place.key)?)
            } else {
                Ok(None)
            }
        };
        let Some(name) = name.map(|n| n.trim().trim_start_matches('@').to_lowercase()) else {
            return direct();
        };
        if name.is_empty() {
            return direct();
        }
        let found = memory.people_in(&place.key, 30)?.into_iter().find(|p| {
            p.name.to_lowercase() == name
                || p.handle
                    .as_deref()
                    .is_some_and(|h| h.to_lowercase() == name)
        });
        match found {
            Some(person) => Ok(Some(person)),
            None => direct(),
        }
    }
}

async fn deliberate(brain: &dyn Brain, request: &Completion) -> Result<Deliberation> {
    let reply = brain.complete(request).await?;
    actions::parse(&reply.text).map_err(Error::Parse)
}

#[allow(clippy::cast_possible_truncation)]
pub(super) fn minutes_from(minutes: f64) -> Duration {
    Duration::seconds((minutes * 60.0).round() as i64)
}
