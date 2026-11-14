//! Attention: hearing, deciding when to look, looking, and acting on it.

use std::collections::VecDeque;

use chrono::{DateTime, Duration, Utc};
use ghost_brain::{Brain, Completion, Effort, Item, Step, Turn};
use ghost_core::{
    journal::Event,
    life::{Phase, Reach},
    memory::{Intention, Person, PlaceState},
    narrate,
    social::{self, PlaceKind},
    tempo::Notice,
    when::when,
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
/// How many other conversations she gets to once her phone is in her hand.
const PHONE_SITTING: usize = 4;
/// Most she will say per hour in a place where only automated accounts talk.
const AUTOMATED_SAYS_PER_HOUR: i64 = 6;
/// How many times a brain may ask to recall before it has to answer.
const RECALL_ROUNDS: u8 = 2;
/// How many times a failed deliberation is retried.
const THINK_RETRIES: u8 = 3;
/// How many turns a brain gets in one look, when what she does is tool
/// calls: enough to look something up and remember something before
/// answering, and no more.
const TALK_STEPS: u8 = 5;
/// How much of what a search returned she is shown.
const LOOKED_UP: usize = 3000;
/// How many of the latest messages are read for words that bring back a
/// piece of her past.
const PAST_SCAN: usize = 3;

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
    /// When what she does is tool calls: this look so far, as the brain
    /// sees it. Empty until the first turn.
    pub items: Vec<Item>,
    /// What she has decided to do so far, to be done when the look ends.
    pub took: Vec<Action>,
    /// What she made of it, in a line.
    pub thought: String,
    /// She has been told once that a thought is not a message.
    pub nudged: bool,
}

impl Thinking {
    pub fn new(place: &str, why: Why, cheap: bool) -> Self {
        Self {
            place: place.to_owned(),
            upto: 0,
            unread_chars: 0,
            round: 0,
            why,
            recalled: None,
            cheap,
            items: Vec::new(),
            took: Vec::new(),
            thought: String::new(),
            nudged: false,
        }
    }
}

/// What one turn of a brain came to.
pub(super) enum Thought {
    /// Everything she does, in one JSON reply.
    Whole(Deliberation),
    /// One turn of tool calls.
    Step(Step),
    /// What looking things up returned.
    Looked(Vec<Found>),
}

/// One look-up: the call it answers, what was asked, what came back, and
/// whether it failed.
pub(super) struct Found {
    pub call: String,
    pub query: String,
    pub output: String,
    pub failed: bool,
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
        let images = m.images;
        let heard = self.record(Event::Heard {
            channel: channel.to_owned(),
            place: m.place,
            from: m.from,
            id: m.id,
            text: m.text,
            reply_to: m.reply_to,
            addressed,
            images: images.clone(),
        })?;
        if !images.is_empty() {
            self.view(heard.seq, &place_key, &images, now)?;
        }
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
            // At her desk on something of her own, she looks up rarely.
            reach: if self.working.is_some() {
                moment.reach.max(Reach::Busy)
            } else {
                moment.reach
            },
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
        // One thing at a time. If she is in the middle of a sitting, it is
        // put down first, and she looks once it has stopped.
        if self.set_work_aside() {
            self.agenda.attend.insert(
                place_key.to_owned(),
                Attend {
                    due: now + Duration::seconds(5),
                    addressed: place.kind == PlaceKind::Direct || place.unread_addressed > 0,
                },
            );
            return Ok(());
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
        if addressed {
            self.phone_in_hand(&place.key, now)?;
        }
        self.consider_digest(&place, now)?;
        self.ponder(
            Thinking::new(&place.key, Why::Unread, !addressed),
            &place,
            now,
        )
    }

    /// She has picked her phone up to look at something meant for her.
    /// Nobody answers one conversation and then puts the phone down with
    /// three others waiting: whatever else was written to her is looked at
    /// in the same sitting, the people closest to her first, instead of each
    /// conversation waiting out a clock of its own. Group chatter that was
    /// not addressed to her keeps its own time.
    fn phone_in_hand(&mut self, looking_at: &str, now: DateTime<Utc>) -> Result<()> {
        let soon = after(now, self.tempo.settle() * 4);
        let mut waiting = Vec::new();
        for (key, attend) in &self.agenda.attend {
            if key == looking_at || !attend.addressed || attend.due <= soon {
                continue;
            }
            let Some(place) = self.ghost.memory().place(key)? else {
                continue;
            };
            waiting.push((self.patience(&place, now)?, key.clone()));
        }
        waiting.sort_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
        // One after another, with a moment between: she is still one person.
        for (turn, (_, key)) in (1u32..).zip(waiting.into_iter().take(PHONE_SITTING)) {
            let due = after(now, self.tempo.settle() * (8 * turn));
            if let Some(attend) = self.agenda.attend.get_mut(&key) {
                attend.due = attend.due.min(due);
            }
        }
        Ok(())
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
        let brain = if job.cheap {
            self.tick.clone()
        } else {
            self.think.clone()
        };
        let with_tools = self.cfg.talk_tools && brain.works();
        // What she sees is put together once for a look made of tool
        // calls, and again for each round of one made of JSON replies.
        let request = if with_tools && !job.items.is_empty() {
            None
        } else {
            Some(self.compose(place, &mut job, now, with_tools)?)
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
        if with_tools {
            let system = match request {
                Some((system, prompt)) => {
                    job.items.push(Item::user(prompt));
                    system
                }
                None => self.who(now)?,
            };
            let turn = Turn {
                system,
                items: job.items.clone(),
                tools: actions::tools(
                    &self.prompts,
                    self.work_means(),
                    self.can_look_up(),
                    self.can(&place.channel, Can::Files),
                ),
                max_tokens: self.cfg.max_tokens,
                temperature: None,
                // Talking is not a problem to be worked out at length, and
                // what she thinks is spent from the same allowance as what
                // she says.
                effort: Some(Effort::Low),
            };
            self.jobs.spawn(async move {
                let result = brain
                    .step(&turn)
                    .await
                    .map(Thought::Step)
                    .map_err(Error::from);
                Outcome::Thought {
                    job: Box::new(job),
                    result,
                }
            });
        } else if let Some((system, prompt)) = request {
            let request = Completion::ask(system, prompt, self.cfg.max_tokens)
                .expecting_json()
                .thinking(Effort::Low);
            self.jobs.spawn(async move {
                let result = deliberate(brain.as_ref(), &request)
                    .await
                    .map(Thought::Whole);
                Outcome::Thought {
                    job: Box::new(job),
                    result,
                }
            });
        }
        Ok(())
    }

    /// Put together what she sees when she looks at a place.
    /// Returns the system prompt and the prompt: who she is, and what is
    /// in front of her. `with_tools` chooses how she is told to answer.
    fn compose(
        &self,
        place: &PlaceState,
        job: &mut Thinking,
        now: DateTime<Utc>,
        with_tools: bool,
    ) -> Result<(String, String)> {
        let memory = self.ghost.memory();
        let tz = self.life.clock().tz();
        // The recent messages as they were written. Of what came before
        // them she has her own note, if the conversation is that long; then
        // everything since the note is shown, so that nothing falls between.
        let kept = memory.digest(&place.key)?;
        let mut lines = memory.thread(
            &place.key,
            self.cfg.thread_window * if kept.is_some() { 2 } else { 1 },
        )?;
        if let Some(kept) = &kept {
            lines.retain(|l| l.seq > kept.upto);
        }
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
            let (account, notes) = self.known_of(person, place, 8, now)?;
            let mut card = narrate::person_card(person, &standing, &[], now, self.life.clock());
            card.push_str(&account);
            for note in &notes {
                card.push_str(&format!("\n  · {}", note.text));
            }
            cards.push(card);
        }
        // What she already means to do here, so she does not resolve to do it twice.
        let keys: Vec<String> = people.iter().map(|p| p.key.clone()).collect();
        let pursuing = match &job.why {
            Why::Intention(intention) => Some(intention.seq),
            Why::Unread => None,
        };
        let meant: Vec<String> = self
            .intentions_about(&place.key, &keys)?
            .into_iter()
            .filter(|i| Some(i.seq) != pursuing)
            .map(|i| {
                format!(
                    "- by {}: {}",
                    i.due.with_timezone(&tz).format("%m-%d %H:%M"),
                    i.what
                )
            })
            .collect();
        if !meant.is_empty() {
            cards.push(format!("\nYou already mean to:\n{}", meant.join("\n")));
        }

        let addressed = match (place.kind, place.unread_addressed > 0) {
            (PlaceKind::Direct, _) => self.prompts.text("addressed_direct").to_owned(),
            (_, true) => self.prompts.text("addressed_you").to_owned(),
            (_, false) => self.prompts.fill(
                "addressed_nobody",
                &[(
                    "said",
                    memory
                        .said_since(&place.key, now - Duration::hours(1))?
                        .to_string(),
                )],
            ),
        };
        let why = match &job.why {
            Why::Unread => match place.unread {
                1 => "There is 1 new message here since you last looked.".to_owned(),
                n => format!("There are {n} new messages here since you last looked."),
            },
            Why::Intention(intention) => self
                .prompts
                .fill("intention_due", &[("what", intention.what.clone())]),
        };
        let recalled = self.prompts.section(
            "recalled",
            "recalled",
            job.recalled.as_deref().unwrap_or_default(),
        );

        // The pieces of her past that the last few messages touch, and only
        // those: what is in front of her every time is what she talks about.
        let recent: Vec<&str> = lines
            .iter()
            .rev()
            .take(PAST_SCAN)
            .map(|l| l.text.as_str())
            .collect();
        let touched: Vec<&ghost_core::identity::Past> = self
            .ghost
            .identity()
            .seed()
            .past
            .iter()
            .filter(|p| p.touched_by(&recent))
            .collect();
        let brought_back = self
            .prompts
            .section("past", "past", &narrate::past(&touched));

        let (who, right_now) = self.portrait_on(now, Some(&place.channel))?;
        let reply = if with_tools {
            self.prompts.reply_tools.clone()
        } else {
            narrate::fill(
                &self.prompts.reply_json,
                &[(
                    "abilities",
                    self.abilities(self.can(&place.channel, Can::Files)),
                )],
            )
        };
        let prompt = narrate::fill(
            &self.prompts.attend,
            &[
                ("where", self.describe_place(place, &people)),
                ("elsewhere", self.elsewhere(place, now)?),
                ("now", right_now),
                ("reply", reply),
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
                    format!(
                        "{}{}",
                        kept.map_or_else(String::new, |d| format!(
                            "What you kept of this conversation{} before the messages below:\n{}\n\n",
                            d.sources.map_or_else(String::new, |s| {
                                let clock = self.life.clock();
                                format!(
                                    ", from {} until {},",
                                    when(s.since, now, clock),
                                    when(s.until, now, clock)
                                )
                            }),
                            d.text
                        )),
                        narrate::transcript(&lines, place.read_upto, self.life.clock(), now)
                    ),
                ),
                ("recalled", recalled),
                ("brought_back", brought_back),
                ("why", why),
                ("addressed", addressed),
            ],
        );
        Ok((who, prompt))
    }

    pub(super) fn describe_place(&self, place: &PlaceState, people: &[Person]) -> String {
        let channel = ("channel", place.channel.clone());
        let mut out = match place.kind {
            PlaceKind::Direct => self.prompts.fill(
                "place_direct",
                &[
                    channel,
                    (
                        "who",
                        people
                            .first()
                            .map_or("someone", |p| p.name.as_str())
                            .to_owned(),
                    ),
                ],
            ),
            PlaceKind::Group => self.prompts.fill(
                "place_group",
                &[
                    channel,
                    (
                        "name",
                        place
                            .name
                            .as_deref()
                            .map_or_else(|| "(unnamed)".to_owned(), |n| format!("\"{n}\"")),
                    ),
                ],
            ),
            PlaceKind::Public => self.prompts.fill("place_public", &[channel]),
        };
        if let Some(me) = self
            .channels
            .get(&place.channel)
            .and_then(|c| c.me.as_ref())
        {
            out.push_str(
                &self.prompts.fill(
                    "place_me",
                    &[(
                        "me",
                        me.handle
                            .as_deref()
                            .map_or_else(|| me.name.clone(), |h| format!("@{h}")),
                    )],
                ),
            );
        }
        out
    }

    /// A brain answered, or failed to.
    pub(super) fn on_thought(
        &mut self,
        mut job: Thinking,
        result: Result<Thought>,
        now: DateTime<Utc>,
    ) -> Result<()> {
        self.thinking.remove(&job.place);
        let Some(place) = self.ghost.memory().place(&job.place)? else {
            return Ok(());
        };
        let thought = match result {
            Ok(thought) => thought,
            Err(e) => return self.could_not_think(&place, &job, &e, now),
        };
        self.retries.remove(&job.place);
        let deliberation = match thought {
            Thought::Whole(deliberation) => deliberation,
            Thought::Step(step) => return self.on_turn(job, step, &place, now),
            Thought::Looked(found) => {
                for f in found {
                    if !f.failed {
                        self.record(Event::LookedUp {
                            query: f.query,
                            found: f.output.clone(),
                            place: Some(place.key.clone()),
                        })?;
                    }
                    job.items
                        .push(Item::result(f.call, "look_up", f.output, f.failed));
                }
                return self.go_on(job, &place, now);
            }
        };
        if job.round < RECALL_ROUNDS {
            let found = match (deliberation.recall(), deliberation.reread()) {
                (Some(query), _) => Some(self.remembered(query, &place, now)?),
                (None, Some(n)) => Some(self.reread(n, &place, now)?),
                (None, None) => None,
            };
            if let Some(found) = found {
                // What she went back to is added to what she already found.
                job.recalled = Some(match job.recalled.take() {
                    Some(before) => format!("{before}\n\n{found}"),
                    None => found,
                });
                job.round += 1;
                return self.ponder(job, &place, now);
            }
        }
        self.apply(&place, &job, deliberation, now)
    }

    /// What a search of her memory turns up, as she would be told it.
    fn remembered(&self, query: &str, place: &PlaceState, now: DateTime<Utc>) -> Result<String> {
        let hits = self.ghost.memory().search(&place.channel, query, 8)?;
        if hits.is_empty() {
            return Ok(format!(
                "You searched your memory for \"{query}\" and found nothing."
            ));
        }
        let clock = self.life.clock();
        let mut found = Vec::with_capacity(hits.len());
        for h in &hits {
            // Something she keeps is shown as what it is, so that she can
            // refer to it by its number.
            let kept = match h.kind.as_str() {
                "kept" => self.ghost.memory().belonging(h.seq)?,
                _ => None,
            };
            let text = match kept {
                Some(kept) => format!(
                    "you kept an image ({}): {}",
                    kept.file,
                    clip(&kept.note, 300)
                ),
                None => clip(&h.text, 300),
            };
            found.push(format!(
                "- [#{}] {}{}{}",
                h.seq,
                h.at.map(|at| format!("{}: ", narrate::capital(&when(at, now, clock))))
                    .unwrap_or_default(),
                text,
                self.learned_where(h.place.as_deref(), &place.key)?
            ));
        }
        Ok(found.join("\n"))
    }

    /// What taking things on would be done with, if she can at all.
    pub(super) fn work_means(&self) -> Option<&str> {
        self.can_work().then(|| {
            self.prompts.text(if self.workshop.machine.is_some() {
                "means_machine"
            } else {
                "means_desk"
            })
        })
    }

    /// Whether she can glance something up in the middle of a conversation.
    fn can_look_up(&self) -> bool {
        self.cfg.work.web && self.workshop.web.search.is_some()
    }

    /// One turn of a look made of tool calls came back. What she asked to
    /// know is answered, what she decided to do is kept for the end, and a
    /// call that could not be read is handed back to be put right.
    fn on_turn(
        &mut self,
        mut job: Thinking,
        step: Step,
        place: &PlaceState,
        now: DateTime<Utc>,
    ) -> Result<()> {
        let said = step.text.trim().to_owned();
        if job.thought.is_empty() {
            job.thought.clone_from(&said);
        }
        let calls = step.calls.clone();
        job.items.push(step.into_item());
        job.round += 1;
        if calls.is_empty() {
            // A brain new to this writes its answer as text now and then.
            // Text is a thought, and a thought reaches nobody: say so once,
            // where someone is waiting on her.
            let waited_on = place.kind == PlaceKind::Direct || place.unread_addressed > 0;
            if !said.is_empty()
                && job.took.is_empty()
                && !job.nudged
                && waited_on
                && job.round < TALK_STEPS
            {
                job.nudged = true;
                job.items.push(Item::user(self.prompts.text("not_sent")));
                return self.ponder(job, place, now);
            }
            return self.conclude(job, place, now);
        }
        let mut again = false;
        let mut searches = Vec::new();
        for call in calls {
            let (output, failed) = match actions::from_call(&call) {
                Err(why) => {
                    again = true;
                    (why, true)
                }
                Ok(Action::Recall { query }) => {
                    again = true;
                    (self.remembered(&query, place, now)?, false)
                }
                Ok(Action::Reread { memory }) => {
                    again = true;
                    (self.reread(memory, place, now)?, false)
                }
                Ok(Action::LookUp { query }) => {
                    if self.can_look_up() {
                        tracing::info!("{}: looking up \"{}\"", place.key, clip(&query, 80));
                        searches.push((call.id, query));
                        continue;
                    }
                    again = true;
                    (self.prompts.text("no_look_up").to_owned(), true)
                }
                // Told at once when it cannot be done, so that she can
                // put it right, as with a call she got wrong.
                Ok(
                    action @ (Action::Keep { .. } | Action::LetGo { .. } | Action::SendFile { .. }),
                ) => {
                    if let Some(why) = self.refusal(place, &action)? {
                        again = true;
                        (why, true)
                    } else {
                        job.took.push(action);
                        ("ok".to_owned(), false)
                    }
                }
                Ok(action) => {
                    job.took.push(action);
                    ("ok".to_owned(), false)
                }
            };
            job.items
                .push(Item::result(call.id, call.name, output, failed));
        }
        if !searches.is_empty() {
            // The web is slow: it is looked up off to the side, and the
            // look goes on when the answers are in.
            let Some(source) = self.workshop.web.search.clone() else {
                return self.conclude(job, place, now);
            };
            self.thinking.insert(place.key.clone());
            self.jobs.spawn(async move {
                use ghost_work::Tool as _;
                let search = ghost_work::tools::Search::new(source);
                let mut found = Vec::with_capacity(searches.len());
                for (call, query) in searches {
                    let outcome = search.call(&serde_json::json!({"query": query})).await;
                    found.push(Found {
                        call,
                        query,
                        output: clip(&outcome.output, LOOKED_UP),
                        failed: outcome.failed,
                    });
                }
                Outcome::Thought {
                    job: Box::new(job),
                    result: Ok(Thought::Looked(found)),
                }
            });
            return Ok(());
        }
        if again {
            return self.go_on(job, place, now);
        }
        self.conclude(job, place, now)
    }

    /// Another turn, if she has any left; otherwise the look ends with
    /// what she has decided so far.
    fn go_on(&mut self, job: Thinking, place: &PlaceState, now: DateTime<Utc>) -> Result<()> {
        if job.round < TALK_STEPS {
            self.ponder(job, place, now)
        } else {
            self.conclude(job, place, now)
        }
    }

    /// A look made of tool calls is over: do what she decided.
    fn conclude(
        &mut self,
        mut job: Thinking,
        place: &PlaceState,
        now: DateTime<Utc>,
    ) -> Result<()> {
        let deliberation = Deliberation {
            thought: std::mem::take(&mut job.thought),
            actions: std::mem::take(&mut job.took),
        };
        self.apply(place, &job, deliberation, now)
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
                            // A public reply has to hang off something: the
                            // newest message she read, unless she chose one.
                            None if place.kind == PlaceKind::Public && bubbles.is_empty() => {
                                self.platform_id(place, job.upto)?
                            }
                            None => None,
                        };
                        bubbles.push_back(Bubble {
                            text,
                            reply_to,
                            file: None,
                        });
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
                    if self.intend(
                        now + minutes_from(minutes.clamp(1.0, 7.0 * 24.0 * 60.0)),
                        format!("get back to this conversation: {why}"),
                        about,
                        Some(place.key.clone()),
                        true,
                    )? {
                        did.push("put it off".to_owned());
                    }
                }
                // Handled before this, as another round of thought.
                Action::LookUp { .. } => {}
                Action::SendFile {
                    file,
                    message,
                    image,
                    text,
                } => match self.giveable(place, file.as_deref(), message, image)? {
                    Ok(gift) => {
                        bubbles.push_back(Bubble {
                            text: text.unwrap_or_default().trim().to_owned(),
                            reply_to: None,
                            file: Some(gift),
                        });
                        did.push("sent a file".to_owned());
                    }
                    // Where what she does is tool calls she was told at
                    // once; answering in JSON, it is only written down.
                    Err(why) => {
                        self.record(Event::Decided {
                            about: place.key.clone(),
                            decision: "did not send a file".into(),
                            reason: why,
                        })?;
                    }
                },
                Action::Undertake { brief, for_whom } => {
                    let asker = self.resolve(place, for_whom.as_deref())?;
                    match self.undertake(&brief, asker.as_ref(), Some(place), now)? {
                        None => did.push("took something on".to_owned()),
                        Some(why) => {
                            self.record(Event::Decided {
                                about: place.key.clone(),
                                decision: "did not take it on".into(),
                                reason: why,
                            })?;
                        }
                    }
                }
                Action::Note { about, text } => {
                    if let Some(person) = self.resolve(place, about.as_deref())? {
                        let sources = self.shown(place)?;
                        self.record(Event::Noted {
                            about: person.key,
                            text,
                            place: Some(place.key.clone()),
                            sources,
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
                    if self.intend(
                        now + minutes_from(hours.clamp(0.25, 24.0 * 30.0) * 60.0),
                        what,
                        person.map(|p| p.key),
                        Some(target),
                        false,
                    )? {
                        did.push("made a note to self".to_owned());
                    }
                }
                Action::Keep {
                    message,
                    image,
                    note,
                } => match self.keep(place, message, image, &note)? {
                    None => did.push("kept something".to_owned()),
                    // Nothing to keep is nothing done: there is no entry.
                    Some(why) => debug!("{}: kept nothing: {why}", place.key),
                },
                Action::LetGo { thing } => {
                    if self.let_go(thing)? {
                        did.push("let something go".to_owned());
                    }
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
                Action::Recall { .. } | Action::Reread { .. } | Action::Post { .. } => {}
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
