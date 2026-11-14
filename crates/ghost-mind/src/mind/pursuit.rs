//! What she is after, and what doing things has taught her.
//!
//! A pursuit is a direction of her own over months, in her words. Nothing
//! here gives her one: she writes it down when she finds she has it, looking
//! back before sleep, and from then on may take a piece of work on toward
//! it without anyone asking. What each piece of work taught her about how
//! to work is kept the same way, and is in front of her the next time.

use chrono::{DateTime, Duration, Utc};
use ghost_core::journal::Event;
use tracing::debug;

use super::{Mind, public::REVIEW};
use crate::{Result, actions::clip};

/// How many directions she holds at once.
const HELD: usize = 3;
/// How many of the things work has taught her are put in front of her.
const SHOWN: usize = 12;

impl Mind {
    /// What she is after, for her to look at before sleep.
    pub(super) fn pursuit_block(&self) -> Result<String> {
        let held = self.ghost.memory().pursuits(HELD)?;
        let lines = if held.is_empty() {
            self.prompts.text("pursuits_none").to_owned()
        } else {
            held.iter()
                .map(|p| format!("- [{}] {}", p.seq, p.lesson))
                .collect::<Vec<_>>()
                .join("\n")
        };
        Ok(self.prompts.fill("pursuits", &[("pursuits", lines)]))
    }

    /// Whether she may take something on tonight for herself: she is after
    /// something, has the means, has nothing of her own in hand, and has
    /// not started something of her own in the last few days.
    fn may_start_own_work(&self, now: DateTime<Utc>) -> Result<bool> {
        let days = self.cfg.work.pursue_days;
        if days == 0 || !self.can_work() {
            return Ok(false);
        }
        let memory = self.ghost.memory();
        if memory.pursuits(1)?.is_empty() {
            return Ok(false);
        }
        let since = now - Duration::days(i64::from(days));
        Ok(!memory.undertakings(false, 40)?.iter().any(|u| {
            let own = u.asked_by.is_none() && u.place.is_none() && !u.id.starts_with(REVIEW);
            own && (u.outcome.is_none() || u.at >= since)
        }))
    }

    /// How she is told about taking something on for herself.
    pub(super) fn own_work_line(&self, now: DateTime<Utc>) -> Result<String> {
        let key = if self.may_start_own_work(now)? {
            "own_work_yes"
        } else {
            "own_work_no"
        };
        Ok(self.prompts.text(key).to_owned())
    }

    /// She changed what she is after: a new direction, a changed one, or
    /// one let go.
    pub(super) fn on_pursuits(&mut self, changes: Vec<(String, Option<i64>)>) -> Result<()> {
        let mut held: Vec<i64> = self
            .ghost
            .memory()
            .pursuits(HELD * 4)?
            .iter()
            .map(|p| p.seq)
            .collect();
        for (what, replaces) in changes.into_iter().take(2) {
            let what = what.trim().to_owned();
            let replaces = replaces.filter(|seq| held.contains(seq));
            match (what.is_empty(), replaces) {
                // Nothing said, about nothing.
                (true, None) => continue,
                // One more than she holds.
                (false, None) if held.len() >= HELD => {
                    debug!("already after {HELD} things; not adding: {what}");
                    continue;
                }
                _ => {}
            }
            if let Some(old) = replaces {
                held.retain(|seq| *seq != old);
            }
            let entry = self.record(Event::Pursued {
                what: what.clone(),
                replaces,
            })?;
            if !what.is_empty() {
                held.push(entry.seq);
            }
        }
        Ok(())
    }

    /// She decided, before sleep, to do something of her own tomorrow.
    pub(super) fn start_own_work(&mut self, brief: &str, now: DateTime<Utc>) -> Result<()> {
        if brief.trim().is_empty() || !self.may_start_own_work(now)? {
            return Ok(());
        }
        if let Some(why) = self.undertake(brief, None, None, &[], now)? {
            debug!("did not take on something of her own: {why}");
        }
        Ok(())
    }

    /// What work has taught her about working, for the top of the next
    /// piece of it.
    pub(super) fn practice_block(&self) -> Result<String> {
        let held = self.ghost.memory().practice(SHOWN)?;
        let lessons = held
            .iter()
            .map(|l| format!("- {}", l.lesson))
            .collect::<Vec<_>>()
            .join("\n");
        Ok(self.prompts.section("practice", "lessons", &lessons))
    }

    /// A piece of work ended and left something worth keeping about how
    /// to work.
    pub(super) fn on_practice(&mut self, lesson: &str) -> Result<()> {
        let lesson = lesson.trim();
        if lesson.is_empty() {
            return Ok(());
        }
        self.record(Event::Practised {
            lesson: clip(lesson, 300),
            replaces: None,
        })?;
        Ok(())
    }
}
