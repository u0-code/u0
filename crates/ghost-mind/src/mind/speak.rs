//! Saying things the way a person does: read, type, send, pause, repeat.
//!
//! Each place she is speaking in has one small state machine. Its one firm
//! rule: she never sends something written before she saw their latest
//! message. If they say more while she is typing, what she had is dropped and
//! she looks again.

use std::collections::VecDeque;

use chrono::{DateTime, Duration, Utc};
use ghost_core::{
    journal::Event,
    memory::PlaceState,
    social::{self, PlaceKind},
};
use tracing::warn;

use super::{Mind, Outcome, after, attend::Thinking};
use crate::{Act, Acted, Can, Result};

/// How often a typing indicator has to be renewed to stay visible.
const TYPING_LASTS: Duration = Duration::seconds(4);

#[derive(Debug, Clone)]
pub(super) struct Bubble {
    pub text: String,
    pub reply_to: Option<String>,
    /// A file to hand over with it: its path on her machine.
    pub file: Option<String>,
}

/// The largest file she hands over.
const LARGEST_FILE: u64 = 20 * 1024 * 1024;

/// The last part of a path: what a file is called.
fn file_name(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .map_or_else(|| path.to_owned(), |n| n.to_string_lossy().into_owned())
}

pub(super) struct Speaking {
    channel: String,
    place_id: String,
    /// Seq of the newest incoming message she had seen when she decided.
    upto: i64,
    bubbles: VecDeque<Bubble>,
    step: Step,
    /// In a public thread a message has to hang off another one, so each
    /// bubble after the first answers the one before it.
    chain: bool,
}

enum Step {
    /// Taking in what was written.
    Reading { until: DateTime<Utc> },
    /// Typing the bubble at the front. `nudge` is when the indicator is due.
    Typing {
        until: DateTime<Utc>,
        nudge: DateTime<Utc>,
    },
    /// Handed to the body; waiting to hear it was sent.
    Sending,
}

impl Speaking {
    /// When this needs attention next.
    pub(super) fn due(&self) -> Option<DateTime<Utc>> {
        match self.step {
            Step::Reading { until } => Some(until),
            Step::Typing { until, nudge } => Some(until.min(nudge)),
            Step::Sending => None,
        }
    }
}

impl Mind {
    pub(super) fn begin_speaking(
        &mut self,
        place: &PlaceState,
        job: &Thinking,
        bubbles: VecDeque<Bubble>,
        now: DateTime<Utc>,
    ) {
        let until = after(now, self.tempo.read(job.unread_chars));
        self.agenda.speaking.insert(
            place.key.clone(),
            Speaking {
                channel: place.channel.clone(),
                place_id: place.id.clone(),
                upto: job.upto,
                bubbles,
                step: Step::Reading { until },
                chain: place.kind == PlaceKind::Public,
            },
        );
    }

    /// Start speaking in a place outside of a look at it, as when replying to
    /// something on a feed.
    pub(super) fn begin_speaking_in(
        &mut self,
        key: &str,
        channel: &str,
        place_id: &str,
        upto: i64,
        bubble: Bubble,
        now: DateTime<Utc>,
    ) {
        self.agenda.speaking.insert(
            key.to_owned(),
            Speaking {
                channel: channel.to_owned(),
                place_id: place_id.to_owned(),
                upto,
                bubbles: VecDeque::from([bubble]),
                step: Step::Reading { until: now },
                chain: true,
            },
        );
    }

    /// Advance one place's speaking by whatever is due.
    pub(super) fn speak(&mut self, key: &str, now: DateTime<Utc>) -> Result<()> {
        let Some(speaking) = self.agenda.speaking.get(key) else {
            return Ok(());
        };
        let Some(front) = speaking.bubbles.front().cloned() else {
            self.agenda.speaking.remove(key);
            return Ok(());
        };
        let (channel, place_id, upto) = (
            speaking.channel.clone(),
            speaking.place_id.clone(),
            speaking.upto,
        );
        match speaking.step {
            Step::Reading { until } if now >= until => {
                let typing = self.tempo.compose(&front.text, &mut self.rng);
                self.set_step(
                    key,
                    Step::Typing {
                        until: after(now, typing),
                        nudge: now,
                    },
                );
                self.speak(key, now)
            }
            Step::Typing { until, .. } if now >= until => {
                let said_more = self
                    .ghost
                    .memory()
                    .place(key)?
                    .is_some_and(|p| p.last_in_seq > upto);
                if said_more {
                    self.agenda.speaking.remove(key);
                    self.record(Event::Decided {
                        about: key.to_owned(),
                        decision: "held back".into(),
                        reason: "they said more before she sent it".into(),
                    })?;
                    return self.schedule_attend(key, now);
                }
                self.set_step(key, Step::Sending);
                let body = self.body.clone();
                let place = key.to_owned();
                let machine = self.workshop.machine.clone();
                self.jobs.spawn(async move {
                    let act = match &front.file {
                        None => Ok(Act::Send {
                            place: place_id,
                            text: front.text.clone(),
                            reply_to: front.reply_to.clone(),
                        }),
                        // The file is read off her machine at the moment
                        // it is sent, so what arrives is what is there.
                        Some(path) => match &machine {
                            Some(machine) => machine
                                .read(path, LARGEST_FILE)
                                .await
                                .map(|data| Act::File {
                                    place: place_id,
                                    name: file_name(path),
                                    data,
                                    text: Some(front.text.clone()).filter(|t| !t.is_empty()),
                                })
                                .map_err(|e| format!("{path} could not be read: {e}")),
                            None => Err(format!("there is no machine to read {path} from")),
                        },
                    };
                    let result = match act {
                        Ok(act) => body.act(&channel, act).await,
                        Err(e) => Err(e),
                    };
                    Outcome::Sent {
                        place,
                        bubble: front,
                        result,
                    }
                });
                Ok(())
            }
            Step::Typing { until, nudge } if now >= nudge => {
                if self.can(&channel, Can::Typing) {
                    self.fire(&channel, Act::Typing { place: place_id });
                }
                self.set_step(
                    key,
                    Step::Typing {
                        until,
                        nudge: now + TYPING_LASTS,
                    },
                );
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// The body reported on a send.
    pub(super) fn on_sent(
        &mut self,
        key: &str,
        bubble: Bubble,
        result: std::result::Result<Acted, String>,
        now: DateTime<Utc>,
    ) -> Result<()> {
        // The key is all that is needed to journal it, so a send that
        // succeeded is recorded even if the state around it has gone.
        let Some((channel, place_id)) = social::split_key(key) else {
            return Ok(());
        };
        let (channel, place_id) = (channel.to_owned(), place_id.to_owned());
        let acted = match result {
            Ok(acted) => acted,
            Err(e) => {
                warn!("{key}: could not send: {e}");
                self.agenda.speaking.remove(key);
                self.record(Event::Decided {
                    about: key.to_owned(),
                    decision: "could not send".into(),
                    reason: e,
                })?;
                return Ok(());
            }
        };
        let sent_as = acted.id.clone();
        self.record(Event::Said {
            channel,
            place: place_id,
            id: acted.id,
            text: match &bubble.file {
                Some(path) => format!("[file: {}] {}", file_name(path), bubble.text)
                    .trim_end()
                    .to_owned(),
                None => bubble.text,
            },
            reply_to: bubble.reply_to,
        })?;
        let next = self.agenda.speaking.get_mut(key).and_then(|s| {
            s.bubbles.pop_front();
            let chain = s.chain;
            let next = s.bubbles.front_mut()?;
            if chain && next.reply_to.is_none() {
                next.reply_to = sent_as;
            }
            Some(next.clone())
        });
        let Some(next) = next else {
            self.agenda.speaking.remove(key);
            // Anything that arrived while she was talking is still unread.
            return self.schedule_attend(key, now);
        };
        let pause = self.tempo.between(&mut self.rng);
        let typing = self.tempo.compose(&next.text, &mut self.rng);
        self.set_step(
            key,
            Step::Typing {
                until: after(now, pause + typing),
                nudge: after(now, pause),
            },
        );
        Ok(())
    }

    fn set_step(&mut self, key: &str, step: Step) {
        if let Some(speaking) = self.agenda.speaking.get_mut(key) {
            speaking.step = step;
        }
    }
}
