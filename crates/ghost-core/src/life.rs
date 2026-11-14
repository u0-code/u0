//! Her clock, and what a day is made of.
//!
//! This module is the pure half of the life engine: given her rhythm, her
//! journal, and a moment in time, it says whether she is awake, what she is in
//! the middle of, and how reachable that leaves her. Deciding what a day
//! contains takes a brain and lives in `ghost-mind`; the result comes back
//! here as a [`Event::Planned`] entry.

use chrono::{DateTime, Duration, LocalResult, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};

use crate::{
    Result,
    ghost::Ghost,
    identity::Identity,
    journal::{Entry, Event, Journal},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Asleep,
    Awake,
}

/// How reachable an activity leaves her.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reach {
    /// Phone in hand, nothing pressing.
    Free,
    /// Doing something, but checks now and then.
    Glance,
    /// Occupied. Looks rarely.
    Busy,
    /// Not looking until this is over.
    Away,
}

/// One stretch of a planned day. Times are local `HH:MM`; a time earlier than
/// her wake time belongs to the small hours of the following date.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Block {
    pub from: String,
    pub to: String,
    pub what: String,
    pub reach: Reach,
}

/// A block with its times resolved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Doing {
    /// Position in the day's plan.
    pub index: u32,
    pub what: String,
    pub reach: Reach,
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
}

/// A planned day, resolved against her clock.
#[derive(Debug, Clone, Serialize)]
pub struct Day {
    /// Seq of the `Planned` entry.
    pub seq: i64,
    pub day: NaiveDate,
    pub weather: Option<String>,
    pub blocks: Vec<Doing>,
}

/// Where she is in her day at one instant.
#[derive(Debug, Clone, Serialize)]
pub struct Moment {
    pub phase: Phase,
    pub doing: Option<Doing>,
    pub reach: Reach,
    /// When this state ends: she wakes, the block finishes, or the next one starts.
    pub until: DateTime<Utc>,
}

/// Her rhythm, resolved into concrete times.
#[derive(Debug, Clone)]
pub struct Clock {
    tz: Tz,
    wake: NaiveTime,
    sleep: NaiveTime,
}

impl Clock {
    /// Resolve her rhythm.
    ///
    /// # Panics
    /// Never in practice: the times were validated when the persona was parsed.
    pub fn of(identity: &Identity) -> Self {
        let seed = identity.seed();
        Self {
            tz: identity.tz(),
            wake: seed.rhythm.wake_time().expect("validated at parse"),
            sleep: seed.rhythm.sleep_time().expect("validated at parse"),
        }
    }

    /// A rhythm given directly, for when there is no persona to read it from.
    pub fn new(tz: Tz, wake: NaiveTime, sleep: NaiveTime) -> Self {
        Self { tz, wake, sleep }
    }

    pub fn tz(&self) -> Tz {
        self.tz
    }

    pub fn wake(&self) -> NaiveTime {
        self.wake
    }

    pub fn sleep(&self) -> NaiveTime {
        self.sleep
    }

    /// Whether her rhythm says she is awake at `now`.
    pub fn phase_at(&self, now: DateTime<Utc>) -> Phase {
        let t = now.with_timezone(&self.tz).time();
        let awake = if self.wake < self.sleep {
            // Sleeps before midnight: awake inside [wake, sleep).
            t >= self.wake && t < self.sleep
        } else {
            // Sleeps after midnight: awake outside [sleep, wake).
            t >= self.wake || t < self.sleep
        };
        if awake { Phase::Awake } else { Phase::Asleep }
    }

    /// The waking day `now` belongs to: the local date she last woke on, or
    /// will wake on if it is still before her wake time.
    pub fn day_of(&self, now: DateTime<Utc>) -> NaiveDate {
        let local = now.with_timezone(&self.tz);
        let date = local.date_naive();
        if self.sleep < self.wake && local.time() < self.sleep {
            date.pred_opt().unwrap_or(date)
        } else {
            date
        }
    }

    /// The instant a local time falls on within waking day `day`.
    pub fn instant(&self, day: NaiveDate, t: NaiveTime) -> DateTime<Utc> {
        let date = if t < self.wake {
            day.succ_opt().unwrap_or(day)
        } else {
            day
        };
        self.resolve(date.and_time(t))
    }

    /// The next time she wakes, strictly after `now`.
    pub fn next_wake(&self, now: DateTime<Utc>) -> DateTime<Utc> {
        self.next_at(now, self.wake)
    }

    /// The next time she goes to sleep, strictly after `now`.
    pub fn next_sleep(&self, now: DateTime<Utc>) -> DateTime<Utc> {
        self.next_at(now, self.sleep)
    }

    /// The next time her phase flips.
    pub fn next_turn(&self, now: DateTime<Utc>) -> DateTime<Utc> {
        match self.phase_at(now) {
            Phase::Awake => self.next_sleep(now),
            Phase::Asleep => self.next_wake(now),
        }
    }

    fn next_at(&self, now: DateTime<Utc>, t: NaiveTime) -> DateTime<Utc> {
        let date = now.with_timezone(&self.tz).date_naive();
        let today = self.resolve(date.and_time(t));
        if today > now {
            today
        } else {
            self.resolve(date.succ_opt().unwrap_or(date).and_time(t))
        }
    }

    fn resolve(&self, naive: NaiveDateTime) -> DateTime<Utc> {
        match self.tz.from_local_datetime(&naive) {
            LocalResult::Single(d) | LocalResult::Ambiguous(d, _) => d.with_timezone(&Utc),
            // A local time skipped by a clock change: an hour later is close enough.
            LocalResult::None => self
                .tz
                .from_local_datetime(&(naive + Duration::hours(1)))
                .earliest()
                .map_or_else(|| Utc.from_utc_datetime(&naive), |d| d.with_timezone(&Utc)),
        }
    }

    /// Resolve a plan's blocks, dropping any that do not parse or run backwards.
    pub fn resolve_blocks(&self, day: NaiveDate, blocks: &[Block]) -> Vec<Doing> {
        blocks
            .iter()
            .enumerate()
            .filter_map(|(i, b)| {
                let from = NaiveTime::parse_from_str(&b.from, "%H:%M").ok()?;
                let to = NaiveTime::parse_from_str(&b.to, "%H:%M").ok()?;
                let from = self.instant(day, from);
                let to = self.instant(day, to);
                (to > from).then(|| Doing {
                    index: u32::try_from(i).unwrap_or(u32::MAX),
                    what: b.what.clone(),
                    reach: b.reach,
                    from,
                    to,
                })
            })
            .collect()
    }
}

/// The pure life engine for one ghost.
pub struct Life {
    clock: Clock,
}

impl Life {
    pub fn new(identity: &Identity) -> Self {
        Self {
            clock: Clock::of(identity),
        }
    }

    pub fn clock(&self) -> &Clock {
        &self.clock
    }

    /// The phase her journal last recorded, if any.
    pub fn recorded_phase(journal: &Journal) -> Result<Option<Phase>> {
        Ok(journal.last_of(&["woke", "slept"])?.map(|e| match e.event {
            Event::Woke => Phase::Awake,
            _ => Phase::Asleep,
        }))
    }

    /// Advance her clock to `now`. Records `Woke` or `Slept` when the phase
    /// has changed since the journal last recorded one.
    pub fn tick(&self, ghost: &mut Ghost, now: DateTime<Utc>) -> Result<Option<Entry>> {
        let should = self.clock.phase_at(now);
        if Self::recorded_phase(ghost.journal())? == Some(should) {
            return Ok(None);
        }
        let event = match should {
            Phase::Awake => Event::Woke,
            Phase::Asleep => Event::Slept,
        };
        ghost.record_at(now, event).map(Some)
    }

    /// The plan for the waking day `now` belongs to, if she has made one.
    pub fn today(&self, journal: &Journal, now: DateTime<Utc>) -> Result<Option<Day>> {
        let Some(entry) = journal.last_of(&["planned"])? else {
            return Ok(None);
        };
        let Event::Planned {
            day,
            weather,
            blocks,
        } = entry.event
        else {
            return Ok(None);
        };
        let Ok(day) = NaiveDate::parse_from_str(&day, "%Y-%m-%d") else {
            return Ok(None);
        };
        if day != self.clock.day_of(now) {
            return Ok(None);
        }
        Ok(Some(Day {
            seq: entry.seq,
            day,
            weather,
            blocks: self.clock.resolve_blocks(day, &blocks),
        }))
    }

    /// Where she is in her day at `now`.
    pub fn moment(&self, journal: &Journal, now: DateTime<Utc>) -> Result<Moment> {
        if self.clock.phase_at(now) == Phase::Asleep {
            return Ok(Moment {
                phase: Phase::Asleep,
                doing: None,
                reach: Reach::Away,
                until: self.clock.next_wake(now),
            });
        }
        let bedtime = self.clock.next_sleep(now);
        let Some(day) = self.today(journal, now)? else {
            return Ok(Moment {
                phase: Phase::Awake,
                doing: None,
                reach: Reach::Free,
                until: bedtime,
            });
        };
        if let Some(doing) = day.blocks.iter().find(|b| b.from <= now && now < b.to) {
            return Ok(Moment {
                phase: Phase::Awake,
                reach: doing.reach,
                until: doing.to.min(bedtime),
                doing: Some(doing.clone()),
            });
        }
        let next_start = day
            .blocks
            .iter()
            .map(|b| b.from)
            .filter(|from| *from > now)
            .min()
            .unwrap_or(bedtime);
        Ok(Moment {
            phase: Phase::Awake,
            doing: None,
            reach: Reach::Free,
            until: next_start.min(bedtime),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clock(wake: &str, sleep: &str) -> Clock {
        Clock {
            tz: chrono_tz::Asia::Shanghai,
            wake: NaiveTime::parse_from_str(wake, "%H:%M").unwrap(),
            sleep: NaiveTime::parse_from_str(sleep, "%H:%M").unwrap(),
        }
    }

    fn local(d: u32, h: u32, m: u32) -> DateTime<Utc> {
        chrono_tz::Asia::Shanghai
            .with_ymd_and_hms(2026, 11, d, h, m, 0)
            .unwrap()
            .with_timezone(&Utc)
    }

    fn block(from: &str, to: &str, reach: Reach) -> Block {
        Block {
            from: from.into(),
            to: to.into(),
            what: "x".into(),
            reach,
        }
    }

    #[test]
    fn sleeps_after_midnight() {
        let c = clock("08:30", "00:30");
        assert_eq!(c.phase_at(local(12, 0, 10)), Phase::Awake);
        assert_eq!(c.phase_at(local(12, 0, 40)), Phase::Asleep);
        assert_eq!(c.phase_at(local(12, 8, 29)), Phase::Asleep);
        assert_eq!(c.phase_at(local(12, 8, 30)), Phase::Awake);
        assert_eq!(c.phase_at(local(12, 23, 59)), Phase::Awake);
    }

    #[test]
    fn sleeps_before_midnight() {
        let c = clock("07:00", "23:00");
        assert_eq!(c.phase_at(local(12, 6, 59)), Phase::Asleep);
        assert_eq!(c.phase_at(local(12, 12, 0)), Phase::Awake);
        assert_eq!(c.phase_at(local(12, 23, 0)), Phase::Asleep);
    }

    #[test]
    fn small_hours_belong_to_the_previous_day() {
        let c = clock("08:30", "00:30");
        let d12 = NaiveDate::from_ymd_opt(2026, 11, 12).unwrap();
        assert_eq!(c.day_of(local(12, 23, 0)), d12);
        assert_eq!(c.day_of(local(13, 0, 10)), d12);
        assert_eq!(c.day_of(local(13, 9, 0)), d12.succ_opt().unwrap());
        // A block ending at 00:20 on the 12th's plan lands on the 13th.
        let to = c.instant(d12, NaiveTime::from_hms_opt(0, 20, 0).unwrap());
        assert_eq!(to, local(13, 0, 20));
    }

    #[test]
    fn turns_are_strictly_ahead() {
        let c = clock("08:30", "00:30");
        assert_eq!(c.next_wake(local(12, 8, 30)), local(13, 8, 30));
        assert_eq!(c.next_wake(local(12, 3, 0)), local(12, 8, 30));
        assert_eq!(c.next_sleep(local(12, 14, 0)), local(13, 0, 30));
        assert_eq!(c.next_turn(local(12, 3, 0)), local(12, 8, 30));
    }

    #[test]
    fn bad_blocks_are_dropped() {
        let c = clock("08:30", "00:30");
        let d = NaiveDate::from_ymd_opt(2026, 11, 12).unwrap();
        let blocks = vec![
            block("09:00", "10:00", Reach::Glance),
            block("11:00", "10:00", Reach::Busy),
            block("lunch", "13:00", Reach::Free),
            block("23:30", "00:15", Reach::Free),
        ];
        let resolved = c.resolve_blocks(d, &blocks);
        assert_eq!(
            resolved.iter().map(|b| b.index).collect::<Vec<_>>(),
            vec![0, 3]
        );
        assert_eq!(resolved[1].to, local(13, 0, 15));
    }

    #[test]
    fn moment_follows_the_plan() {
        let life = Life {
            clock: clock("08:30", "00:30"),
        };
        let mut j = Journal::in_memory();
        // No plan: awake and free until bedtime.
        let m = life.moment(&j, local(12, 10, 0)).unwrap();
        assert_eq!((m.phase, m.reach), (Phase::Awake, Reach::Free));
        assert_eq!(m.until, local(13, 0, 30));

        j.append(Event::Planned {
            day: "2026-11-12".into(),
            weather: None,
            blocks: vec![
                block("09:00", "10:30", Reach::Busy),
                block("14:00", "15:00", Reach::Away),
            ],
        })
        .unwrap();
        let m = life.moment(&j, local(12, 9, 30)).unwrap();
        assert_eq!(m.reach, Reach::Busy);
        assert_eq!(m.until, local(12, 10, 30));
        // Between blocks: free until the next one starts.
        let m = life.moment(&j, local(12, 11, 0)).unwrap();
        assert_eq!(m.reach, Reach::Free);
        assert_eq!(m.until, local(12, 14, 0));
        // Asleep: away until she wakes.
        let m = life.moment(&j, local(13, 3, 0)).unwrap();
        assert_eq!((m.phase, m.reach), (Phase::Asleep, Reach::Away));
        assert_eq!(m.until, local(13, 8, 30));
        // Next day: yesterday's plan no longer applies.
        assert!(life.today(&j, local(13, 9, 0)).unwrap().is_none());
    }
}
