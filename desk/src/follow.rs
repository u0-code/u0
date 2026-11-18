//! Following her steps: what she called while she looked at a place
//! (`look.steps`) and what a piece of work did, step by step
//! (`work.steps`), kept per ghost and asked for only while it is worth it.
//!
//! The thread on screen says what it would show: the place, whether she
//! is thinking there, the newest entry in its journal a look can end with,
//! and the work whose cards are open. From that alone this decides when to
//! ask, so that the decisions can be tested without a daemon:
//!
//! - A place is asked about once when the thread first shows it, whether
//!   or not anything is going on there, once more each time an entry a
//!   look ends with (`decided`, `stopped`) comes in, and about once a
//!   second while she thinks there or a look of hers there is still going
//!   on.
//! - A piece of work, or a part of it, is asked about when its card is
//!   opened, every two seconds while it is open and still going, once more
//!   after it ends, and again at once while its answer says there is more.
//!
//! - The work of the place on screen whose context the right column shows
//!   is asked about for that alone, by the same rule as an open card,
//!   without its steps.
//!
//! One ask is in flight per place and per work at a time. An ask that is
//! refused (a daemon from before these methods, a work it no longer has)
//! is not repeated for a while; one that never comes back is given up on.

use std::{
    collections::{BTreeMap, HashMap},
    time::{Duration, Instant},
};

use ghost_link::protocol::{LoggedStep, Look, WorkContext, WorkLog, WorkPart};

/// How often a place she is thinking in is asked about.
pub const LOOK_EVERY: Duration = Duration::from_secs(1);
/// How often open work whose card is open is asked about.
pub const WORK_EVERY: Duration = Duration::from_secs(2);
/// How long an ask may go unanswered before it is taken as lost: the
/// connection it went out on went away.
const LOST: Duration = Duration::from_secs(15);
/// How long to leave a place or a work alone after an ask was refused.
const QUIET: Duration = Duration::from_secs(30);
/// How many steps of one piece of work are held: a card shows the latest.
const STEPS_KEPT: usize = 2000;

/// Something to ask her daemon, read-only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Read {
    /// `look.steps` at a place, by key: the looks that ended after the
    /// journal entry `after`, and the one going on.
    Looks { place: String, after: Option<u64> },
    /// `work.steps` of a work or a part, by id, after the step `after`.
    Steps { work: String, after: Option<u64> },
    /// `journal.place`: the entries of the place, by key, so that a thread
    /// older than the tail the desk holds can be shown.
    Place { place: String },
    /// `work.steps` of a work, by id, for what its newest step put in front
    /// of its model, and none of its steps.
    Context { work: String },
}

/// What one ghost's thread has learned of her steps.
#[derive(Debug, Default)]
pub struct Following {
    /// By place key.
    pub places: HashMap<String, PlaceLooks>,
    /// By work id, parts by their own.
    pub works: HashMap<String, WorkSteps>,
    /// What filled a worker's context, by work id.
    pub contexts: HashMap<String, Contexts>,
}

/// An ask that is due: after which journal entry, or which step, to read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct After(Option<u64>);

/// An ask in flight: when it went, and what was known when it did.
#[derive(Debug, Clone, Copy)]
struct Asked {
    at: Instant,
    /// For a place: the newest entry a look could end with then.
    upto: Option<i64>,
    /// For a work: whether it was still going then.
    open: bool,
}

impl Asked {
    fn pending(&self, now: Instant) -> bool {
        now.duration_since(self.at) < LOST
    }
}

/// Her looks at one place, by number.
#[derive(Debug, Default)]
pub struct PlaceLooks {
    pub looks: BTreeMap<u64, Look>,
    /// The newest entry an ended look was tied to: asks go on from there.
    after: Option<u64>,
    asked: Option<Asked>,
    answered: Option<Instant>,
    /// The newest entry a look could end with when the last answer was
    /// asked for: one newer is a look that may have ended since.
    covered: Option<i64>,
    quiet_until: Option<Instant>,
}

impl PlaceLooks {
    /// The look tied to the journal entry `seq`, if one is.
    pub fn ended_with(&self, seq: i64) -> Option<&Look> {
        self.looks.values().find(|l| l.seq == Some(seq))
    }

    /// The look she is in the middle of, if any.
    pub fn running(&self) -> Option<&Look> {
        self.looks.values().find(|l| l.running)
    }

    /// Whether to ask now, and from where: `thinking` is whether she is
    /// thinking about what was said there, `newest_end` the newest entry
    /// in the thread a look can end with.
    fn due(&self, now: Instant, thinking: bool, newest_end: Option<i64>) -> Option<After> {
        if self.asked.is_some_and(|a| a.pending(now)) || self.quiet_until.is_some_and(|q| now < q) {
            return None;
        }
        let Some(answered) = self.answered else {
            // A place first shown is asked about once, whatever is going
            // on there: what filled her context last is part of it.
            return Some(After(None));
        };
        let live = thinking || self.running().is_some();
        let ended = newest_end > self.covered;
        let again = live && now.duration_since(answered) >= LOOK_EVERY;
        (ended || again).then_some(After(self.after))
    }

    fn take(&mut self, now: Instant, answer: Result<Vec<Look>, String>) {
        let asked = self.asked.take();
        self.answered = Some(now);
        self.covered = asked.and_then(|a| a.upto).max(self.covered);
        let Ok(looks) = answer else {
            self.quiet_until = Some(now + QUIET);
            return;
        };
        // Whatever was going on and is not in the answer was given up on
        // and tried again: it will not end with an entry.
        self.looks.retain(|_, l| !l.running);
        for look in looks {
            if let Some(seq) = look.seq.and_then(|s| u64::try_from(s).ok()) {
                self.after = self.after.max(Some(seq));
            }
            self.looks.insert(look.look, look);
        }
        // An entry a look the answer had ended with needs no ask of its own.
        let ended = self.after.and_then(|a| i64::try_from(a).ok());
        self.covered = self.covered.max(ended);
    }
}

/// One piece of work's steps, and the parts it handed out.
#[derive(Debug, Default)]
pub struct WorkSteps {
    pub steps: Vec<LoggedStep>,
    pub parts: Vec<WorkPart>,
    /// There are more after the last one held.
    more: bool,
    asked: Option<Asked>,
    answered: Option<Instant>,
    /// Whether the work was still going when the last answer was asked for.
    answered_open: bool,
    quiet_until: Option<Instant>,
}

impl WorkSteps {
    /// Whether any have been read yet.
    pub fn read(&self) -> bool {
        self.answered.is_some()
    }

    fn last(&self) -> Option<u64> {
        self.steps.last().map(|s| s.n)
    }

    /// Whether to ask now, and after which step: `open` is whether the
    /// work is still going.
    fn due(&self, now: Instant, open: bool) -> Option<After> {
        if self.asked.is_some_and(|a| a.pending(now)) || self.quiet_until.is_some_and(|q| now < q) {
            return None;
        }
        let Some(answered) = self.answered else {
            return Some(After(None));
        };
        let again = open && now.duration_since(answered) >= WORK_EVERY;
        // It ended since it was last asked about: its last steps.
        let ended = !open && self.answered_open;
        (self.more || again || ended).then_some(After(self.last()))
    }

    fn take(&mut self, now: Instant, answer: Result<WorkLog, String>) {
        let asked = self.asked.take();
        self.answered = Some(now);
        let Ok(log) = answer else {
            self.quiet_until = Some(now + QUIET);
            self.more = false;
            self.answered_open = false;
            return;
        };
        let last = self.last();
        self.steps
            .extend(log.steps.into_iter().filter(|s| Some(s.n) > last));
        if self.steps.len() > STEPS_KEPT {
            self.steps.drain(..self.steps.len() - STEPS_KEPT);
        }
        self.parts = log.parts;
        self.more = log.more;
        self.answered_open = asked.is_some_and(|a| a.open);
    }
}

/// What filled one worker's context at its newest step, followed for
/// that alone.
#[derive(Debug, Default)]
pub struct Contexts {
    pub newest: Option<WorkContext>,
    asked: Option<Asked>,
    answered: Option<Instant>,
    answered_open: bool,
    quiet_until: Option<Instant>,
}

impl Contexts {
    /// Whether to ask now: once at first, every two seconds while the
    /// work is open, and once more after it ends.
    fn due(&self, now: Instant, open: bool) -> bool {
        if self.asked.is_some_and(|a| a.pending(now)) || self.quiet_until.is_some_and(|q| now < q) {
            return false;
        }
        let Some(answered) = self.answered else {
            return true;
        };
        (open && now.duration_since(answered) >= WORK_EVERY) || (!open && self.answered_open)
    }

    /// Keep `context` if it is newer than the one held.
    fn keep(&mut self, context: Option<WorkContext>) {
        if let Some(c) = context
            && self.newest.as_ref().is_none_or(|held| held.n <= c.n)
        {
            self.newest = Some(c);
        }
    }

    fn take(&mut self, now: Instant, answer: Result<Option<WorkContext>, String>) {
        let asked = self.asked.take();
        self.answered = Some(now);
        self.answered_open = asked.is_some_and(|a| a.open);
        if let Ok(context) = answer {
            self.keep(context);
        } else {
            self.quiet_until = Some(now + QUIET);
            self.answered_open = false;
        }
    }
}

impl Following {
    /// What to ask about the place `place` now, if anything; it is taken
    /// as asked.
    pub fn place(
        &mut self,
        place: &str,
        now: Instant,
        thinking: bool,
        newest_end: Option<i64>,
    ) -> Option<Read> {
        let looks = self.places.entry(place.to_owned()).or_default();
        let After(after) = looks.due(now, thinking, newest_end)?;
        looks.asked = Some(Asked {
            at: now,
            upto: newest_end,
            open: false,
        });
        Some(Read::Looks {
            place: place.to_owned(),
            after,
        })
    }

    /// What to ask about the work `work` now, if anything; it is taken as
    /// asked.
    pub fn work(&mut self, work: &str, now: Instant, open: bool) -> Option<Read> {
        let steps = self.works.entry(work.to_owned()).or_default();
        let After(after) = steps.due(now, open)?;
        steps.asked = Some(Asked {
            at: now,
            upto: None,
            open,
        });
        Some(Read::Steps {
            work: work.to_owned(),
            after,
        })
    }

    /// What to ask about the context of the work `work` now, if anything;
    /// it is taken as asked.
    pub fn context(&mut self, work: &str, now: Instant, open: bool) -> Option<Read> {
        let held = self.contexts.entry(work.to_owned()).or_default();
        if !held.due(now, open) {
            return None;
        }
        held.asked = Some(Asked {
            at: now,
            upto: None,
            open,
        });
        Some(Read::Context {
            work: work.to_owned(),
        })
    }

    /// Take in what filled a worker's context, or why it could not be read.
    pub fn took_context(
        &mut self,
        work: &str,
        now: Instant,
        answer: Result<Option<WorkContext>, String>,
    ) {
        self.contexts
            .entry(work.to_owned())
            .or_default()
            .take(now, answer);
    }

    /// What filled the context of the work `work` at its newest step, as
    /// far as the desk has heard.
    pub fn work_context(&self, work: &str) -> Option<&WorkContext> {
        self.contexts.get(work)?.newest.as_ref()
    }

    /// Take in her looks at `place`, or why they could not be read.
    pub fn looks(&mut self, place: &str, now: Instant, answer: Result<Vec<Look>, String>) {
        self.places
            .entry(place.to_owned())
            .or_default()
            .take(now, answer);
    }

    /// Take in a work's steps, or why they could not be read. What its
    /// newest step was given comes with them.
    pub fn steps(&mut self, work: &str, now: Instant, answer: Result<WorkLog, String>) {
        if let Ok(log) = &answer
            && let Some(held) = self.contexts.get_mut(work)
        {
            held.keep(log.context.clone());
        }
        self.works
            .entry(work.to_owned())
            .or_default()
            .take(now, answer);
    }

    /// Forget everything asked and not answered: the connection went away,
    /// and nothing asked on it will be.
    pub fn lost(&mut self) {
        for p in self.places.values_mut() {
            p.asked = None;
        }
        for w in self.works.values_mut() {
            w.asked = None;
        }
        for c in self.contexts.values_mut() {
            c.asked = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use ghost_link::protocol::{LookRound, WorkState};

    use super::*;

    fn look(n: u64, seq: Option<i64>) -> Look {
        Look {
            look: n,
            seq,
            running: seq.is_none(),
            rounds: vec![LookRound {
                at: "2026-11-16T12:00:00Z".into(),
                calls: Vec::new(),
                usage: None,
                context: None,
            }],
        }
    }

    fn step(n: u64) -> LoggedStep {
        LoggedStep {
            n,
            at: None,
            said: String::new(),
            ms: None,
            calls: Vec::new(),
        }
    }

    const PLACE: &str = "console:s1";

    #[test]
    fn a_quiet_place_is_asked_about_once_and_again_only_when_a_look_may_have_ended() {
        let mut f = Following::default();
        let t = Instant::now();
        assert_eq!(
            f.place(PLACE, t, false, None),
            Some(Read::Looks {
                place: PLACE.into(),
                after: None
            }),
            "once when first shown, though nothing shows a look yet"
        );
        assert_eq!(f.place(PLACE, t, false, Some(40)), None, "one at a time");
        f.looks(PLACE, t, Ok(vec![look(7, Some(40))]));
        let later = t + Duration::from_secs(5);
        assert_eq!(f.place(PLACE, later, false, Some(40)), None);
        assert_eq!(
            f.place(PLACE, later, false, Some(52)),
            Some(Read::Looks {
                place: PLACE.into(),
                after: Some(40)
            }),
            "a new entry may be a look that ended"
        );
    }

    #[test]
    fn while_she_thinks_a_place_is_asked_about_every_second_until_nothing_runs() {
        let mut f = Following::default();
        let t = Instant::now();
        assert!(f.place(PLACE, t, true, None).is_some());
        f.looks(PLACE, t, Ok(vec![look(3, None)]));
        let soon = t + Duration::from_millis(400);
        assert_eq!(f.place(PLACE, soon, false, None), None, "not yet a second");
        let second = t + LOOK_EVERY;
        assert!(
            f.place(PLACE, second, false, None).is_some(),
            "a look still runs, though nothing waits on her"
        );
        f.looks(PLACE, second, Ok(vec![look(3, Some(60))]));
        let looks = &f.places[PLACE];
        assert!(looks.running().is_none());
        assert_eq!(looks.ended_with(60).map(|l| l.look), Some(3));
        let after = second + Duration::from_secs(3);
        assert_eq!(f.place(PLACE, after, false, Some(60)), None, "nothing runs");
    }

    #[test]
    fn a_look_given_up_on_and_tried_again_stops_being_followed() {
        let mut f = Following::default();
        let t = Instant::now();
        f.place(PLACE, t, true, None);
        f.looks(PLACE, t, Ok(vec![look(3, None)]));
        f.place(PLACE, t + LOOK_EVERY, true, None);
        f.looks(PLACE, t + LOOK_EVERY, Ok(Vec::new()));
        assert!(f.places[PLACE].looks.is_empty());
    }

    #[test]
    fn a_refused_or_lost_ask_is_not_repeated_at_once() {
        let mut f = Following::default();
        let t = Instant::now();
        f.place(PLACE, t, true, None);
        f.looks(PLACE, t, Err("method not found".into()));
        assert_eq!(f.place(PLACE, t + LOOK_EVERY, true, None), None);
        assert!(f.place(PLACE, t + QUIET, true, None).is_some());

        let other = "qq:9";
        f.place(other, t, true, None);
        assert_eq!(
            f.place(other, t + LOOK_EVERY, true, None),
            None,
            "in flight"
        );
        assert!(
            f.place(other, t + LOST, true, None).is_some(),
            "taken as lost"
        );
        f.lost();
        assert!(f.place(other, t + LOST, true, None).is_some());
    }

    #[test]
    fn open_work_is_asked_about_every_two_seconds_and_once_more_when_it_ends() {
        let mut f = Following::default();
        let t = Instant::now();
        let ask = |after| {
            Some(Read::Steps {
                work: "w1".into(),
                after,
            })
        };
        assert_eq!(f.work("w1", t, true), ask(None));
        let log = WorkLog {
            steps: vec![step(1), step(2)],
            parts: vec![WorkPart {
                id: "w1/1".into(),
                brief: "images".into(),
                state: WorkState::Running,
                brain: None,
            }],
            more: false,
            context: None,
        };
        f.steps("w1", t, Ok(log));
        assert_eq!(f.work("w1", t + Duration::from_secs(1), true), None);
        assert_eq!(f.work("w1", t + WORK_EVERY, true), ask(Some(2)));
        f.steps(
            "w1",
            t + WORK_EVERY,
            Ok(WorkLog {
                steps: vec![step(2), step(3)],
                parts: Vec::new(),
                more: false,
                context: None,
            }),
        );
        let held: Vec<u64> = f.works["w1"].steps.iter().map(|s| s.n).collect();
        assert_eq!(held, [1, 2, 3], "a step already held is not held twice");
        let end = t + Duration::from_secs(3);
        assert_eq!(f.work("w1", end, false), ask(Some(3)), "its last steps");
        f.steps(
            "w1",
            end,
            Ok(WorkLog {
                steps: Vec::new(),
                parts: Vec::new(),
                more: false,
                context: None,
            }),
        );
        assert_eq!(f.work("w1", end + Duration::from_secs(60), false), None);
    }

    #[test]
    fn work_with_more_steps_than_one_answer_is_read_on_at_once() {
        let mut f = Following::default();
        let t = Instant::now();
        f.work("w2", t, false);
        f.steps(
            "w2",
            t,
            Ok(WorkLog {
                steps: (1..=100).map(step).collect(),
                parts: Vec::new(),
                more: true,
                context: None,
            }),
        );
        assert_eq!(
            f.work("w2", t, false),
            Some(Read::Steps {
                work: "w2".into(),
                after: Some(100)
            })
        );
    }

    fn context(n: u64) -> WorkContext {
        WorkContext {
            n,
            at: None,
            usage: None,
            parts: BTreeMap::new(),
            window: None,
            folds_at: None,
        }
    }

    #[test]
    fn the_context_of_open_work_is_asked_about_every_two_seconds_and_only_the_newest_is_kept() {
        let mut f = Following::default();
        let t = Instant::now();
        let ask = Some(Read::Context { work: "w3".into() });
        assert_eq!(f.context("w3", t, true), ask);
        assert_eq!(f.context("w3", t, true), None, "one at a time");
        f.took_context("w3", t, Ok(Some(context(4))));
        assert_eq!(f.context("w3", t + Duration::from_secs(1), true), None);
        assert_eq!(f.context("w3", t + WORK_EVERY, true), ask);
        // Its card's steps came in meanwhile with a newer one; the answer
        // asked before them is older and does not take its place.
        f.steps(
            "w3",
            t + WORK_EVERY,
            Ok(WorkLog {
                steps: Vec::new(),
                parts: Vec::new(),
                more: false,
                context: Some(context(9)),
            }),
        );
        f.took_context("w3", t + WORK_EVERY, Ok(Some(context(8))));
        assert_eq!(f.work_context("w3").map(|c| c.n), Some(9));
        let end = t + Duration::from_secs(5);
        assert_eq!(f.context("w3", end, false), ask, "once more when it ends");
        f.took_context("w3", end, Ok(None));
        assert_eq!(f.work_context("w3").map(|c| c.n), Some(9), "kept");
        assert_eq!(f.context("w3", end + Duration::from_secs(60), false), None);
    }
}
