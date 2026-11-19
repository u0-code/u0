//! Where she stands with someone, and what that permits.
//!
//! Standing is derived, never set: it follows from how long she has known a
//! person, how often they have actually talked, and the sum of every shift in
//! feeling she has recorded. Nothing here calls a model.

use chrono::{DateTime, Duration, Utc};
use serde::Serialize;

use crate::memory::Person;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    /// She left. Terminal.
    Gone,
    Estranged,
    Cooling,
    Stranger,
    Acquaintance,
    Friend,
    Close,
}

impl Stage {
    /// Multiplier on how long she takes to notice their messages.
    pub fn patience(self) -> f32 {
        match self {
            Self::Close => 0.6,
            Self::Friend => 0.8,
            Self::Acquaintance => 1.0,
            Self::Stranger => 1.3,
            Self::Cooling => 2.5,
            Self::Estranged | Self::Gone => 6.0,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Standing {
    pub stage: Stage,
    pub warmth: i64,
    pub known_days: i64,
    pub active_days: i64,
}

/// Where she stands with `person` at `now`.
pub fn standing(person: &Person, now: DateTime<Utc>) -> Standing {
    let known_days = (now - person.first_at).num_days().max(0);
    let stage = if person.gone {
        Stage::Gone
    } else if person.warmth <= -6 {
        Stage::Estranged
    } else if person.warmth <= -3 {
        Stage::Cooling
    } else if person.active_days >= 14 && person.warmth >= 8 {
        Stage::Close
    } else if person.active_days >= 4 && person.warmth >= 3 {
        Stage::Friend
    } else if person.active_days >= 2 || person.heard >= 10 {
        Stage::Acquaintance
    } else {
        Stage::Stranger
    };
    Standing {
        stage,
        warmth: person.warmth,
        known_days,
        active_days: person.active_days,
    }
}

/// Leaving is the end of a process, not a switch: it only takes effect once
/// she has already cooled toward someone.
pub fn may_leave(standing: &Standing) -> bool {
    matches!(standing.stage, Stage::Cooling | Stage::Estranged)
}

/// Whether she may write first. She does not chase: once she has written on
/// two separate days without an answer, or already today, she waits.
pub fn may_reach_out(person: &Person, standing: &Standing, now: DateTime<Utc>) -> bool {
    if person.direct.is_none() || person.automated {
        return false;
    }
    if matches!(
        standing.stage,
        Stage::Gone | Stage::Estranged | Stage::Cooling
    ) {
        return false;
    }
    if person.unanswered_days >= 2 {
        return false;
    }
    let wrote_recently = person.unanswered_days > 0
        && person
            .last_out_at
            .is_some_and(|at| now - at < Duration::hours(20));
    !wrote_recently
}

#[cfg(test)]
mod tests {
    use super::*;

    fn person(active_days: i64, heard: i64, warmth: i64) -> Person {
        Person {
            key: "t:a".into(),
            channel: "t".into(),
            id: "a".into(),
            name: "A".into(),
            handle: None,
            automated: false,
            first_at: Utc::now() - Duration::days(30),
            last_at: Utc::now(),
            heard,
            warmth,
            gone: false,
            direct: Some("t:a".into()),
            active_days,
            unanswered_days: 0,
            last_out_at: None,
        }
    }

    #[test]
    fn stages_follow_time_and_warmth() {
        let now = Utc::now();
        assert_eq!(standing(&person(1, 3, 0), now).stage, Stage::Stranger);
        assert_eq!(standing(&person(2, 3, 0), now).stage, Stage::Acquaintance);
        assert_eq!(standing(&person(5, 50, 4), now).stage, Stage::Friend);
        assert_eq!(standing(&person(20, 500, 9), now).stage, Stage::Close);
        // Time alone does not make a friend.
        assert_eq!(
            standing(&person(20, 500, 0), now).stage,
            Stage::Acquaintance
        );
        assert_eq!(standing(&person(20, 500, -4), now).stage, Stage::Cooling);
        assert_eq!(standing(&person(20, 500, -9), now).stage, Stage::Estranged);
        let mut gone = person(20, 500, 9);
        gone.gone = true;
        assert_eq!(standing(&gone, now).stage, Stage::Gone);
    }

    #[test]
    fn leaving_needs_a_history() {
        let now = Utc::now();
        assert!(!may_leave(&standing(&person(5, 50, 4), now)));
        assert!(may_leave(&standing(&person(5, 50, -4), now)));
    }

    #[test]
    fn she_does_not_chase() {
        let now = Utc::now();
        let mut p = person(5, 50, 4);
        let s = standing(&p, now);
        assert!(may_reach_out(&p, &s, now));
        p.unanswered_days = 1;
        p.last_out_at = Some(now - Duration::hours(2));
        assert!(!may_reach_out(&p, &s, now));
        p.last_out_at = Some(now - Duration::hours(30));
        assert!(may_reach_out(&p, &s, now));
        p.unanswered_days = 2;
        assert!(!may_reach_out(&p, &s, now));
        let mut stranger_without_dm = person(1, 1, 0);
        stranger_without_dm.direct = None;
        let s = standing(&stranger_without_dm, now);
        assert!(!may_reach_out(&stranger_without_dm, &s, now));
    }
}
