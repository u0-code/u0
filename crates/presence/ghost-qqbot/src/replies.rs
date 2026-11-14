//! What a QQ bot may say, and when.
//!
//! A bot on QQ mostly answers. A message sent with the id of one it received
//! is a passive reply, and is allowed a few times per received message,
//! within minutes of it. A message sent with no such id is proactive, and
//! QQ allows a bot very few of those. So each place keeps the messages that
//! arrived there recently and how often each has been answered, and every
//! part she sends is charged to one of them while one is left.

use std::{
    collections::{HashMap, VecDeque},
    time::{Duration, Instant},
};

/// Passive replies allowed per received message.
pub const PER_MESSAGE: u32 = 4;
/// How long after a message in a group it can still be answered.
pub const GROUP_WINDOW: Duration = Duration::from_secs(5 * 60);
/// How long after a private message it can still be answered.
pub const DIRECT_WINDOW: Duration = Duration::from_secs(30 * 60);
/// Recent messages kept per place. More than could ever still be answered.
const KEPT: usize = 32;

/// A received message to answer: its id, and the sequence number this
/// answer carries. QQ refuses two answers to one message with one number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Passive {
    pub msg_id: String,
    pub seq: u32,
}

#[derive(Debug, Default)]
pub struct Replies {
    places: HashMap<String, VecDeque<Received>>,
}

#[derive(Debug)]
struct Received {
    id: String,
    at: Instant,
    answered: u32,
}

fn window(place: &str) -> Duration {
    if place.starts_with("group:") {
        GROUP_WINDOW
    } else {
        DIRECT_WINDOW
    }
}

impl Replies {
    /// A message arrived in `place`.
    pub fn received(&mut self, place: &str, id: &str, now: Instant) {
        let recent = self.places.entry(place.to_owned()).or_default();
        recent.push_back(Received {
            id: id.to_owned(),
            at: now,
            answered: 0,
        });
        while recent.len() > KEPT {
            recent.pop_front();
        }
    }

    /// The message one more part sent to `place` answers, charging it: the
    /// one she replied to if it can still be answered, else the newest that
    /// can. `None` means it would be proactive.
    pub fn answer(&mut self, place: &str, reply_to: Option<&str>, now: Instant) -> Option<Passive> {
        let within = window(place);
        let recent = self.places.get_mut(place)?;
        recent.retain(|r| now.duration_since(r.at) < within);
        let open = |r: &Received| r.answered < PER_MESSAGE;
        let index = reply_to
            .and_then(|wanted| recent.iter().position(|r| r.id == wanted && open(r)))
            .or_else(|| recent.iter().rposition(open))?;
        let chosen = &mut recent[index];
        chosen.answered += 1;
        Some(Passive {
            msg_id: chosen.id.clone(),
            seq: chosen.answered,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn passive(id: &str, seq: u32) -> Passive {
        Passive {
            msg_id: id.to_owned(),
            seq,
        }
    }

    #[test]
    fn each_part_answers_the_newest_message_with_its_own_number_until_the_allowance_is_spent() {
        let start = Instant::now();
        let mut replies = Replies::default();
        replies.received("c2c:U1", "m-1", start);
        replies.received("c2c:U1", "m-2", start);
        let parts: Vec<_> = (0..5)
            .map(|_| replies.answer("c2c:U1", None, start))
            .collect();
        assert_eq!(
            parts,
            [
                Some(passive("m-2", 1)),
                Some(passive("m-2", 2)),
                Some(passive("m-2", 3)),
                Some(passive("m-2", 4)),
                Some(passive("m-1", 1)),
            ]
        );
        // Nothing arrived somewhere else: speaking there would be proactive.
        assert_eq!(replies.answer("c2c:U2", None, start), None);
    }

    #[test]
    fn the_message_she_replies_to_is_answered_while_it_can_be() {
        let start = Instant::now();
        let mut replies = Replies::default();
        replies.received("group:G1", "m-1", start);
        replies.received("group:G1", "m-2", start);
        assert_eq!(
            replies.answer("group:G1", Some("m-1"), start),
            Some(passive("m-1", 1))
        );
        assert_eq!(
            replies.answer("group:G1", Some("gone"), start),
            Some(passive("m-2", 1))
        );
    }

    #[test]
    fn a_group_message_can_be_answered_for_minutes_and_a_private_one_for_longer() {
        let start = Instant::now();
        let mut replies = Replies::default();
        replies.received("group:G1", "g", start);
        replies.received("c2c:U1", "c", start);
        let later = start + Duration::from_secs(6 * 60);
        assert_eq!(replies.answer("group:G1", None, later), None);
        assert_eq!(replies.answer("c2c:U1", None, later), Some(passive("c", 1)));
        let much_later = start + Duration::from_secs(31 * 60);
        assert_eq!(replies.answer("c2c:U1", None, much_later), None);
    }
}
