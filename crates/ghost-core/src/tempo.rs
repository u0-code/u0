//! How long things take her.
//!
//! A person does not answer the instant a message lands. She notices it after
//! a while that depends on what she is doing, reads it, types at a human
//! speed, and pauses between messages. Everything here is a pure function of
//! its inputs and a random source, so it can be tested and tuned.

use std::time::Duration;

use fastrand::Rng;

use crate::life::{Phase, Reach};

/// The pace she lives at. `1.0` is human; `0.0` removes every wait.
#[derive(Debug, Clone, Copy)]
pub struct Tempo {
    scale: f32,
}

/// What determines how soon she notices a message.
#[derive(Debug, Clone)]
pub struct Notice {
    pub phase: Phase,
    pub reach: Reach,
    /// How long until her current state ends (she wakes, the block is over).
    pub until: Duration,
    /// She said something in this place moments ago.
    pub engaged: bool,
    /// The message was meant for her.
    pub addressed: bool,
    /// Multiplier from how she stands with the sender. Below 1 is quicker.
    pub patience: f32,
}

impl Tempo {
    pub fn new(scale: f32) -> Self {
        Self {
            scale: if scale.is_finite() {
                scale.max(0.0)
            } else {
                1.0
            },
        }
    }

    pub fn scale(&self) -> f32 {
        self.scale
    }

    /// True when every wait has been switched off, for development.
    pub fn instant(&self) -> bool {
        self.scale == 0.0
    }

    /// How long until she looks at a place with something new in it.
    pub fn notice(&self, n: &Notice, rng: &mut Rng) -> Duration {
        if self.instant() {
            return Duration::ZERO;
        }
        if n.phase == Phase::Asleep {
            // Not until she wakes, and not the first thing she does.
            return n.until + self.jitter(rng, 180.0, 2400.0, 1.0);
        }
        if n.engaged {
            let (lo, hi) = if n.addressed { (2.0, 7.0) } else { (6.0, 40.0) };
            return self.jitter(rng, lo, hi, n.patience.min(1.5));
        }
        let (lo, hi) = match (n.reach, n.addressed) {
            (Reach::Free, true) => (8.0, 90.0),
            (Reach::Free, false) => (120.0, 900.0),
            (Reach::Glance, true) => (60.0, 600.0),
            (Reach::Glance, false) => (600.0, 2400.0),
            (Reach::Busy, true) => (600.0, 2400.0),
            (Reach::Busy, false) => (1800.0, 5400.0),
            (Reach::Away, _) => (60.0, 600.0),
        };
        let wait = self.jitter(rng, lo, hi, n.patience);
        match n.reach {
            // Not looking until this is over.
            Reach::Away => n.until + wait,
            // Looks rarely; at the latest shortly after she is done.
            Reach::Busy => wait.min(n.until + self.jitter(rng, 30.0, 300.0, 1.0)),
            Reach::Free | Reach::Glance => wait,
        }
    }

    /// How long to let a burst of incoming messages settle before looking.
    pub fn settle(&self) -> Duration {
        self.scaled(3.5).max(Duration::from_millis(150))
    }

    /// How long reading `chars` characters takes.
    pub fn read(&self, chars: usize) -> Duration {
        self.scaled((0.6 + as_f32(chars) / 28.0).min(12.0))
    }

    /// How long typing `text` takes, including a moment of thought.
    pub fn compose(&self, text: &str, rng: &mut Rng) -> Duration {
        let (wide, narrow) = text.chars().fold((0usize, 0usize), |(w, n), c| {
            if is_wide(c) { (w + 1, n) } else { (w, n + 1) }
        });
        let typing = as_f32(wide) / 2.4 + as_f32(narrow) / 5.0;
        let thinking = 0.6 + rng.f32() * 1.8;
        let wobble = 0.75 + rng.f32() * 0.5;
        self.scaled(((typing + thinking) * wobble).min(28.0))
    }

    /// The pause between two of her messages.
    pub fn between(&self, rng: &mut Rng) -> Duration {
        self.scaled(0.4 + rng.f32() * 1.2)
    }

    fn jitter(&self, rng: &mut Rng, lo: f32, hi: f32, factor: f32) -> Duration {
        // Skewed toward the short end: most looks are quick, a few are not.
        let u = rng.f32();
        self.scaled((lo + (hi - lo) * u * u) * factor.max(0.1))
    }

    fn scaled(&self, seconds: f32) -> Duration {
        Duration::from_secs_f32((seconds * self.scale).max(0.0))
    }
}

/// CJK and other scripts typed through an input method.
fn is_wide(c: char) -> bool {
    matches!(u32::from(c), 0x2E80..=0x9FFF | 0xAC00..=0xD7AF | 0xF900..=0xFAFF | 0xFF00..=0xFFEF)
}

#[allow(clippy::cast_precision_loss)]
fn as_f32(n: usize) -> f32 {
    n as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notice(phase: Phase, reach: Reach, until: u64, engaged: bool, addressed: bool) -> Notice {
        Notice {
            phase,
            reach,
            until: Duration::from_secs(until),
            engaged,
            addressed,
            patience: 1.0,
        }
    }

    #[test]
    fn zero_scale_removes_every_wait() {
        let t = Tempo::new(0.0);
        let mut rng = Rng::with_seed(1);
        assert_eq!(
            t.notice(
                &notice(Phase::Asleep, Reach::Away, 9000, false, true),
                &mut rng
            ),
            Duration::ZERO
        );
        assert_eq!(t.compose("hello there", &mut rng), Duration::ZERO);
        assert_eq!(t.read(500), Duration::ZERO);
    }

    #[test]
    fn asleep_waits_past_waking() {
        let t = Tempo::new(1.0);
        let mut rng = Rng::with_seed(7);
        for _ in 0..50 {
            let d = t.notice(
                &notice(Phase::Asleep, Reach::Away, 3600, false, true),
                &mut rng,
            );
            assert!(d >= Duration::from_secs(3600 + 180));
        }
    }

    #[test]
    fn engaged_is_quick_and_busy_is_bounded_by_the_block() {
        let t = Tempo::new(1.0);
        let mut rng = Rng::with_seed(3);
        for _ in 0..50 {
            let quick = t.notice(
                &notice(Phase::Awake, Reach::Free, 9999, true, true),
                &mut rng,
            );
            assert!(quick <= Duration::from_secs(8));
            let busy = t.notice(
                &notice(Phase::Awake, Reach::Busy, 120, false, true),
                &mut rng,
            );
            assert!(busy <= Duration::from_secs(120 + 300));
            let away = t.notice(
                &notice(Phase::Awake, Reach::Away, 500, false, true),
                &mut rng,
            );
            assert!(away >= Duration::from_secs(500 + 60));
        }
    }

    #[test]
    fn patience_stretches_the_wait() {
        let t = Tempo::new(1.0);
        let mut a = Rng::with_seed(11);
        let mut b = Rng::with_seed(11);
        let mut n = notice(Phase::Awake, Reach::Free, 9999, false, true);
        let warm = t.notice(&n, &mut a);
        n.patience = 3.0;
        let cold = t.notice(&n, &mut b);
        assert!(cold > warm * 2);
    }

    #[test]
    fn typing_chinese_is_slower_per_character() {
        let t = Tempo::new(1.0);
        let zh = t.compose("我今天去了东边那家老茶馆坐了一下午", &mut Rng::with_seed(5));
        let en = t.compose("i went to the old tea", &mut Rng::with_seed(5));
        assert!(zh > en);
        assert!(zh <= Duration::from_secs(28));
    }
}
