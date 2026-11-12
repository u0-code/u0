//! Her clock. v0: a fixed rhythm of waking and sleeping in her home timezone.
//!
//! The life engine does not wait to be asked. It is ticked by the daemon and
//! writes what happened into the journal. Later versions add day plans, events
//! from her city, her projects, and the nightly reflection; the shape stays the
//! same: a tick reads the journal, decides what time has done to her, and
//! appends.

use chrono::{DateTime, NaiveTime, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};

use crate::{
    Result,
    identity::SealedIdentity,
    journal::{Entry, Event, Journal},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Asleep,
    Awake,
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
    /// Never in practice: the times were validated when the certificate was parsed.
    pub fn of(identity: &SealedIdentity) -> Self {
        let seed = identity.seed();
        Self {
            tz: identity.tz(),
            wake: seed.rhythm.wake_time().expect("validated at parse"),
            sleep: seed.rhythm.sleep_time().expect("validated at parse"),
        }
    }

    pub fn tz(&self) -> Tz {
        self.tz
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
}

/// The life engine for one ghost.
pub struct Life {
    clock: Clock,
}

impl Life {
    pub fn new(identity: &SealedIdentity) -> Self {
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

    /// Advance her clock to `now`. Appends a `Woke` or `Slept` entry when the
    /// phase has changed since the journal last recorded one.
    pub fn tick(&self, journal: &mut Journal, now: DateTime<Utc>) -> Result<Option<Entry>> {
        let should = self.clock.phase_at(now);
        if Self::recorded_phase(journal)? == Some(should) {
            return Ok(None);
        }
        let event = match should {
            Phase::Awake => Event::Woke,
            Phase::Asleep => Event::Slept,
        };
        journal.append_at(now, event).map(Some)
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn clock(wake: &str, sleep: &str) -> Clock {
        Clock {
            tz: chrono_tz::Asia::Shanghai,
            wake: NaiveTime::parse_from_str(wake, "%H:%M").unwrap(),
            sleep: NaiveTime::parse_from_str(sleep, "%H:%M").unwrap(),
        }
    }

    fn local(h: u32, m: u32) -> DateTime<Utc> {
        chrono_tz::Asia::Shanghai
            .with_ymd_and_hms(2026, 11, 12, h, m, 0)
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn sleeps_after_midnight() {
        let c = clock("08:30", "00:30");
        assert_eq!(c.phase_at(local(0, 10)), Phase::Awake);
        assert_eq!(c.phase_at(local(0, 40)), Phase::Asleep);
        assert_eq!(c.phase_at(local(8, 29)), Phase::Asleep);
        assert_eq!(c.phase_at(local(8, 30)), Phase::Awake);
        assert_eq!(c.phase_at(local(23, 59)), Phase::Awake);
    }

    #[test]
    fn sleeps_before_midnight() {
        let c = clock("07:00", "23:00");
        assert_eq!(c.phase_at(local(6, 59)), Phase::Asleep);
        assert_eq!(c.phase_at(local(12, 0)), Phase::Awake);
        assert_eq!(c.phase_at(local(23, 0)), Phase::Asleep);
    }
}
