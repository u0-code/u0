//! Attention: hearing, and deciding when to look.
//!
//! A message is written down as unread and nothing else happens: when she
//! looks is decided here, from her day, the place, and who is in it, and
//! so is whether, once the moment comes, she looks at all. The look itself
//! is put together in `look`, what a brain makes of it comes back in
//! `thought`, and what she decided is carried out in `decided`.

use chrono::{DateTime, Duration, Utc};
use ghost_brain::{Call, Item, Step};
use ghost_core::{
    journal::Event,
    life::Phase,
    memory::{Intention, PlaceState},
    social::{self, PlaceKind},
    tempo::Notice,
    will,
};
use tracing::{debug, warn};

use super::{Attend, Mind, after};
use crate::{
    Incoming, Result,
    actions::{Action, Deliberation},
};

/// How long after she speaks somewhere she is still "in" that conversation.
const ENGAGED_FOR: Duration = Duration::minutes(3);
/// How many settles a burst may push her look out by, from the first line
/// she had not seen: about fourteen seconds at her usual pace.
const BURST_SETTLES: u32 = 4;
/// How many other conversations she gets to once her phone is in her hand.
const PHONE_SITTING: usize = 4;
/// Most she will say per hour in a place where only automated accounts talk.
const AUTOMATED_SAYS_PER_HOUR: i64 = 6;

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
    /// The servers of tools she loaded in this look: their tools are
    /// offered for the rest of it.
    pub loaded: ghost_work::tools::Loaded,
    /// Which look this is, once it has been sent off: none is 0.
    pub look: u64,
    /// Its number in the record of her looks, once it has kept a round
    /// there: what a console knows it by.
    pub kept: Option<i64>,
    /// The round waiting on what reaches beyond the conversation: when it
    /// was called, and its calls in order as the record keeps them, with
    /// those still out left `None`.
    pub out: Option<(DateTime<Utc>, Vec<Option<serde_json::Value>>)>,
    /// What the brain's last turn cost, as the provider counted it, for
    /// the record of her looks.
    pub usage: Option<ghost_brain::Usage>,
    /// What was put in front of the brain, in characters, by part, for the
    /// record of her looks: measured when the look is composed, and the
    /// tools again on each turn, as servers she loads add to them.
    pub context: std::collections::BTreeMap<String, u64>,
}

impl Thinking {
    /// The intention this look pursues, if that is why she looked.
    pub fn intention(&self) -> Option<i64> {
        match &self.why {
            Why::Intention(intention) => Some(intention.seq),
            Why::Unread => None,
        }
    }
}

/// A look whose brain call, or reach beyond the conversation, is out.
pub(super) struct Looking {
    /// Which look it is. What comes back of any other look in the place
    /// is not hers to act on: she was stopped in the middle of that one.
    pub look: u64,
    /// Calls the job off.
    pub abort: tokio::task::AbortHandle,
    /// The intention it pursues, if that is why she looked.
    pub intention: Option<i64>,
    /// Its number in the record of her looks, if it has kept a round:
    /// the same look, as a console knows it.
    pub kept: Option<i64>,
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
            loaded: ghost_work::tools::Loaded::default(),
            look: 0,
            kept: None,
            out: None,
            usage: None,
            context: std::collections::BTreeMap::new(),
        }
    }
}

/// What one turn of a brain came to.
pub(super) enum Thought {
    /// Everything she does, in one JSON reply.
    Whole(Deliberation),
    /// One turn of tool calls.
    Step(Step),
    /// What came back from beyond the conversation: the web, her machine.
    Looked(Vec<Found>),
}

/// One call that reached beyond the conversation, and what came of it:
/// nothing, when it took longer than a conversation allows.
pub(super) struct Found {
    pub call: Call,
    pub outcome: Option<ghost_work::Outcome>,
    /// How long it took, in milliseconds, or was waited for.
    pub ms: u128,
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
        let from = social::key(channel, &m.from.id);
        if m.place.kind == PlaceKind::Direct
            && self.ghost.memory().person(&from)?.is_some_and(|p| p.gone)
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
        // At a terminal she looks as soon as it is in: whatever her day is,
        // it is not hers to fit in around it.
        if self.facts(&place.channel).waited_on {
            self.agenda.attend.insert(
                place_key.to_owned(),
                Attend {
                    due: now,
                    since: now,
                    addressed: true,
                    backoff: 1.0,
                },
            );
            return Ok(());
        }
        let addressed = place.kind == PlaceKind::Direct || place.unread_addressed > 0;
        // Where she is holding back, what the others say among themselves
        // waits until something is said to her.
        if !addressed && self.holds_back(&place, now) {
            return Ok(());
        }
        // Her last word here went unanswered: the chatter is looked at once
        // the pause is over, not before.
        if !addressed && let Some(until) = self.waiting_to_be_answered(&place, now)? {
            self.agenda.attend.insert(
                place_key.to_owned(),
                Attend {
                    due: until,
                    since: now,
                    addressed: false,
                    backoff: 1.0,
                },
            );
            return Ok(());
        }
        let backoff = if addressed {
            1.0
        } else {
            self.backoff(&place, now)?
        };
        // Let a burst settle before looking, but not forever: where someone
        // always speaks before the last line has settled, she still looks
        // within a few settles of the first thing she had not seen.
        let since = self.agenda.attend.get(place_key).map_or(now, |a| a.since);
        let settle = self.tempo.settle();
        let floor = after(now, settle).min(after(since, settle * BURST_SETTLES));
        if let Some(existing) = self.agenda.attend.get_mut(place_key)
            && (existing.addressed || !addressed)
            && backoff >= existing.backoff
        {
            // Already going to look. More messages only push that out a
            // little, unless they are a reason not to wait as long.
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
            backoff,
        };
        let due = after(now, self.tempo.notice(&notice, &mut self.rng)).max(floor);
        let due = self
            .agenda
            .attend
            .get(place_key)
            .map_or(due, |existing| existing.due.min(due).max(floor));
        // Where the platform lets her answer only for a while, what is
        // said to her is looked at well within it while she is awake: an
        // answer put off past it is refused, and she would have said
        // nothing at all. Asleep, she is asleep.
        let due = match self.facts(&place.channel).answer_within(place.kind) {
            Some(within) if addressed && moment.phase != Phase::Asleep => {
                due.min(after(since, within / 2).max(floor))
            }
            _ => due,
        };
        self.agenda.attend.insert(
            place_key.to_owned(),
            Attend {
                due,
                since,
                addressed,
                backoff,
            },
        );
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
        if self.thinking.contains_key(place_key) || self.agenda.speaking.contains_key(place_key) {
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
        // A terminal does not wait on her day: not on sleep, not on how
        // often she has spoken here, not on who else is talking.
        let terminal = self.facts(&place.channel).waited_on;
        let addressed = terminal || place.kind == PlaceKind::Direct || place.unread_addressed > 0;
        if !addressed && self.holds_back(&place, now) {
            return Ok(());
        }
        if !addressed && self.waiting_to_be_answered(&place, now)?.is_some() {
            return self.schedule_attend(place_key, now);
        }
        let moment = self.life.moment(self.ghost.journal(), now)?;
        if moment.phase == Phase::Asleep && !self.tempo.instant() && !terminal {
            return self.schedule_attend(place_key, now);
        }
        if !terminal
            && self.only_bots_talking(&place)?
            && self
                .ghost
                .memory()
                .said_since(place_key, now - Duration::hours(1))?
                >= AUTOMATED_SAYS_PER_HOUR
        {
            return self.stay_out(
                &place,
                "only automated accounts are talking, and she has said enough",
            );
        }
        if !terminal && self.in_a_loop_with_bots(&place)? {
            return self.stay_out(
                &place,
                "she and automated accounts have been answering each other with no person in between",
            );
        }
        if !self.budget.allow(now) {
            // The budget is the body's, and holds at a terminal too: what
            // is said there waits a minute rather than ten.
            let wait = if terminal {
                warn!("brain budget for this hour is spent; the terminal {place_key} waits");
                Duration::minutes(1)
            } else {
                warn!("brain budget for this hour is spent; {place_key} waits");
                Duration::minutes(10)
            };
            self.agenda.attend.insert(
                place_key.to_owned(),
                Attend {
                    due: now + wait,
                    since: now,
                    addressed: true,
                    backoff: 1.0,
                },
            );
            return Ok(());
        }
        // Picking her phone up is how she comes to a chat; a terminal is not
        // on her phone.
        if addressed && !terminal {
            self.phone_in_hand(&place.key, now)?;
        }
        self.consider_digest(&place, now)?;
        let why = Why::Unread;
        self.ponder(Thinking::new(&place.key, why, !addressed), &place, now)
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

    /// She leaves what is unread in a place read and says nothing, without
    /// asking a brain, for `reason`.
    fn stay_out(&mut self, place: &PlaceState, reason: &str) -> Result<()> {
        self.record(Event::Read {
            channel: place.channel.clone(),
            place: place.id.clone(),
            upto: place.last_in_seq,
        })?;
        self.record(Event::Decided {
            about: place.key.clone(),
            decision: "stayed out of it".into(),
            reason: reason.to_owned(),
        })?;
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
}
